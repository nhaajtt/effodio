//! Reads the files that wait in the index: each becomes segments the size the model handles well, each segment a
//! vector with its place in the file.
//!
//! Nothing long is squeezed into one point. A long file gets its start in the first pass, so that everything is
//! searchable soon, and the rest in a later pass.

use crate::kinds::{FileKind, Modality, SegmentKind};
use crate::readers::{images, media, office, pdf, Picture};
use crate::store::{FileRow, Segment, Store};
use crate::text;
use anyhow::{anyhow, bail, Result};
use serde_json::{Map, Value};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// What turns text, pictures and sound into vectors.
pub trait Embed {
    /// One vector per text, in order. Texts arrive with their prompt.
    fn texts(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    /// `budget`: how many soft tokens the picture may fill (70, 140, 280, 560 or 1120).
    fn image(&mut self, picture: &Picture, budget: usize) -> Result<Vec<f32>>;
    /// Mono samples at 16 kHz, 30 seconds at most.
    fn audio(&mut self, samples: &[f32]) -> Result<Vec<f32>>;
}

pub fn document_prompt(title: Option<&str>, text: &str) -> String {
    let title = title.filter(|t| !t.is_empty()).unwrap_or("none");
    format!("title: {title} | text: {text}")
}

pub fn query_prompt(text: &str) -> String {
    format!("task: search result | query: {text}")
}

/// Soft-token budgets: a screenshot's small text needs twice a photo's detail; a video has many frames.
pub const PHOTO_BUDGET: usize = 140;
pub const SCREENSHOT_BUDGET: usize = 280;
pub const PAGE_BUDGET: usize = 280;
pub const FRAME_BUDGET: usize = 70;

#[derive(Clone, Debug)]
pub struct Options {
    /// Read the text in pictures and scanned pages (Windows' OCR), for exact words.
    pub ocr: bool,
    /// How much of a long file the first pass reads.
    pub first_pages: usize,
    pub first_passages: usize,
    pub first_seconds: f64,
    /// Where the later pass stops.
    pub max_pages: usize,
    pub max_passages: usize,
    pub max_seconds: f64,
    /// A new video frame is kept when it differs this much from the last one kept.
    pub shot_change: f64,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            ocr: true,
            first_pages: 30,
            first_passages: 40,
            first_seconds: 30.0 * 60.0,
            max_pages: 5000,
            max_passages: 4000,
            max_seconds: 12.0 * 3600.0,
            shot_change: 0.06,
        }
    }
}

#[derive(Clone, Debug, serde::Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Started { total: usize },
    File { path: String, kind: FileKind, done: usize, total: usize, seconds: f64, segments: usize },
    Failed { path: String, error: String, done: usize, total: usize },
    /// The first pass is over; long files now get their later parts.
    Deepening { total: usize },
    Finished { indexed: usize, failed: usize, seconds: f64, stopped: bool },
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Summary {
    pub indexed: usize,
    pub failed: usize,
    pub stopped: bool,
}

/// What reading (part of) a file gave.
struct Read {
    segments: Vec<Segment>,
    info: Map<String, Value>,
    /// `Some(progress)`: more is left for the later pass, from there.
    more: Option<i64>,
}

pub struct Indexer<'a, E: Embed> {
    pub store: &'a mut Store,
    pub embed: &'a mut E,
    pub options: Options,
    pub stop: &'a AtomicBool,
    /// While set, the indexer waits between files.
    pub pause: &'a AtomicBool,
}

fn file_title(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().replace(['_', '-'], " ")).unwrap_or_default()
}

/// What keyword search knows a file by: its name and the two folders above it.
fn search_name(path: &Path) -> String {
    let mut parts: Vec<String> =
        path.ancestors().skip(1).take(2).filter_map(|p| p.file_name()).map(|n| n.to_string_lossy().into_owned()).collect();
    parts.reverse();
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    // "IMG_2214.jpg" also as "IMG 2214 jpg", so that parts of a name match.
    let spaced = name.replace(['_', '-', '.'], " ");
    parts.push(name);
    parts.push(spaced);
    parts.join(" ")
}

fn content_hash(path: &Path, size: u64) -> Option<String> {
    if size > 256 << 20 {
        return None;
    }
    let mut hasher = blake3::Hasher::new();
    let mut file = std::fs::File::open(path).ok()?;
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hasher.finalize().to_hex()[..32].to_string())
}

impl<'a, E: Embed> Indexer<'a, E> {
    fn halted(&self) -> bool {
        while self.pause.load(Ordering::Relaxed) && !self.stop.load(Ordering::Relaxed) {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
        self.stop.load(Ordering::Relaxed)
    }

    /// Reads every waiting file, then the later parts of long ones.
    pub fn run(&mut self, mut on_event: impl FnMut(Event)) -> Result<Summary> {
        let started = std::time::Instant::now();
        let mut summary = Summary::default();
        let counts = self.store.counts()?;
        let total = counts.pending as usize;
        on_event(Event::Started { total });
        let mut done = 0;
        'first: loop {
            let batch = self.store.waiting("pending", 64)?;
            if batch.is_empty() {
                break;
            }
            for file in batch {
                if self.halted() {
                    summary.stopped = true;
                    break 'first;
                }
                done += 1;
                self.one(&file, false, done, total.max(done), &mut summary, &mut on_event)?;
            }
        }
        if !summary.stopped {
            let total = self.store.counts()?.partial as usize;
            if total > 0 {
                on_event(Event::Deepening { total });
            }
            let mut done = 0;
            'later: loop {
                let batch = self.store.waiting("partial", 16)?;
                if batch.is_empty() {
                    break;
                }
                for file in batch {
                    if self.halted() {
                        summary.stopped = true;
                        break 'later;
                    }
                    done += 1;
                    self.one(&file, true, done, total.max(done), &mut summary, &mut on_event)?;
                }
            }
        }
        on_event(Event::Finished {
            indexed: summary.indexed,
            failed: summary.failed,
            seconds: started.elapsed().as_secs_f64(),
            stopped: summary.stopped,
        });
        Ok(summary)
    }

    fn one(
        &mut self, file: &FileRow, later: bool, done: usize, total: usize, summary: &mut Summary,
        on_event: &mut impl FnMut(Event),
    ) -> Result<()> {
        let started = std::time::Instant::now();
        let path = Path::new(&file.path);
        match self.read(file, later) {
            Ok(read) => {
                let hash = if later { None } else { content_hash(path, file.size) };
                let (state, progress) = match read.more {
                    Some(progress) => ("partial", progress),
                    None => ("done", 0),
                };
                let mut info = file.info.clone();
                info.extend(read.info);
                self.store.save(
                    file.id, &search_name(path), &read.segments, &info, state, progress, hash.as_deref(), !later,
                )?;
                summary.indexed += 1;
                on_event(Event::File {
                    path: file.path.clone(),
                    kind: file.kind,
                    done,
                    total,
                    seconds: started.elapsed().as_secs_f64(),
                    segments: read.segments.len(),
                });
            }
            Err(error) => {
                let message = format!("{error:#}");
                if later {
                    // The first part stays searchable; the rest is given up on.
                    self.store.db.execute("UPDATE files SET state = 'done', error = ? WHERE id = ?",
                                          rusqlite::params![message, file.id])?;
                } else {
                    self.store.fail(file.id, &message)?;
                }
                summary.failed += 1;
                on_event(Event::Failed { path: file.path.clone(), error: message, done, total });
            }
        }
        Ok(())
    }

    fn read(&mut self, file: &FileRow, later: bool) -> Result<Read> {
        let path = Path::new(&file.path);
        if !path.exists() {
            bail!("the file is gone");
        }
        let mut read = match file.kind {
            FileKind::Image | FileKind::Screenshot => self.picture(path, file.kind)?,
            FileKind::Pdf => self.pdf(path, file, later)?,
            FileKind::Doc => self.document(path, file, later)?,
            FileKind::Audio => self.audio(path, file, later)?,
            FileKind::Video => self.video(path, file, later)?,
        };
        if later {
            for segment in &mut read.segments {
                segment.late = true;
            }
        } else {
            read.segments.insert(0, Segment::new(SegmentKind::Name, Modality::None));
        }
        Ok(read)
    }

    fn ocr(&self, picture: &Picture) -> Option<String> {
        if !self.options.ocr {
            return None;
        }
        #[cfg(windows)]
        {
            let read = crate::readers::winrt::ocr(picture).ok()?;
            let letters = read.chars().filter(|c| c.is_alphanumeric()).count();
            // A stray "I" or "0" in a photo of a fence isn't text.
            (letters >= 8 && read.split_whitespace().count() >= 2).then_some(read)
        }
        #[cfg(not(windows))]
        {
            let _ = picture;
            None
        }
    }

    fn picture(&mut self, path: &Path, kind: FileKind) -> Result<Read> {
        let picture = images::load(path)?;
        let mut info = images::exif(path);
        info.insert("width".into(), picture.width.into());
        info.insert("height".into(), picture.height.into());
        crate::insight::look(&picture, &mut info);
        let budget = if kind == FileKind::Screenshot { SCREENSHOT_BUDGET } else { PHOTO_BUDGET };
        let mut whole = Segment::new(SegmentKind::Image, Modality::Image);
        whole.vector = Some(self.embed.image(&picture, budget)?);
        let mut segments = vec![whole];
        if let Some(read) = self.ocr(&picture) {
            if kind == FileKind::Screenshot {
                segments[0].excerpt = Some(text::excerpt(&read, 200));
            }
            let mut words = Segment::new(SegmentKind::Ocr, Modality::None);
            words.excerpt = Some(text::excerpt(&read, 200));
            words.text = Some(read);
            segments.push(words);
        }
        Ok(Read { segments, info, more: None })
    }

    /// Embeds the texts of `segments` that have words (each with the document prompt), in batches.
    fn embed_texts(&mut self, title: &str, segments: &mut [Segment]) -> Result<()> {
        let wanted: Vec<usize> = segments
            .iter()
            .enumerate()
            .filter(|(_, s)| s.vector.is_none() && s.text.as_deref().is_some_and(text::has_words))
            .map(|(index, _)| index)
            .collect();
        for batch in wanted.chunks(16) {
            let prompts: Vec<String> = batch
                .iter()
                .map(|&index| document_prompt(Some(title), segments[index].text.as_deref().unwrap_or_default()))
                .collect();
            let vectors = self.embed.texts(&prompts)?;
            if vectors.len() != batch.len() {
                bail!("the model returned {} vectors for {} texts", vectors.len(), batch.len());
            }
            for (&index, vector) in batch.iter().zip(vectors) {
                segments[index].vector = Some(vector);
                segments[index].modality = Modality::Text;
            }
        }
        Ok(())
    }

    fn pdf(&mut self, path: &Path, file: &FileRow, later: bool) -> Result<Read> {
        let texts = pdf::page_texts(path).unwrap_or_default();
        #[cfg(windows)]
        let rendered = crate::readers::winrt::PdfFile::open(path).ok();
        #[cfg(windows)]
        let pages = texts.len().max(rendered.as_ref().map_or(0, |r| r.pages()));
        #[cfg(not(windows))]
        let pages = texts.len();
        if pages == 0 {
            bail!("the PDF has no pages that can be read (it may need a password)");
        }
        let from = if later { file.progress.max(0) as usize } else { 0 };
        let until = if later { pages.min(self.options.max_pages) } else { pages.min(self.options.first_pages) };
        let title = file_title(path);
        let mut segments = Vec::new();
        for page in from..until {
            if self.stop.load(Ordering::Relaxed) {
                bail!("stopped");
            }
            let page_text = texts.get(page).map(String::as_str).unwrap_or_default();
            if pdf::has_text(page_text) {
                for (_, passage) in text::passages(page_text, text::PASSAGE) {
                    let mut segment = Segment::new(SegmentKind::Page, Modality::Text);
                    segment.loc = Some((page + 1) as f64);
                    segment.excerpt = Some(text::excerpt(&passage, 200));
                    segment.text = Some(passage);
                    segments.push(segment);
                }
                continue;
            }
            // Little or no text: a scan, a drawing, a slide. The page is read as a picture.
            #[cfg(windows)]
            if let Some(picture) = rendered.as_ref().and_then(|r| r.render(page, 1280).ok()) {
                if media::is_blank(&media::signature(&picture)) {
                    continue;
                }
                let mut segment = Segment::new(SegmentKind::Page, Modality::Image);
                segment.loc = Some((page + 1) as f64);
                segment.vector = Some(self.embed.image(&picture, PAGE_BUDGET)?);
                let words = if page_text.trim().is_empty() { self.ocr(&picture) } else { Some(page_text.to_string()) };
                if let Some(words) = words {
                    segment.excerpt = Some(text::excerpt(&words, 200));
                    segment.text = Some(words);
                }
                segments.push(segment);
                continue;
            }
            if !page_text.trim().is_empty() {
                let mut segment = Segment::new(SegmentKind::Page, Modality::Text);
                segment.loc = Some((page + 1) as f64);
                segment.excerpt = Some(text::excerpt(page_text, 200));
                segment.text = Some(page_text.to_string());
                segments.push(segment);
            }
        }
        self.embed_texts(&title, &mut segments)?;
        // A page read as a picture keeps its image vector; its words are for keyword search.
        let mut info = Map::new();
        info.insert("pages".into(), pages.into());
        let more = (!later && until < pages.min(self.options.max_pages)).then_some(until as i64);
        Ok(Read { segments, info, more })
    }

    fn document(&mut self, path: &Path, file: &FileRow, later: bool) -> Result<Read> {
        let parts = office::read(path)?;
        let title = file_title(path);
        let mut passages: Vec<(Option<String>, String)> = Vec::new();
        for part in &parts {
            for (_, passage) in text::passages(&part.text, text::PASSAGE) {
                passages.push((part.label.clone(), passage));
            }
        }
        if passages.is_empty() {
            bail!("no text in the file");
        }
        let total = passages.len();
        let from = if later { (file.progress.max(0) as usize).min(total) } else { 0 };
        let until = if later { total.min(self.options.max_passages) } else { total.min(self.options.first_passages) };
        let mut segments = Vec::new();
        for (index, (label, passage)) in passages.into_iter().enumerate().skip(from).take(until.saturating_sub(from)) {
            let mut segment = Segment::new(SegmentKind::Chunk, Modality::Text);
            segment.loc = Some((index + 1) as f64);
            let shown = text::excerpt(&passage, 200);
            segment.excerpt = Some(match &label {
                Some(label) => format!("{label} · {shown}"),
                None => shown,
            });
            segment.text = Some(passage);
            segments.push(segment);
        }
        self.embed_texts(&title, &mut segments)?;
        for segment in &mut segments {
            if segment.vector.is_none() {
                segment.modality = Modality::None;
            }
        }
        let mut info = Map::new();
        info.insert("passages".into(), total.into());
        let more = (!later && until < total.min(self.options.max_passages)).then_some(until as i64);
        Ok(Read { segments, info, more })
    }

    /// The seconds a pass covers: (from, until).
    fn span(&self, file: &FileRow, duration: f64, later: bool) -> (f64, f64) {
        if later {
            (file.progress.max(0) as f64, duration.min(self.options.max_seconds))
        } else {
            (0.0, duration.min(self.options.first_seconds))
        }
    }

    fn sound(&mut self, path: &Path, from: f64, until: f64, duration: f64, segments: &mut Vec<Segment>) -> Result<()> {
        // Windows start every 25 s; one that would start in the overlap of the last pass is where this one begins.
        for (start, samples) in media::windows(path, from)? {
            if start >= until {
                break;
            }
            if self.stop.load(Ordering::Relaxed) {
                bail!("stopped");
            }
            let mut segment = Segment::new(SegmentKind::Sound, Modality::Audio);
            segment.loc = Some(start);
            let end = start + samples.len() as f64 / media::SAMPLE_RATE as f64;
            segment.loc_end = Some(if duration > 0.0 { end.min(duration) } else { end });
            segment.vector = Some(self.embed.audio(&samples)?);
            segments.push(segment);
        }
        Ok(())
    }

    fn media_info(probe: &media::Probe) -> Map<String, Value> {
        let mut info = Map::new();
        info.insert("duration".into(), ((probe.duration * 10.0).round() / 10.0).into());
        if probe.has_video {
            info.insert("width".into(), probe.width.into());
            info.insert("height".into(), probe.height.into());
        }
        if let Some(title) = &probe.title {
            info.insert("title".into(), title.clone().into());
        }
        if let Some(artist) = &probe.artist {
            info.insert("artist".into(), artist.clone().into());
        }
        info
    }

    /// A song's title and artist, for keyword search.
    fn tags(probe: &media::Probe) -> Option<Segment> {
        let words: Vec<&str> = [probe.title.as_deref(), probe.artist.as_deref()].into_iter().flatten().collect();
        if words.is_empty() {
            return None;
        }
        let mut segment = Segment::new(SegmentKind::Ocr, Modality::None);
        segment.text = Some(words.join(" · "));
        Some(segment)
    }

    fn audio(&mut self, path: &Path, file: &FileRow, later: bool) -> Result<Read> {
        let probe = media::probe(path)?;
        if !probe.has_audio {
            bail!("no sound in the file");
        }
        let (from, until) = self.span(file, probe.duration.max(1.0), later);
        let mut segments = Vec::new();
        if !later {
            segments.extend(Self::tags(&probe));
        }
        self.sound(path, from, until, probe.duration, &mut segments)?;
        if segments.iter().all(|s| s.vector.is_none()) && !later {
            bail!("the recording is silent");
        }
        let more = (!later && until < probe.duration.min(self.options.max_seconds)).then_some(until as i64);
        Ok(Read { segments, info: Self::media_info(&probe), more })
    }

    fn video(&mut self, path: &Path, file: &FileRow, later: bool) -> Result<Read> {
        let probe = media::probe(path)?;
        if !probe.has_video {
            // A recording in a video container.
            return self.audio(path, file, later);
        }
        let duration = probe.duration.max(1.0);
        let (from, until) = self.span(file, duration, later);
        let mut segments = Vec::new();
        if !later {
            segments.extend(Self::tags(&probe));
        }
        let mut last: Option<[u8; 256]> = None;
        for (second, frame) in media::frames(path, from)? {
            if second >= until {
                break;
            }
            if self.stop.load(Ordering::Relaxed) {
                bail!("stopped");
            }
            let signature = media::signature(&frame);
            if media::is_blank(&signature) {
                continue;
            }
            if last.as_ref().is_some_and(|last| media::difference(last, &signature) < self.options.shot_change) {
                continue;
            }
            last = Some(signature);
            let mut segment = Segment::new(SegmentKind::Frame, Modality::Image);
            segment.loc = Some(second);
            segment.vector = Some(self.embed.image(&frame, FRAME_BUDGET)?);
            segments.push(segment);
        }
        if probe.has_audio {
            self.sound(path, from, until, probe.duration, &mut segments)?;
        }
        if segments.iter().all(|s| s.vector.is_none()) && !later {
            return Err(anyhow!("nothing to see or hear in the video"));
        }
        let more = (!later && until < duration.min(self.options.max_seconds)).then_some(until as i64);
        Ok(Read { segments, info: Self::media_info(&probe), more })
    }
}

/// An embedder for tests: every word is its own random direction, and a text is the sum of its words', so texts
/// that share words are close and unrelated ones spread as real embeddings do. Pictures are told apart by their
/// average colour, sound by its loudness.
pub struct FakeEmbed;

impl FakeEmbed {
    pub fn vector(words: &str) -> Vec<f32> {
        let mut vector = vec![0f32; crate::store::DIMENSION];
        for word in text::fold(words).split(|c: char| !c.is_alphanumeric()).filter(|w| w.len() > 2) {
            let mut bytes = vec![0u8; crate::store::DIMENSION];
            blake3::Hasher::new().update(word.as_bytes()).finalize_xof().fill(&mut bytes);
            for (value, byte) in vector.iter_mut().zip(bytes) {
                *value += byte as f32 / 127.5 - 1.0;
            }
        }
        normalize(vector)
    }
}

pub fn normalize(mut vector: Vec<f32>) -> Vec<f32> {
    let length = vector.iter().map(|v| (v * v) as f64).sum::<f64>().sqrt() as f32;
    if length > 0.0 {
        for value in &mut vector {
            *value /= length;
        }
    }
    vector
}

impl Embed for FakeEmbed {
    fn texts(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        Ok(texts
            .iter()
            .map(|text| {
                // The prompt's own words are the same for every text.
                let body = text.rsplit_once(" | text: ").or_else(|| text.rsplit_once(" | query: ")).map_or(text.as_str(), |p| p.1);
                Self::vector(body)
            })
            .collect())
    }

    fn image(&mut self, picture: &Picture, _budget: usize) -> Result<Vec<f32>> {
        let mut sums = [0f64; 3];
        for pixel in picture.rgb.as_chunks::<3>().0 {
            for channel in 0..3 {
                sums[channel] += pixel[channel] as f64;
            }
        }
        let mut vector = vec![0f32; crate::store::DIMENSION];
        for channel in 0..3 {
            vector[channel] = (sums[channel] / (picture.rgb.len() / 3).max(1) as f64 / 255.0) as f32 + 0.01;
        }
        Ok(normalize(vector))
    }

    fn audio(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        let mut vector = vec![0f32; crate::store::DIMENSION];
        vector[10] = 1.0;
        vector[11] = samples.iter().map(|s| s.abs()).sum::<f32>() / samples.len().max(1) as f32;
        Ok(normalize(vector))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crawler::{crawl, Root};
    use crate::rules::Custom;
    use std::fs;

    fn indexed(folder: &Path, options: Options) -> (Store, Summary, Vec<Event>) {
        let mut store = Store::open(&folder.join("index.db")).unwrap();
        let roots = vec![Root { path: folder.join("files"), exclude: vec![] }];
        let found = crawl(&roots, &Custom::default(), &AtomicBool::new(false));
        store.reconcile(&[folder.join("files")], &found.files).unwrap();
        let (stop, pause) = (AtomicBool::new(false), AtomicBool::new(false));
        let mut events = Vec::new();
        let summary = {
            let mut embed = FakeEmbed;
            let mut indexer = Indexer { store: &mut store, embed: &mut embed, options, stop: &stop, pause: &pause };
            indexer.run(|event| events.push(event)).unwrap()
        };
        (store, summary, events)
    }

    #[test]
    fn documents_and_pictures_become_segments() {
        let folder = tempfile::tempdir().unwrap();
        let files = folder.path().join("files");
        fs::create_dir_all(&files).unwrap();
        fs::write(files.join("lease.txt"), "The lease allows pets with a deposit of two hundred dollars. ".repeat(4)).unwrap();
        fs::write(files.join("numbers.csv"), "1,2,3,4\n".repeat(200)).unwrap();
        fs::write(files.join("broken.docx"), "not a zip").unwrap();
        image::RgbImage::from_fn(200, 200, |x, y| image::Rgb([(x * 7919 % 251) as u8, (y * 104729 % 241) as u8, ((x * y) % 239) as u8]))
            .save(files.join("beach.png")).unwrap();
        let options = Options { ocr: false, ..Default::default() };
        let (store, summary, events) = indexed(folder.path(), options);
        assert_eq!(summary, Summary { indexed: 3, failed: 1, stopped: false });
        assert!(matches!(events.first(), Some(Event::Started { total: 4 })));
        assert!(matches!(events.last(), Some(Event::Finished { indexed: 3, failed: 1, .. })));
        let counts = store.counts().unwrap();
        assert_eq!((counts.pending, counts.failed), (0, 1));
        // lease: name + 1 passage with a vector; numbers: name + passage without one; beach: name + image.
        assert_eq!(counts.vectors, 2);
        let found: i64 = store.db.query_row("SELECT COUNT(*) FROM fts WHERE fts MATCH 'deposit'", [], |r| r.get(0)).unwrap();
        assert_eq!(found, 1);
        let by_name: i64 = store.db.query_row("SELECT COUNT(*) FROM fts WHERE fts MATCH 'name:beach'", [], |r| r.get(0)).unwrap();
        assert_eq!(by_name, 1);
        assert!(store.failures(5).unwrap()[0].0.ends_with("broken.docx"));
        let hashed: i64 = store.db.query_row("SELECT COUNT(*) FROM files WHERE hash IS NOT NULL", [], |r| r.get(0)).unwrap();
        assert_eq!(hashed, 3);
    }

    #[test]
    fn long_documents_finish_in_a_later_pass() {
        let folder = tempfile::tempdir().unwrap();
        let files = folder.path().join("files");
        fs::create_dir_all(&files).unwrap();
        let paragraph = "Chapter notes about the harbour, the tide tables and the ferry that leaves at dawn. ";
        fs::write(files.join("book.txt"), paragraph.repeat(200)).unwrap();
        let options = Options { ocr: false, first_passages: 3, ..Default::default() };
        let (store, summary, events) = indexed(folder.path(), options);
        // Once in the first pass and once more for the rest.
        assert_eq!(summary.indexed, 2);
        assert!(events.iter().any(|e| matches!(e, Event::Deepening { total: 1 })));
        let (early, late): (i64, i64) = store.db.query_row(
            "SELECT SUM(late = 0 AND kind = 'chunk'), SUM(late = 1) FROM segments", [], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
        assert_eq!(early, 3);
        assert!(late >= 5);
        let total: i64 = store.db.query_row("SELECT COUNT(DISTINCT loc) FROM segments WHERE kind = 'chunk'", [], |r| r.get(0)).unwrap();
        assert_eq!(total, early + late);
        assert_eq!(store.counts().unwrap().partial, 0);
    }

    #[test]
    fn stopping_leaves_the_rest_pending() {
        let folder = tempfile::tempdir().unwrap();
        let files = folder.path().join("files");
        fs::create_dir_all(&files).unwrap();
        for index in 0..5 {
            fs::write(files.join(format!("note {index}.txt")), "A note with enough words to be worth a vector, surely.").unwrap();
        }
        let mut store = Store::open(&folder.path().join("index.db")).unwrap();
        let found = crawl(&[Root { path: files.clone(), exclude: vec![] }], &Custom::default(), &AtomicBool::new(false));
        store.reconcile(&[files], &found.files).unwrap();
        let (stop, pause) = (AtomicBool::new(false), AtomicBool::new(false));
        let mut embed = FakeEmbed;
        let mut indexer = Indexer { store: &mut store, embed: &mut embed, options: Options::default(), stop: &stop, pause: &pause };
        let summary = indexer.run(|event| {
            if matches!(event, Event::File { done: 2, .. }) {
                stop.store(true, Ordering::Relaxed);
            }
        }).unwrap();
        assert!(summary.stopped);
        assert_eq!(summary.indexed, 2);
        assert_eq!(store.counts().unwrap().pending, 3);
    }
}
