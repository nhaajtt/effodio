//! Hybrid search: meaning (the model's vectors) and words (the keyword index), on one scale.
//!
//! Every vector is scored against the query anyway, so each query gets its own scale for free: a segment's meaning
//! score is how far its similarity stands out from the others of its kind, in standard deviations. Only segments
//! that stand out count as matches by meaning, which also gives an honest "nothing found". Words rescue what a vector
//! can't hold exactly: codes, numbers and names.

use crate::kinds::{FileKind, Modality, SegmentKind};
use crate::query::Query;
use crate::store::{Store, DIMENSION};
use crate::text;
use anyhow::Result;
use half::slice::HalfFloatSliceExt;
use rayon::prelude::*;
use rusqlite::params;
use serde::Serialize;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Serialize)]
pub struct Hit {
    pub file: i64,
    pub path: String,
    pub kind: FileKind,
    /// For ordering only.
    pub score: f64,
    /// How far the best segment stands out for this query.
    pub z: Option<f64>,
    pub cosine: Option<f64>,
    /// Share of the query's words found in the file.
    pub words: f64,
    pub segment: i64,
    pub segment_kind: SegmentKind,
    /// A PDF page, a passage's number, or seconds into a recording.
    pub loc: Option<f64>,
    pub loc_end: Option<f64>,
    /// Matched words are wrapped in « ».
    pub excerpt: Option<String>,
    pub modified: f64,
    pub size: u64,
    pub info: serde_json::Map<String, serde_json::Value>,
    /// False for results that pass the gates but trail far behind the best: shown folded away.
    pub strong: bool,
}

/// One segment of a file against a query, for the strip that shows where in a recording the matches are.
#[derive(Clone, Debug, Serialize)]
pub struct Moment {
    pub segment: i64,
    pub kind: SegmentKind,
    pub loc: f64,
    pub loc_end: Option<f64>,
    pub z: f64,
}

#[derive(Clone, Debug)]
pub struct Settings {
    pub limit: usize,
    /// A segment matches by meaning from this many deviations up.
    pub gate: f64,
    /// How much finding the query's words counts against meaning.
    pub word_weight: f64,
    /// The last word is still being typed: match it as the start of words.
    pub prefix_last: bool,
    /// How much of a `-word`'s similarity is taken off.
    pub negative_weight: f32,
    /// A match by meaning needs at least this similarity, whatever its place among the others: in a small index
    /// something always stands out, even for words that are nowhere. (Measured on this model: what a query is
    /// about scores 0.70 and up, what it isn't about 0.60 and down.)
    pub floor: f64,
    /// A segment this close to the best one's similarity matches too, without clearing the gate: with few files
    /// the scale is rough, and the runner-up of two near-equal answers shouldn't vanish.
    pub near: f64,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { limit: 30, gate: 2.0, word_weight: 2.0, prefix_last: false, negative_weight: 0.6, floor: 0.62, near: 0.025 }
    }
}

pub(crate) struct FileMeta {
    pub(crate) kind: FileKind,
    /// Folded, for `in:` filters.
    pub(crate) path: String,
    pub(crate) extension: String,
    pub(crate) modified: f64,
    /// A picture's main colours, for `color:` filters.
    pub(crate) tones: Vec<String>,
}

pub struct Searcher {
    pub store: Store,
    pub(crate) halves: Vec<half::f16>,
    pub(crate) row_file: Vec<i64>,
    pub(crate) row_segment: Vec<i64>,
    pub(crate) row_modality: Vec<Modality>,
    pub(crate) row_sets_scale: Vec<bool>,
    pub(crate) files: HashMap<i64, FileMeta>,
    /// Who is in which files, by folded name, for `person:` filters.
    pub(crate) people: HashMap<String, HashSet<i64>>,
    pub(crate) loaded: bool,
}

/// On the map, a file is tied to at most this many others whose centred vector is at least `TIE_FLOOR` close to its
/// own; a file with none that close is tied to the one nearest it, if that one is at least `TIE_WEAK` close.
const TIES_EACH: usize = 3;
const TIE_FLOOR: f32 = 0.15;
const TIE_WEAK: f32 = 0.04;

const STOPWORDS: &[&str] = &[
    "a", "an", "the", "of", "in", "on", "at", "to", "for", "with", "and", "or", "my", "me", "that", "this", "these",
    "those", "from", "about", "where", "what", "which", "who", "when", "is", "was", "are", "were", "be", "it", "its",
    "some", "any", "find", "show", "file", "files", "photo", "photos", "picture", "pictures", "image", "images",
    "screenshot", "screenshots", "video", "videos", "pdf", "document", "documents", "recording",
    // Vietnamese, folded
    "cua", "va", "la", "cho", "trong", "tim", "cai", "nhung", "cac", "mot", "co", "tai", "ve", "toi", "nao", "day",
    "kia", "voi", "anh", "hinh", "tep", "tai lieu",
];

/// Kind words in a query ("…in a video", "ảnh chụp màn hình…"): a soft boost, not a filter.
const KIND_WORDS: &[(&str, FileKind)] = &[
    ("screenshot", FileKind::Screenshot), ("screenshots", FileKind::Screenshot), ("photo", FileKind::Image),
    ("photos", FileKind::Image), ("picture", FileKind::Image), ("pictures", FileKind::Image),
    ("image", FileKind::Image), ("images", FileKind::Image), ("pdf", FileKind::Pdf), ("pdfs", FileKind::Pdf),
    ("paper", FileKind::Pdf), ("papers", FileKind::Pdf), ("document", FileKind::Doc), ("documents", FileKind::Doc),
    ("note", FileKind::Doc), ("notes", FileKind::Doc), ("spreadsheet", FileKind::Doc), ("slides", FileKind::Doc),
    ("video", FileKind::Video), ("videos", FileKind::Video), ("clip", FileKind::Video), ("movie", FileKind::Video),
    ("audio", FileKind::Audio), ("song", FileKind::Audio), ("podcast", FileKind::Audio), ("voice", FileKind::Audio),
    ("sound", FileKind::Audio), ("recording", FileKind::Audio),
    ("anh", FileKind::Image), ("hinh", FileKind::Image), ("phim", FileKind::Video), ("nhac", FileKind::Audio),
];

/// The content words of a query, folded as the keyword index keeps its text. Kind words and stopwords are dropped:
/// they say what to look for, not what's in it.
pub fn terms(text_in: &str) -> Vec<String> {
    let folded = text::fold(text_in);
    let mut seen = HashSet::new();
    text::words(&folded)
        .filter(|word| word.chars().count() >= 2 && !STOPWORDS.contains(word))
        .filter(|word| seen.insert(word.to_string()))
        .map(str::to_string)
        .collect()
}

pub fn kind_hints(text_in: &str) -> HashSet<FileKind> {
    let folded = text::fold(text_in);
    let mut out = HashSet::new();
    if folded.contains("chup man hinh") || folded.contains("screen shot") {
        out.insert(FileKind::Screenshot);
    }
    if folded.contains("ghi am") {
        out.insert(FileKind::Audio);
    }
    for word in text::words(&folded) {
        if let Some((_, kind)) = KIND_WORDS.iter().find(|(name, _)| *name == word) {
            out.insert(*kind);
        }
    }
    out
}

/// Codes and IDs ("K7Q2LM", "TRX50") and long numbers ("4815162342") mean nothing to an embedding but everything
/// to keyword search: a query made of them weighs words double and meaning half.
pub fn is_lookup(terms: &[String]) -> bool {
    !terms.is_empty()
        && terms.iter().all(|term| {
            term.chars().any(char::is_numeric) && (term.chars().any(char::is_alphabetic) || term.chars().count() >= 4)
        })
}

pub(crate) fn dot(a: &[f32], b: &[f32]) -> f32 {
    let mut sums = [0f32; 8];
    for (x, y) in a.chunks_exact(8).zip(b.chunks_exact(8)) {
        for lane in 0..8 {
            sums[lane] += x[lane] * y[lane];
        }
    }
    sums.iter().sum()
}

/// A query's meaning scores: each similarity minus the average of its modality (text, image and audio vectors sit at
/// slightly different levels for the same query), in standard deviations of all of them together. The scale comes
/// from the rows in `sets_scale`: the later pages of long files are scored on it but don't move it, or a book's
/// hundreds of pages would pull the text average down for every query.
pub fn meaning(scores: &[f32], modality: &[Modality], sets_scale: &[bool]) -> Vec<f64> {
    let mut sum = [0f64; 4];
    let mut count = [0usize; 4];
    for row in 0..scores.len() {
        if sets_scale[row] {
            sum[modality[row].index()] += scores[row] as f64;
            count[modality[row].index()] += 1;
        }
    }
    let all = sum.iter().sum::<f64>() / count.iter().sum::<usize>().max(1) as f64;
    // Small groups take the average of all.
    let means: Vec<f64> = (0..4).map(|m| if count[m] >= 20 { sum[m] / count[m] as f64 } else { all }).collect();
    let centered: Vec<f64> = (0..scores.len()).map(|row| scores[row] as f64 - means[modality[row].index()]).collect();
    let (mut n, mut total) = (0f64, 0f64);
    for row in 0..centered.len() {
        if sets_scale[row] {
            n += 1.0;
            total += centered[row];
        }
    }
    let mean = total / n.max(1.0);
    let mut squares = 0f64;
    for row in 0..centered.len() {
        if sets_scale[row] {
            squares += (centered[row] - mean) * (centered[row] - mean);
        }
    }
    let deviation = (squares / n.max(1.0)).sqrt().max(0.02);
    centered.into_iter().map(|value| (value - mean) / deviation).collect()
}

impl Searcher {
    /// `load_vectors: false` keeps only keyword search ready; the vectors load with the first search by meaning.
    pub fn new(store: Store, load_vectors: bool) -> Result<Searcher> {
        let mut searcher = Searcher {
            store,
            halves: Vec::new(),
            row_file: Vec::new(),
            row_segment: Vec::new(),
            row_modality: Vec::new(),
            row_sets_scale: Vec::new(),
            files: HashMap::new(),
            people: HashMap::new(),
            loaded: false,
        };
        if load_vectors {
            searcher.reload()?;
        } else {
            searcher.reload_files()?;
        }
        Ok(searcher)
    }

    pub fn vector_count(&self) -> usize {
        self.row_file.len()
    }

    pub fn vectors_loaded(&self) -> bool {
        self.loaded
    }

    /// Frees the vectors' memory; keyword search keeps working.
    pub fn release_vectors(&mut self) {
        self.halves = Vec::new();
        self.row_file = Vec::new();
        self.row_segment = Vec::new();
        self.row_modality = Vec::new();
        self.row_sets_scale = Vec::new();
        self.loaded = false;
    }

    fn reload_files(&mut self) -> Result<()> {
        self.files.clear();
        let mut select = self.store.db.prepare("SELECT id, kind, path, mtime, info FROM files")?;
        let mut rows = select.query([])?;
        while let Some(row) = rows.next()? {
            let path: String = row.get(2)?;
            let extension = std::path::Path::new(&path)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            self.files.insert(row.get(0)?, FileMeta {
                kind: FileKind::parse(&row.get::<_, String>(1)?).unwrap_or(FileKind::Doc),
                path: text::fold(&path),
                extension,
                modified: row.get(3)?,
                tones: crate::insight::tones_in(row.get_ref(4)?.as_str().unwrap_or_default()),
            });
        }
        drop(rows);
        drop(select);
        self.people = crate::insight::people(&self.store).unwrap_or_default();
        Ok(())
    }

    /// Reads the files and all vectors again (after indexing added some).
    pub fn reload(&mut self) -> Result<()> {
        self.release_vectors();
        self.reload_files()?;
        let rows: i64 =
            self.store.db.query_row("SELECT COUNT(*) FROM segments WHERE vec IS NOT NULL", [], |row| row.get(0))?;
        let mut halves: Vec<half::f16> = Vec::with_capacity(rows as usize * DIMENSION);
        {
            let mut select = self
                .store
                .db
                .prepare("SELECT id, file, modality, late, vec FROM segments WHERE vec IS NOT NULL ORDER BY id")?;
            let mut found = select.query([])?;
            while let Some(row) = found.next()? {
                let blob = row.get_ref(4)?.as_blob()?;
                if blob.len() != DIMENSION * 2 {
                    continue;
                }
                halves.extend(blob.chunks_exact(2).map(|pair| half::f16::from_le_bytes([pair[0], pair[1]])));
                self.row_segment.push(row.get(0)?);
                self.row_file.push(row.get(1)?);
                self.row_modality.push(Modality::parse(&row.get::<_, String>(2)?));
                self.row_sets_scale.push(row.get::<_, i64>(3)? == 0);
            }
        }
        self.halves = halves;
        self.loaded = true;
        Ok(())
    }

    /// Cosine similarity of `query` with every stored vector (all are normalized): a matrix-vector product, a block
    /// of rows at a time, each block turned into float32 first.
    pub(crate) fn scores(&self, query: &[f32]) -> Vec<f32> {
        const BLOCK: usize = 512;
        self.halves
            .par_chunks(BLOCK * DIMENSION)
            .flat_map_iter(|block| {
                let mut floats = vec![0f32; block.len()];
                block.convert_to_f32_slice(&mut floats);
                floats.chunks_exact(DIMENSION).map(|row| dot(row, query)).collect::<Vec<f32>>()
            })
            .collect()
    }

    fn allowed(&self, file: i64, query: &Query) -> bool {
        let Some(meta) = self.files.get(&file) else { return false };
        (query.kinds.is_empty() || query.kinds.contains(&meta.kind))
            && query.folders.iter().all(|folder| meta.path.contains(folder.as_str()))
            && (query.extensions.is_empty() || query.extensions.contains(&meta.extension))
            && query.after.map_or(true, |after| meta.modified >= after)
            && query.before.map_or(true, |before| meta.modified <= before)
            && query.colors.iter().all(|color| meta.tones.iter().any(|tone| tone == color))
            && query.people.iter().all(|name| self.people.get(name).is_some_and(|files| files.contains(&file)))
    }

    fn is_indexed(&self, pattern: &str) -> bool {
        self.store
            .db
            .query_row("SELECT 1 FROM fts WHERE fts MATCH ? LIMIT 1", [pattern], |_| Ok(()))
            .is_ok()
    }

    /// The stored vector of a segment, for "more like this".
    pub fn vector_of(&self, segment: i64) -> Result<Option<Vec<f32>>> {
        let blob: Option<Vec<u8>> = self
            .store
            .db
            .query_row("SELECT vec FROM segments WHERE id = ?", [segment], |row| row.get(0))
            .unwrap_or(None);
        Ok(blob.map(|bytes| crate::store::from_half(&bytes)))
    }

    /// The first segment of a file that has a vector (a picture's own, a document's first passage).
    pub fn first_vector_segment(&self, file: i64) -> Option<i64> {
        self.store
            .db
            .query_row("SELECT id FROM segments WHERE file = ? AND vec IS NOT NULL ORDER BY id LIMIT 1", [file], |row| {
                row.get(0)
            })
            .ok()
    }

    /// `vector`: the query's embedding (its text with the query prompt, or a picture or sound to search by).
    /// `negative`: the embedding of what `-words` name. `exclude_file`: left out of the results ("more like this").
    pub fn search(
        &mut self, query: &Query, vector: Option<&[f32]>, negative: Option<&[f32]>, exclude_file: Option<i64>,
        settings: &Settings,
    ) -> Result<Vec<Hit>> {
        if vector.is_some() && !self.loaded {
            self.reload()?;
        }
        // Meaning: every vector on this query's scale, each file's best segment kept.
        let mut best: HashMap<i64, (f64, f64, usize)> = HashMap::new();
        if let Some(vector) = vector.filter(|v| v.len() == DIMENSION && !self.row_file.is_empty()) {
            let mut scores = self.scores(vector);
            if let Some(negative) = negative.filter(|n| n.len() == DIMENSION) {
                let against = self.scores(negative);
                for (score, away) in scores.iter_mut().zip(against) {
                    *score -= settings.negative_weight * away.max(0.0);
                }
            }
            let z = meaning(&scores, &self.row_modality, &self.row_sets_scale);
            for row in 0..scores.len() {
                let file = self.row_file[row];
                if Some(file) == exclude_file || !self.allowed(file, query) {
                    continue;
                }
                if best.get(&file).map_or(true, |&(known, _, _)| z[row] > known) {
                    best.insert(file, (z[row], scores[row] as f64, row));
                }
            }
        }

        // Words: the keyword index over names, folders, document and page text, and text read from pictures.
        let terms = terms(&query.text);
        let mut patterns: Vec<String> = terms
            .iter()
            .enumerate()
            .map(|(index, term)| {
                let prefix = settings.prefix_last && index + 1 == terms.len();
                format!("\"{term}\"{}", if prefix { "*" } else { "" })
            })
            .collect();
        // A word that's in no file as typed matches as the start of longer words ("grok" finds "Grok4"); one that is
        // stays whole, so "cat" doesn't bring in "category".
        for (index, term) in terms.iter().enumerate() {
            if term.chars().count() >= 3 && !patterns[index].ends_with('*') && !self.is_indexed(&patterns[index]) {
                // The index keeps stems ("renovation" as "renov"), so a word cut short while typing ("renovat")
                // is the start of none: a few letters fewer may be.
                // A code or a number is never cut: "55102" must not find "55120394".
                let letters: Vec<char> = term.chars().collect();
                let cuts = if term.chars().any(char::is_numeric) { 0 } else { 3usize };
                let shorter = (0..=cuts)
                    .filter(|cut| letters.len() - cut.min(&letters.len()) >= 3)
                    .map(|cut| format!("\"{}\"*", letters[..letters.len() - cut].iter().collect::<String>()))
                    .find(|pattern| self.is_indexed(pattern));
                patterns[index] = shorter.unwrap_or_else(|| format!("\"{term}\"*"));
            }
        }
        struct WordMatch {
            segment: i64,
            snippet: Option<String>,
            rank: usize,
        }
        let mut words: HashMap<i64, WordMatch> = HashMap::new();
        let mut coverage: HashMap<i64, f64> = HashMap::new();
        if !patterns.is_empty() {
            let mut select = self.store.db.prepare(
                "SELECT s.file, s.id, fts.original, s.kind FROM fts JOIN segments s ON s.id = fts.rowid
                 WHERE fts MATCH ? ORDER BY bm25(fts, 3.0, 1.0) LIMIT 2000",
            )?;
            let mut rows = select.query([patterns.join(" OR ")])?;
            while let Some(row) = rows.next()? {
                let file: i64 = row.get(0)?;
                if words.len() >= 200 || words.contains_key(&file) || Some(file) == exclude_file
                    || !self.allowed(file, query)
                {
                    continue;
                }
                let original: Option<String> = row.get(2)?;
                let is_name = row.get::<_, String>(3)? == "name";
                let snippet = if is_name { None } else { original.and_then(|text| text::snippet(&text, &terms, 90)) };
                let rank = words.len() + 1;
                words.insert(file, WordMatch { segment: row.get(1)?, snippet, rank });
            }
            drop(rows);
            if !words.is_empty() {
                let files: Vec<String> = words.keys().map(i64::to_string).collect();
                let sql = format!(
                    "SELECT DISTINCT s.file FROM fts JOIN segments s ON s.id = fts.rowid
                     WHERE fts MATCH ? AND s.file IN ({})",
                    files.join(",")
                );
                let mut each = self.store.db.prepare(&sql)?;
                for pattern in &patterns {
                    let mut rows = each.query([pattern])?;
                    while let Some(row) = rows.next()? {
                        *coverage.entry(row.get(0)?).or_insert(0.0) += 1.0 / patterns.len() as f64;
                    }
                }
            }
        }

        // Files that must or mustn't hold exact words.
        let mut required: Option<HashSet<i64>> = None;
        for phrase in &query.phrases {
            let folded: Vec<String> = text::words(&text::fold(phrase)).map(str::to_string).collect();
            if folded.is_empty() {
                continue;
            }
            let found = self.files_matching(&format!("\"{}\"", folded.join(" ")))?;
            required = Some(match required {
                Some(known) => known.intersection(&found).copied().collect(),
                None => found,
            });
        }
        let mut banned: HashSet<i64> = HashSet::new();
        for word in &query.negative {
            let folded: Vec<String> = text::words(&text::fold(word)).map(str::to_string).collect();
            if !folded.is_empty() {
                banned.extend(self.files_matching(&format!("\"{}\"", folded.join(" ")))?);
            }
        }

        // A file qualifies by standing out in meaning, or by holding at least half of the query's words. Its score:
        // meaning + a bonus that grows with the share of words found, + 1 for the kind the query asks for. One
        // common word can't beat a strong match by meaning; all the words of an exact lookup usually do.
        let lookup = is_lookup(&terms);
        let (meaning_weight, words_weight) =
            if lookup { (0.5, settings.word_weight * 2.0) } else { (1.0, settings.word_weight) };
        let hinted = kind_hints(&query.text);
        let floor = if negative.is_some() { f64::MIN } else { settings.floor };
        // The best of those that clear the gate; with none, nothing is near it either.
        let top_cosine = best
            .values()
            .filter(|&&(z, cosine, _)| z >= settings.gate && cosine >= floor)
            .map(|&(_, cosine, _)| cosine)
            .fold(f64::NAN, f64::max);
        let by_meaning = |z: f64, cosine: f64| {
            cosine >= floor && (z >= settings.gate || (z >= 1.0 && cosine >= top_cosine - settings.near))
        };
        let mut candidates: HashSet<i64> =
            best.iter().filter(|(_, &(z, cosine, _))| by_meaning(z, cosine)).map(|(&file, _)| file).collect();
        candidates.extend(coverage.iter().filter(|(_, &share)| share >= 0.5).map(|(&file, _)| file));
        if let Some(required) = &required {
            // An exact phrase is enough to qualify, and nothing without it does.
            candidates.extend(required.iter().filter(|file| self.allowed(**file, query) && Some(**file) != exclude_file));
            candidates.retain(|file| required.contains(file));
        }
        candidates.retain(|file| !banned.contains(file));
        // Filters alone ("kind:video in:Trip"): the newest files that pass them.
        if query.is_empty() && vector.is_none() && query.has_filters() {
            candidates = self.files.keys().copied().filter(|&file| self.allowed(file, query)).collect();
        }
        let mut scored: Vec<(i64, f64)> = candidates
            .into_iter()
            .map(|file| {
                let z = best.get(&file).map_or(0.0, |&(z, _, _)| z.max(0.0));
                let share = coverage.get(&file).copied().unwrap_or(0.0);
                let mut score = meaning_weight * z + words_weight * share.powf(1.5);
                if self.files.get(&file).is_some_and(|meta| hinted.contains(&meta.kind)) {
                    score += 1.0;
                }
                if required.as_ref().is_some_and(|r| r.contains(&file)) {
                    score += settings.word_weight;
                }
                if query.is_empty() && vector.is_none() {
                    score = self.files.get(&file).map_or(0.0, |meta| meta.modified);
                }
                (file, score)
            })
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
        scored.truncate(settings.limit);

        let top = scored.first().map_or(0.0, |&(_, score)| score);
        let mut hits = Vec::with_capacity(scored.len());
        for (file, score) in scored {
            let semantic = best.get(&file).copied();
            let keyword = words.get(&file);
            // Point at the best segment by meaning if it stands out; else at the segment the words matched.
            let by_meaning = semantic.is_some_and(|(z, cosine, _)| by_meaning(z, cosine) || keyword.is_none());
            let segment = match (by_meaning, semantic, keyword) {
                (true, Some((_, _, row)), _) => self.row_segment[row],
                (_, _, Some(found)) => found.segment,
                (_, Some((_, _, row)), None) => self.row_segment[row],
                (_, None, None) => match self.any_segment(file) {
                    Some(segment) => segment,
                    None => continue,
                },
            };
            let Some(row) = self.store.file(file)? else { continue };
            let (kind_text, loc, loc_end, stored): (String, Option<f64>, Option<f64>, Option<String>) =
                self.store.db.query_row(
                    "SELECT kind, loc, loc_end, excerpt FROM segments WHERE id = ?",
                    [segment],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )?;
            // Quote the matched words only when they come from the segment pointed at, so a hit on page 5 never
            // shows page 2's words. A picture with no text of its own may borrow them from its OCR segment.
            let mut snippet = keyword.filter(|found| found.segment == segment).and_then(|found| found.snippet.clone());
            if snippet.is_none() && loc.is_none() {
                snippet = keyword.and_then(|found| found.snippet.clone());
            }
            if snippet.is_none() && !terms.is_empty() {
                // The segment may hold the words even though another one ranked first for them.
                if let Some(original) = self.text_of(segment)? {
                    snippet = text::snippet(&original, &terms, 90);
                }
            }
            hits.push(Hit {
                file,
                path: row.path,
                kind: row.kind,
                score,
                z: semantic.map(|(z, _, _)| z),
                cosine: semantic.map(|(_, cosine, _)| cosine),
                words: coverage.get(&file).copied().unwrap_or(0.0),
                segment,
                segment_kind: SegmentKind::parse(&kind_text),
                loc,
                loc_end,
                excerpt: snippet.or(stored),
                modified: row.modified,
                size: row.size,
                info: row.info,
                strong: top <= 0.0 || score >= top * 0.5 || keyword.is_some_and(|found| found.rank <= 3 && lookup),
            });
        }
        Ok(hits)
    }

    fn any_segment(&self, file: i64) -> Option<i64> {
        self.store
            .db
            .query_row(
                "SELECT id FROM segments WHERE file = ? ORDER BY (kind = 'name'), id LIMIT 1",
                [file],
                |row| row.get(0),
            )
            .ok()
    }

    fn files_matching(&self, pattern: &str) -> Result<HashSet<i64>> {
        let mut select = self
            .store
            .db
            .prepare("SELECT DISTINCT s.file FROM fts JOIN segments s ON s.id = fts.rowid WHERE fts MATCH ?")?;
        let rows = select.query_map([pattern], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The text of a page, a passage or what was read in a picture, as written.
    pub fn text_of(&self, segment: i64) -> Result<Option<String>> {
        Ok(self
            .store
            .db
            .query_row("SELECT original FROM fts WHERE rowid = ?", [segment], |row| row.get(0))
            .unwrap_or(None))
    }

    /// Every placed segment of a file against a query, on the query's scale: where in a video or a recording the
    /// matches are.
    pub fn moments(&mut self, file: i64, vector: &[f32]) -> Result<Vec<Moment>> {
        if !self.loaded {
            self.reload()?;
        }
        if vector.len() != DIMENSION || self.row_file.is_empty() {
            return Ok(Vec::new());
        }
        let scores = self.scores(vector);
        let z = meaning(&scores, &self.row_modality, &self.row_sets_scale);
        let mut out = Vec::new();
        let mut select = self.store.db.prepare("SELECT kind, loc, loc_end FROM segments WHERE id = ?")?;
        for row in 0..scores.len() {
            if self.row_file[row] != file {
                continue;
            }
            let (kind, loc, loc_end): (String, Option<f64>, Option<f64>) =
                select.query_row(params![self.row_segment[row]], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?;
            if let Some(loc) = loc {
                out.push(Moment { segment: self.row_segment[row], kind: SegmentKind::parse(&kind), loc, loc_end, z: z[row] });
            }
        }
        out.sort_by(|a, b| a.loc.partial_cmp(&b.loc).unwrap_or(std::cmp::Ordering::Equal));
        Ok(out)
    }

    /// Groups of files that are the same picture or nearly: byte-identical copies, and pictures whose vectors are
    /// closer than `threshold` (resized, recompressed or lightly edited versions).
    pub fn duplicates(&mut self, threshold: f32, limit: usize) -> Result<Vec<Vec<i64>>> {
        if !self.loaded {
            self.reload()?;
        }
        let mut parent: HashMap<i64, i64> = HashMap::new();
        fn find(parent: &mut HashMap<i64, i64>, mut x: i64) -> i64 {
            while let Some(&next) = parent.get(&x) {
                if next == x {
                    break;
                }
                x = next;
            }
            x
        }
        let join = |parent: &mut HashMap<i64, i64>, a: i64, b: i64| {
            parent.entry(a).or_insert(a);
            parent.entry(b).or_insert(b);
            let (ra, rb) = (find(parent, a), find(parent, b));
            if ra != rb {
                parent.insert(ra.max(rb), ra.min(rb));
            }
        };
        {
            let mut select = self.store.db.prepare(
                "SELECT group_concat(id) FROM files WHERE hash IS NOT NULL GROUP BY hash, size HAVING COUNT(*) > 1",
            )?;
            let mut rows = select.query([])?;
            while let Some(row) = rows.next()? {
                let ids: Vec<i64> = row.get::<_, String>(0)?.split(',').filter_map(|id| id.parse().ok()).collect();
                for pair in ids.windows(2) {
                    join(&mut parent, pair[0], pair[1]);
                }
            }
        }
        // Whole pictures only: one vector per file, so a pair of rows is a pair of files.
        let pictures: Vec<usize> = (0..self.row_file.len())
            .filter(|&row| {
                self.row_modality[row] == Modality::Image
                    && self.files.get(&self.row_file[row]).is_some_and(|m| {
                        matches!(m.kind, FileKind::Image | FileKind::Screenshot)
                    })
            })
            .take(20_000)
            .collect();
        let floats: Vec<Vec<f32>> = pictures
            .iter()
            .map(|&row| {
                let mut out = vec![0f32; DIMENSION];
                self.halves[row * DIMENSION..(row + 1) * DIMENSION].convert_to_f32_slice(&mut out);
                out
            })
            .collect();
        let pairs: Vec<(usize, usize)> = (0..floats.len())
            .into_par_iter()
            .flat_map_iter(|a| {
                let floats = &floats;
                (a + 1..floats.len()).filter(move |&b| dot(&floats[a], &floats[b]) >= threshold).map(move |b| (a, b))
            })
            .collect();
        for (a, b) in pairs {
            join(&mut parent, self.row_file[pictures[a]], self.row_file[pictures[b]]);
        }
        let mut groups: HashMap<i64, Vec<i64>> = HashMap::new();
        let keys: Vec<i64> = parent.keys().copied().collect();
        for file in keys {
            let root = find(&mut parent, file);
            groups.entry(root).or_default().push(file);
        }
        let mut out: Vec<Vec<i64>> = groups.into_values().filter(|group| group.len() > 1).collect();
        for group in &mut out {
            group.sort();
        }
        out.sort_by(|a, b| b.len().cmp(&a.len()).then(a[0].cmp(&b[0])));
        out.truncate(limit);
        Ok(out)
    }

    /// Every file as a point on a plane: its first vector projected onto the two directions the index varies most
    /// along (found by power iteration). Files that mean similar things land near each other.
    pub fn map(&mut self, limit: usize) -> Result<Vec<(i64, f32, f32)>> {
        Ok(self.chart(limit)?.0)
    }

    /// The map's points, and the ties between them: each file with the few files that mean most nearly the same
    /// (by the cosine of their centred vectors), when they're near enough to call related.
    #[allow(clippy::type_complexity)]
    pub fn chart(&mut self, limit: usize) -> Result<(Vec<(i64, f32, f32)>, Vec<(i64, i64, f32)>)> {
        if !self.loaded {
            self.reload()?;
        }
        let mut seen = HashSet::new();
        let rows: Vec<usize> = (0..self.row_file.len())
            .filter(|&row| self.row_sets_scale[row] && seen.insert(self.row_file[row]))
            .take(limit)
            .collect();
        if rows.len() < 3 {
            return Ok((Vec::new(), Vec::new()));
        }
        let mut data: Vec<Vec<f32>> = rows
            .iter()
            .map(|&row| {
                let mut out = vec![0f32; DIMENSION];
                self.halves[row * DIMENSION..(row + 1) * DIMENSION].convert_to_f32_slice(&mut out);
                out
            })
            .collect();
        // Each modality is centred on its own mean, or the map would only show text apart from pictures.
        for modality in Modality::ALL {
            let members: Vec<usize> =
                (0..rows.len()).filter(|&i| self.row_modality[rows[i]] == modality).collect();
            if members.is_empty() {
                continue;
            }
            let mut mean = vec![0f32; DIMENSION];
            for &i in &members {
                for (m, v) in mean.iter_mut().zip(&data[i]) {
                    *m += v / members.len() as f32;
                }
            }
            for &i in &members {
                for (v, m) in data[i].iter_mut().zip(&mean) {
                    *v -= m;
                }
            }
        }
        let component = |data: &[Vec<f32>], avoid: Option<&[f32]>| -> Vec<f32> {
            let mut direction: Vec<f32> = (0..DIMENSION).map(|i| ((i * 7919 + 13) % 101) as f32 / 101.0 - 0.5).collect();
            for _ in 0..30 {
                let projections: Vec<f32> = data.par_iter().map(|row| dot(row, &direction)).collect();
                let mut next = vec![0f32; DIMENSION];
                for (row, &p) in data.iter().zip(&projections) {
                    for (n, v) in next.iter_mut().zip(row) {
                        *n += v * p;
                    }
                }
                if let Some(avoid) = avoid {
                    let along = dot(&next, avoid);
                    for (n, a) in next.iter_mut().zip(avoid) {
                        *n -= along * a;
                    }
                }
                let length = next.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-12);
                direction = next.into_iter().map(|v| v / length).collect();
            }
            direction
        };
        let first = component(&data, None);
        let second = component(&data, Some(&first));
        let points: Vec<(i64, f32, f32)> = rows
            .iter()
            .zip(&data)
            .map(|(&row, vector)| (self.row_file[row], dot(vector, &first), dot(vector, &second)))
            .collect();

        let lengths: Vec<f32> = data.iter().map(|vector| dot(vector, vector).sqrt().max(1e-12)).collect();
        let nearest: Vec<Vec<(usize, f32)>> = (0..data.len())
            .into_par_iter()
            .map(|one| {
                let mut best: Vec<(usize, f32)> = (0..data.len())
                    .filter(|&other| other != one)
                    .map(|other| (other, dot(&data[one], &data[other]) / (lengths[one] * lengths[other])))
                    .filter(|&(_, cosine)| cosine >= TIE_WEAK)
                    .collect();
                best.sort_by(|a, b| b.1.total_cmp(&a.1));
                best.truncate(TIES_EACH);
                // Its clearly related files; failing any, the one nearest it, as a weaker tie.
                let strong = best.iter().take_while(|&&(_, cosine)| cosine >= TIE_FLOOR).count();
                best.truncate(strong.max(1));
                best
            })
            .collect();
        let mut tied = HashSet::new();
        let mut links = Vec::new();
        for (one, list) in nearest.iter().enumerate() {
            for &(other, cosine) in list {
                let pair = (one.min(other), one.max(other));
                if tied.insert(pair) {
                    links.push((self.row_file[rows[pair.0]], self.row_file[rows[pair.1]], cosine));
                }
            }
        }
        Ok((points, links))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexer::{normalize, FakeEmbed};
    use crate::query::parse;
    use crate::store::{Seen, Segment};

    /// A store with documents (name, text) of the given kinds, embedded with the fake embedder.
    fn library(folder: &std::path::Path, files: &[(&str, FileKind, &[&str])]) -> Searcher {
        let mut store = Store::open(&folder.join("index.db")).unwrap();
        let seen: Vec<Seen> = files
            .iter()
            .enumerate()
            .map(|(index, (name, kind, _))| Seen {
                path: format!("C:\\lib\\{name}").into(),
                root: "C:\\lib".into(),
                kind: *kind,
                size: 100 + index as u64,
                modified: 1_000_000.0 + index as f64 * 86_400.0,
            })
            .collect();
        store.reconcile(&[], &seen).unwrap();
        for (name, _, texts) in files {
            let row = store.file_by_path(&format!("C:\\lib\\{name}")).unwrap().unwrap();
            let mut segments = vec![Segment::new(SegmentKind::Name, Modality::None)];
            for (index, text) in texts.iter().enumerate() {
                let mut segment = Segment::new(SegmentKind::Chunk, Modality::Text);
                segment.loc = Some((index + 1) as f64);
                segment.text = Some(text.to_string());
                segment.excerpt = Some(text.to_string());
                segment.vector = Some(FakeEmbed::vector(text));
                segments.push(segment);
            }
            store.save(row.id, name, &segments, &Default::default(), "done", 0, None, true).unwrap();
        }
        Searcher::new(store, true).unwrap()
    }

    fn filler() -> Vec<(String, String)> {
        let topics = ["garden tomatoes watering", "bicycle repair chain", "winter soup recipe", "tax return form",
            "guitar chords practice", "mountain hiking trail", "birthday party invitation", "laptop battery warranty",
            "ocean sailing knots", "bread baking flour", "chess opening theory", "camera lens aperture",
            "train timetable europe", "yoga morning stretch", "coffee roasting beans", "paint colours bedroom",
            "language grammar verbs", "budget spreadsheet rent", "football match tickets", "piano sonata sheet",
            "dentist appointment reminder", "movie review thriller", "weather forecast storm", "car insurance renewal",
            "wedding seating plan"];
        topics.iter().enumerate().map(|(i, topic)| (format!("filler {i}.txt"), format!("Notes on {topic} and more {topic}."))).collect()
    }

    fn search(searcher: &mut Searcher, text: &str) -> Vec<Hit> {
        let query = parse(text, 2_000_000.0);
        let vector = (!query.text.is_empty()).then(|| FakeEmbed::vector(&query.text));
        let negative = (!query.negative.is_empty()).then(|| FakeEmbed::vector(&query.negative.join(" ")));
        searcher.search(&query, vector.as_deref(), negative.as_deref(), None, &Settings { floor: 0.0, ..Settings::default() }).unwrap()
    }

    fn names(hits: &[Hit]) -> Vec<String> {
        hits.iter().map(|hit| hit.path.rsplit('\\').next().unwrap().to_string()).collect()
    }

    #[test]
    fn meaning_scores_are_relative_to_each_kind() {
        let mut scores = vec![0.30f32; 40];
        let mut modality = vec![Modality::Text; 40];
        for row in 20..40 {
            scores[row] = 0.10;
            modality[row] = Modality::Image;
        }
        scores[5] = 0.50;
        scores[25] = 0.30;
        let z = meaning(&scores, &modality, &vec![true; 40]);
        // Both stand out from their own kind by the same amount, though one cosine is far higher.
        assert!((z[5] - z[25]).abs() < 0.5, "{} {}", z[5], z[25]);
        assert!(z[5] > 3.0);
        assert!(z[0].abs() < 1.0);
    }

    #[test]
    fn meaning_and_words_rank_together() {
        let folder = tempfile::tempdir().unwrap();
        let filler = filler();
        let mut files: Vec<(&str, FileKind, &[&str])> = vec![
            ("tenancy.txt", FileKind::Doc, &["The lease clause about pets: dogs and cats are allowed with a deposit."]),
            ("invoice.txt", FileKind::Doc, &["Invoice INV-20418 for the kitchen renovation, total due in thirty days."]),
            ("hoa-don.txt", FileKind::Doc, &["Hóa đơn tiền điện tháng mười, tổng cộng phải thanh toán trước ngày 15."]),
        ];
        let owned: Vec<[&str; 1]> = filler.iter().map(|(_, text)| [text.as_str()]).collect();
        for (index, (name, _)) in filler.iter().enumerate() {
            files.push((name.as_str(), FileKind::Doc, &owned[index]));
        }
        let mut searcher = library(folder.path(), &files);
        assert_eq!(names(&search(&mut searcher, "clause about pets in the lease"))[0], "tenancy.txt");
        // An exact code: found by its words.
        let hits = search(&mut searcher, "20418");
        assert_eq!(names(&hits)[0], "invoice.txt");
        assert!(hits[0].excerpt.as_deref().unwrap().contains("«20418»"), "{:?}", hits[0].excerpt);
        // Without accents, and as the start of a word.
        assert_eq!(names(&search(&mut searcher, "hoa don tien dien"))[0], "hoa-don.txt");
        assert_eq!(names(&search(&mut searcher, "renovat"))[0], "invoice.txt");
        // By file name.
        assert_eq!(names(&search(&mut searcher, "tenancy"))[0], "tenancy.txt");
        // A code that's in no file doesn't find the nearest other code.
        assert!(search(&mut searcher, "20419").iter().all(|hit| hit.words == 0.0));
        // Nothing stands out for words that are nowhere.
        assert!(search(&mut searcher, "zzyzx quuxly").is_empty());
    }

    #[test]
    fn filters_phrases_and_exclusions() {
        let folder = tempfile::tempdir().unwrap();
        let filler = filler();
        let mut files: Vec<(&str, FileKind, &[&str])> = vec![
            ("beach-dog.txt", FileKind::Doc, &["A dog runs on the beach at golden hour, chasing waves."]),
            ("beach-cat.txt", FileKind::Doc, &["A cat sleeps on the beach towel in the golden hour light."]),
            ("beach.mp4", FileKind::Video, &["Dog on the beach, golden light on the sand."]),
        ];
        let owned: Vec<[&str; 1]> = filler.iter().map(|(_, text)| [text.as_str()]).collect();
        for (index, (name, _)) in filler.iter().enumerate() {
            files.push((name.as_str(), FileKind::Doc, &owned[index]));
        }
        let mut searcher = library(folder.path(), &files);
        let all = names(&search(&mut searcher, "beach golden"));
        assert!(all.len() >= 3 && all[..3].iter().all(|name| name.starts_with("beach")), "{all:?}");
        assert_eq!(names(&search(&mut searcher, "beach golden kind:video")), vec!["beach.mp4"]);
        assert_eq!(names(&search(&mut searcher, "beach golden ext:mp4")), vec!["beach.mp4"]);
        let without = names(&search(&mut searcher, "beach golden -cat"));
        assert!(!without.contains(&"beach-cat.txt".to_string()), "{without:?}");
        assert_eq!(names(&search(&mut searcher, "\"beach towel\"")), vec!["beach-cat.txt"]);
        // Filters alone list what passes them, newest first.
        assert_eq!(names(&search(&mut searcher, "kind:video")), vec!["beach.mp4"]);
        // Files are a day apart from 1,000,000 s: the third one on is after day 2.
        let recent = search(&mut searcher, "beach golden after:1970-01-14");
        assert_eq!(names(&recent), vec!["beach.mp4"]);
    }

    #[test]
    fn more_like_this_and_moments() {
        let folder = tempfile::tempdir().unwrap();
        let filler = filler();
        let mut files: Vec<(&str, FileKind, &[&str])> = vec![
            ("talk.mp3", FileKind::Audio, &["Welcome and introductions.", "How sleep affects memory and learning.", "Questions from the audience."]),
            ("sleep.txt", FileKind::Doc, &["Sleep, memory and learning: notes from the lecture."]),
        ];
        let owned: Vec<[&str; 1]> = filler.iter().map(|(_, text)| [text.as_str()]).collect();
        for (index, (name, _)) in filler.iter().enumerate() {
            files.push((name.as_str(), FileKind::Doc, &owned[index]));
        }
        let mut searcher = library(folder.path(), &files);
        let hits = search(&mut searcher, "sleep memory learning");
        let talk = hits.iter().find(|hit| hit.path.ends_with("talk.mp3")).unwrap();
        // The hit points at the passage that matched, not the file's start.
        assert_eq!(talk.loc, Some(2.0));
        let moments = searcher.moments(talk.file, &FakeEmbed::vector("sleep memory learning")).unwrap();
        assert_eq!(moments.len(), 3);
        assert!(moments[1].z > moments[0].z && moments[1].z > moments[2].z);
        // More like the lecture notes: the talk, and not the notes themselves.
        let notes = hits.iter().find(|hit| hit.path.ends_with("sleep.txt")).unwrap();
        let vector = searcher.vector_of(searcher.first_vector_segment(notes.file).unwrap()).unwrap().unwrap();
        let similar = searcher.search(&Query::default(), Some(&vector), None, Some(notes.file), &Settings { floor: 0.0, ..Settings::default() }).unwrap();
        assert_eq!(names(&similar)[0], "talk.mp3");
        assert!(!names(&similar).contains(&"sleep.txt".to_string()));
    }

    #[test]
    fn duplicates_and_the_map() {
        let folder = tempfile::tempdir().unwrap();
        let mut store = Store::open(&folder.path().join("index.db")).unwrap();
        let seen: Vec<Seen> = (0..6)
            .map(|index| Seen {
                path: format!("C:\\lib\\p{index}.jpg").into(),
                root: "C:\\lib".into(),
                kind: FileKind::Image,
                size: 5000,
                modified: 10.0,
            })
            .collect();
        store.reconcile(&[], &seen).unwrap();
        for index in 0..6 {
            let row = store.file_by_path(&format!("C:\\lib\\p{index}.jpg")).unwrap().unwrap();
            let mut vector = vec![0f32; DIMENSION];
            // 0 and 1 are near copies; the others point elsewhere.
            vector[if index <= 1 { 0 } else { index * 10 }] = 1.0;
            if index == 1 {
                vector[1] = 0.05;
            }
            let mut segment = Segment::new(SegmentKind::Image, Modality::Image);
            segment.vector = Some(normalize(vector));
            let hash = if index >= 4 { Some("samebytes") } else { None };
            store.save(row.id, "p", &[segment], &Default::default(), "done", 0, hash, true).unwrap();
        }
        let mut searcher = Searcher::new(store, true).unwrap();
        let groups = searcher.duplicates(0.97, 10).unwrap();
        assert_eq!(groups.len(), 2);
        assert!(groups.iter().all(|group| group.len() == 2));
        let map = searcher.map(100).unwrap();
        let (points, links) = searcher.chart(100).unwrap();
        assert_eq!(points.len(), map.len());
        assert!(links.iter().all(|(a, b, cosine)| a != b && *cosine >= TIE_WEAK));
        assert_eq!(map.len(), 6);
        assert!(map.iter().all(|(_, x, y)| x.is_finite() && y.is_finite()));
    }
}
