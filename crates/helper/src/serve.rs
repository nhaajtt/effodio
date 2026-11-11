//! Answers searches. The text part of the model loads with the first search and stays while searches keep coming; the
//! image and audio encoders load only for a search by picture or sound, and go again soon after.
//!
//! One JSON object per line in (`{"id": 1, "op": "search", "q": "dog on the beach"}`), one per line out
//! (`{"id": 1, "ok": true, "result": {...}}`).

use crate::{device, emit, open_model, vector_source};
use anyhow::{anyhow, bail, Result};
use glint_core::config::{self, Config};
use glint_core::indexer::query_prompt;
use glint_core::kinds::FileKind;
use glint_core::query::{self, Query};
use glint_core::readers::{images, media, Picture};
use glint_core::search::{Searcher, Settings};
use glint_core::store::Store;
use glint_embed::{Device, Embedder};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::BufRead;
use std::path::Path;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// Without a search for this long, the helper quits (the app starts it again when needed).
const IDLE: Duration = Duration::from_secs(600);
/// The encoders for pictures and sound take about 0.8 GB: they go this long after their last use.
const MEDIA_IDLE: Duration = Duration::from_secs(120);

pub struct Session {
    searcher: Searcher,
    config: Config,
    text: Option<Embedder>,
    full: Option<(Embedder, Instant)>,
    /// Query vectors by their text: typing a query asks for the same words again and again.
    remembered: HashMap<String, Vec<f32>>,
    data_version: i64,
}

/// What the tidy-up looks for among the photos, in words the model reads.
const TIDY: &[(&str, &str)] = &[
    ("blurry", "a blurry, out of focus, shaky photo"),
    ("receipts", "a photo of a receipt, a bill or an invoice"),
    ("papers", "a photo of a printed page, a form or a whiteboard"),
    ("memes", "a meme: a funny picture with a caption"),
    ("dark", "a photo that came out almost completely black"),
];

/// What an album can be about: the words the model reads, then its name in English and in Vietnamese.
const SUBJECTS: &[(&str, &str, &str)] = &[
    ("the beach and the sea", "Beach", "Biển"),
    ("mountains and hills", "Mountains", "Núi"),
    ("a forest, trees and plants", "Nature", "Thiên nhiên"),
    ("flowers", "Flowers", "Hoa"),
    ("a sunset or a sunrise", "Sunsets", "Hoàng hôn"),
    ("a city street and buildings", "City", "Phố xá"),
    ("a building's architecture", "Architecture", "Kiến trúc"),
    ("the inside of a home, rooms and furniture", "Home", "Nhà cửa"),
    ("food and drinks on a table", "Food", "Đồ ăn"),
    ("a cat or a dog, pets", "Pets", "Thú cưng"),
    ("wild animals and birds", "Animals", "Động vật"),
    ("a portrait of one person", "Portraits", "Chân dung"),
    ("a selfie", "Selfies", "Ảnh tự chụp"),
    ("a group of people together", "Together", "Tụ họp"),
    ("a baby or small children", "Children", "Trẻ nhỏ"),
    ("a wedding", "Wedding", "Đám cưới"),
    ("a party or a birthday", "Parties", "Tiệc tùng"),
    ("a concert or a stage", "Shows", "Sân khấu"),
    ("sport and exercise", "Sport", "Thể thao"),
    ("cars and motorbikes", "Vehicles", "Xe cộ"),
    ("an airplane, an airport, travelling", "Travel", "Du lịch"),
    ("snow and winter", "Winter", "Mùa đông"),
    ("the night, lights in the dark", "Night", "Đêm"),
    ("a drawing, a painting or an illustration", "Art", "Tranh vẽ"),
    ("a chart, a graph or a diagram", "Charts", "Biểu đồ"),
    ("a map", "Maps", "Bản đồ"),
    ("a printed page or a document", "Papers", "Giấy tờ"),
    ("a receipt or an invoice", "Receipts", "Hóa đơn"),
    ("a product photo, something for sale", "Things", "Đồ vật"),
    ("clothes and fashion", "Clothes", "Quần áo"),
    ("a computer screen with an app or a website", "Screens", "Màn hình"),
    ("a chat conversation with message bubbles", "Chats", "Tin nhắn"),
    ("a video game", "Games", "Trò chơi"),
    ("a meme with a caption", "Memes", "Ảnh chế"),
    ("a logo or an icon", "Logos", "Biểu trưng"),
    ("work at an office, a desk, a meeting", "Work", "Công việc"),
    ("a school, a classroom, studying", "School", "Học tập"),
];

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

impl Session {
    pub fn open(database: &Path, config: Config) -> Result<Session> {
        let store = Store::open(database)?;
        store.claim_vector_source(&vector_source())?;
        // Saved searches remember the newest file they had been shown.
        let _ = store.db.execute("ALTER TABLE collections ADD COLUMN mark INTEGER NOT NULL DEFAULT 0", []);
        crate::faces::prepare(&store.db)?;
        let searcher = Searcher::new(store, false)?;
        Ok(Session { searcher, config, text: None, full: None, remembered: HashMap::new(), data_version: -1 })
    }

    /// The index changed under us (the indexer added files): read it again before the next search.
    fn refresh(&mut self) -> Result<()> {
        let version: i64 = self.searcher.store.db.query_row("PRAGMA data_version", [], |row| row.get(0))?;
        if version != self.data_version {
            if self.data_version != -1 || self.searcher.vectors_loaded() {
                self.searcher.reload()?;
            }
            self.data_version = version;
        }
        Ok(())
    }

    fn embed_query(&mut self, text: &str) -> Result<Vec<f32>> {
        if let Some(vector) = self.remembered.get(text) {
            return Ok(vector.clone());
        }
        if self.text.is_none() {
            // Searches always run on the processor: the panel never waits for a graphics card to wake up.
            self.text = Some(open_model(true, Device::Cpu)?);
        }
        let vector = self.text.as_mut().unwrap().embed_text(&query_prompt(text))?;
        if self.remembered.len() > 256 {
            self.remembered.clear();
        }
        self.remembered.insert(text.to_string(), vector.clone());
        Ok(vector)
    }

    fn full_model(&mut self) -> Result<&mut Embedder> {
        if self.full.is_none() {
            self.full = Some((open_model(false, device(&self.config))?, Instant::now()));
        }
        let (model, used) = self.full.as_mut().unwrap();
        *used = Instant::now();
        Ok(model)
    }

    pub fn release_idle(&mut self) {
        if self.full.as_ref().is_some_and(|(_, used)| used.elapsed() > MEDIA_IDLE) {
            self.full = None;
        }
    }

    /// The vector of a picture or a recording to search by.
    fn embed_file(&mut self, path: &Path) -> Result<Vec<f32>> {
        match glint_core::kinds::kind_of(path) {
            Some(glint_core::kinds::FileKind::Image | glint_core::kinds::FileKind::Screenshot) => {
                let picture = images::load(path)?;
                self.full_model()?.embed_image(&picture.rgb, picture.width, picture.height, 140)
            }
            Some(glint_core::kinds::FileKind::Audio) => {
                let (_, samples) =
                    media::windows(path, 0.0)?.next().ok_or_else(|| anyhow!("the recording is silent"))?;
                self.full_model()?.embed_audio(&samples)
            }
            Some(glint_core::kinds::FileKind::Video) => {
                let picture: Picture = media::frame_at(path, 1.0, 448)?;
                self.full_model()?.embed_image(&picture.rgb, picture.width, picture.height, 140)
            }
            _ => bail!("search by example takes a picture, a recording or a video"),
        }
    }

    fn settings(request: &Value) -> Settings {
        Settings {
            limit: request["limit"].as_u64().unwrap_or(30).clamp(1, 500) as usize,
            prefix_last: request["typing"].as_bool().unwrap_or(false),
            ..Settings::default()
        }
    }

    fn hits(&mut self, request: &Value, query: &Query, vector: Option<Vec<f32>>, exclude: Option<i64>) -> Result<Value> {
        let started = Instant::now();
        let negative = if query.negative.is_empty() {
            None
        } else {
            Some(self.embed_query(&query.negative.join(" "))?)
        };
        let hits = self.searcher.search(query, vector.as_deref(), negative.as_deref(), exclude, &Self::settings(request))?;
        Ok(json!({
            "hits": hits,
            "ms": started.elapsed().as_secs_f64() * 1000.0,
            "vectors": self.searcher.vector_count(),
            "filters": {
                "kinds": query.kinds, "folders": query.folders, "extensions": query.extensions,
                "after": query.after, "before": query.before, "negative": query.negative, "phrases": query.phrases,
            },
        }))
    }

    fn member(&self, id: i64) -> Result<Option<Value>> {
        Ok(self.searcher.store.file(id)?.map(|row| {
            json!({
                "file": row.id, "path": row.path, "kind": row.kind, "size": row.size,
                "modified": row.modified, "info": row.info,
            })
        }))
    }

    fn members(&self, ids: &[i64]) -> Result<Vec<Value>> {
        let mut out = Vec::new();
        for &id in ids {
            out.extend(self.member(id)?);
        }
        Ok(out)
    }

    fn newest_file(&self) -> i64 {
        self.searcher.store.db.query_row("SELECT COALESCE(MAX(id), 0) FROM files", [], |row| row.get(0)).unwrap_or(0)
    }

    /// The files a saved search finds that came after `mark`.
    fn fresh(&mut self, words: &str, mark: i64) -> Result<usize> {
        let query = query::parse(words, now());
        let vector = if query.text.trim().is_empty() { None } else { Some(self.embed_query(&query.text)?) };
        let settings = Settings { limit: 200, ..Settings::default() };
        let hits = self.searcher.search(&query, vector.as_deref(), None, None, &settings)?;
        Ok(hits.iter().filter(|hit| hit.strong && hit.file > mark).count())
    }

    pub fn handle(&mut self, request: &Value) -> Result<Value> {
        self.release_idle();
        let op = request["op"].as_str().unwrap_or_default();
        if let Some(answer) = crate::faces::handle(&self.searcher.store, request) {
            // Names may have changed: `person:` filters read them.
            self.searcher.reload_people();
            return answer;
        }
        match op {
            "search" => {
                self.refresh()?;
                let started = Instant::now();
                let mut query = query::parse(request["q"].as_str().unwrap_or_default(), now());
                // A name someone gave a face, typed among the words, asks for that person's photos.
                let (rest, people) = self.searcher.names_in(&query.text);
                if !people.is_empty() {
                    query.text = rest;
                    query.people.extend(people);
                }
                let vector = if query.text.trim().is_empty() { None } else { Some(self.embed_query(&query.text)?) };
                let embed_ms = started.elapsed().as_secs_f64() * 1000.0;
                let mut result = self.hits(request, &query, vector, None)?;
                result["embed_ms"] = json!(embed_ms);
                result["ms"] = json!(started.elapsed().as_secs_f64() * 1000.0);
                Ok(result)
            }
            // More like a result: its own vector is the query.
            "similar" => {
                self.refresh()?;
                let (segment, file) = match (request["segment"].as_i64(), request["file"].as_i64()) {
                    (Some(segment), file) => (segment, file),
                    (None, Some(file)) => (
                        self.searcher.first_vector_segment(file).ok_or_else(|| anyhow!("nothing to compare with"))?,
                        Some(file),
                    ),
                    _ => bail!("similar needs a segment or a file"),
                };
                let vector = self.searcher.vector_of(segment)?.ok_or_else(|| anyhow!("nothing to compare with"))?;
                let query = query::parse(request["q"].as_str().unwrap_or_default(), now());
                let filters_only = Query { text: String::new(), phrases: Vec::new(), negative: Vec::new(), ..query };
                self.hits(request, &filters_only, Some(vector), file)
            }
            // Search by example: a picture, a recording or a video from anywhere on the computer.
            "example" => {
                self.refresh()?;
                let path = request["path"].as_str().ok_or_else(|| anyhow!("example needs a path"))?;
                let vector = self.embed_file(Path::new(path))?;
                let query = query::parse(request["q"].as_str().unwrap_or_default(), now());
                let filters_only = Query { text: String::new(), phrases: Vec::new(), negative: Vec::new(), ..query };
                let known = self.searcher.store.file_by_path(path)?.map(|row| row.id);
                self.hits(request, &filters_only, Some(vector), known)
            }
            // Where in a file the query matches: the strip under a video or a recording.
            "moments" => {
                self.refresh()?;
                let file = request["file"].as_i64().ok_or_else(|| anyhow!("moments needs a file"))?;
                let query = query::parse(request["q"].as_str().unwrap_or_default(), now());
                if query.text.trim().is_empty() {
                    return Ok(json!({"moments": []}));
                }
                let vector = self.embed_query(&query.text)?;
                Ok(json!({"moments": self.searcher.moments(file, &vector)?}))
            }
            "text" => {
                let segment = request["segment"].as_i64().ok_or_else(|| anyhow!("text needs a segment"))?;
                Ok(json!({"text": self.searcher.text_of(segment)?}))
            }
            "duplicates" => {
                self.refresh()?;
                let threshold = request["threshold"].as_f64().unwrap_or(0.94) as f32;
                let groups = self.searcher.duplicates(threshold, 200)?;
                let mut out = Vec::new();
                for group in groups {
                    let mut files = Vec::new();
                    for id in group {
                        if let Some(row) = self.searcher.store.file(id)? {
                            files.push(json!({
                                "file": row.id, "path": row.path, "kind": row.kind, "size": row.size,
                                "modified": row.modified, "info": row.info,
                            }));
                        }
                    }
                    if files.len() > 1 {
                        out.push(files);
                    }
                }
                Ok(json!({"groups": out}))
            }
            "map" => {
                self.refresh()?;
                let (points, links) = self.searcher.chart(request["limit"].as_u64().unwrap_or(4000) as usize)?;
                let mut out = Vec::with_capacity(points.len());
                for (id, x, y) in points {
                    if let Some(row) = self.searcher.store.file(id)? {
                        out.push(json!({"file": id, "x": x, "y": y, "kind": row.kind, "path": row.path}));
                    }
                }
                let links: Vec<Value> = links.iter().map(|(a, b, strength)| json!([a, b, strength])).collect();
                Ok(json!({"points": out, "links": links}))
            }
            "collections" => {
                let rows: Vec<(i64, String, String, f64, i64)> = {
                    let mut select = self
                        .searcher
                        .store
                        .db
                        .prepare("SELECT id, name, query, created, mark FROM collections ORDER BY created")?;
                    let rows = select
                        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)))?;
                    rows.collect::<glint_core::rusqlite::Result<_>>()?
                };
                // With `fresh`, each comes with how many files it finds that it hasn't shown yet.
                let count = request["fresh"].as_bool().unwrap_or(false);
                if count {
                    self.refresh()?;
                }
                let mut out = Vec::new();
                for (id, name, words, created, mark) in rows {
                    let fresh = if count { self.fresh(&words, mark).unwrap_or(0) } else { 0 };
                    out.push(json!({"id": id, "name": name, "query": words, "created": created, "fresh": fresh}));
                }
                Ok(json!({"collections": out}))
            }
            "collection_seen" => {
                let id = request["collection"].as_i64().ok_or_else(|| anyhow!("which collection?"))?;
                self.searcher.store.db.execute("UPDATE collections SET mark = ? WHERE id = ?", (self.newest_file(), id))?;
                Ok(json!({}))
            }
            // Suggestions for a tidier photo library. Nothing is moved or deleted here.
            "tidy" => {
                self.refresh()?;
                let mut heaps = Vec::new();
                for (name, prompt) in TIDY {
                    let vector = self.embed_query(prompt)?;
                    let query = Query { kinds: vec![FileKind::Image], ..Query::default() };
                    let settings = Settings { limit: 60, ..Settings::default() };
                    let hits = self.searcher.search(&query, Some(&vector), None, None, &settings)?;
                    let ids: Vec<i64> = hits.iter().filter(|hit| hit.strong).map(|hit| hit.file).collect();
                    if !ids.is_empty() {
                        heaps.push(json!({"name": name, "files": self.members(&ids)?}));
                    }
                }
                let old: Vec<i64> = {
                    let mut select = self.searcher.store.db.prepare(
                        "SELECT id FROM files WHERE kind = 'screenshot' AND mtime < ? ORDER BY mtime LIMIT 60",
                    )?;
                    let rows = select.query_map([now() - 365.0 * 86400.0], |row| row.get(0))?;
                    rows.collect::<glint_core::rusqlite::Result<_>>()?
                };
                if !old.is_empty() {
                    heaps.push(json!({"name": "screens", "files": self.members(&old)?}));
                }
                // Shots taken one after another: the largest file of each is the one to keep.
                let mut bursts = Vec::new();
                for group in self.searcher.bursts(20.0, 0.86, 60)? {
                    let files = self.members(&group)?;
                    if files.len() > 1 {
                        let keep = files
                            .iter()
                            .max_by_key(|file| file["size"].as_u64().unwrap_or(0))
                            .map(|file| file["file"].clone());
                        bursts.push(json!({"keep": keep, "files": files}));
                    }
                }
                Ok(json!({"heaps": heaps, "bursts": bursts}))
            }
            // Photos gathered by what they show, each album named after what sets it apart from the rest.
            "albums" => {
                self.refresh()?;
                let near = request["near"].as_f64().unwrap_or(0.34) as f32;
                let (albums, whole) = self.searcher.albums(near, 4, 60)?;
                let mut subjects = Vec::new();
                if !albums.is_empty() {
                    for (prompt, _, _) in SUBJECTS {
                        subjects.push(self.embed_query(&format!("a photo of {prompt}"))?);
                    }
                }
                let dot = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>();
                let mut out = Vec::new();
                for album in &albums {
                    let mut ranked: Vec<(usize, f32)> = subjects
                        .iter()
                        .enumerate()
                        .map(|(index, subject)| (index, dot(&album.centre, subject) - dot(&whole, subject)))
                        .collect();
                    ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
                    let (index, lead) = ranked[0];
                    let name = (lead > 0.0).then(|| json!({"en": SUBJECTS[index].1, "vi": SUBJECTS[index].2}));
                    out.push(json!({"name": name, "lead": lead, "files": self.members(&album.files)?}));
                }
                Ok(json!({"albums": out}))
            }
            // From one file through the others about the same thing.
            "thread" => {
                self.refresh()?;
                let file = request["file"].as_i64().ok_or_else(|| anyhow!("thread needs a file"))?;
                let mut out = Vec::new();
                for (id, nearness) in self.searcher.thread(file, 9)? {
                    if let Some(mut member) = self.member(id)? {
                        member["nearness"] = json!(nearness);
                        out.push(member);
                    }
                }
                Ok(json!({"thread": out}))
            }
            "collection_add" => {
                let name = request["name"].as_str().unwrap_or_default().trim();
                let query = request["q"].as_str().unwrap_or_default().trim();
                if name.is_empty() || query.is_empty() {
                    bail!("a collection needs a name and a search");
                }
                self.searcher.store.db.execute(
                    "INSERT INTO collections(name, query, created, mark) VALUES(?, ?, ?, ?)",
                    (name, query, now(), self.newest_file()),
                )?;
                Ok(json!({"id": self.searcher.store.db.last_insert_rowid()}))
            }
            "collection_remove" => {
                let id = request["collection"].as_i64().ok_or_else(|| anyhow!("which collection?"))?;
                self.searcher.store.db.execute("DELETE FROM collections WHERE id = ?", [id])?;
                Ok(json!({}))
            }
            "counts" => Ok(json!({
                "counts": self.searcher.store.counts()?,
                "failures": self.searcher.store.failures(50)?,
                "index_bytes": std::fs::metadata(&self.searcher.store.file).map(|m| m.len()).unwrap_or(0),
            })),
            // A PDF page as a picture, for previews: written to the cache, its path returned.
            "page" => {
                let path = request["path"].as_str().ok_or_else(|| anyhow!("page needs a path"))?;
                let page = request["page"].as_u64().unwrap_or(1).max(1) as usize;
                let side = request["side"].as_u64().unwrap_or(1400).clamp(200, 3000) as u32;
                let picture = render_page(Path::new(path), page - 1, side)?;
                let folder = config::cache_folder().join("pages");
                std::fs::create_dir_all(&folder)?;
                let name = format!("{}-{page}-{side}.jpg", &digest_hex(path)[..20]);
                let target = folder.join(name);
                std::fs::write(&target, images::jpeg(&picture, 85)?)?;
                Ok(json!({"path": target, "width": picture.width, "height": picture.height}))
            }
            "frame" => {
                let path = request["path"].as_str().ok_or_else(|| anyhow!("frame needs a path"))?;
                let second = request["second"].as_f64().unwrap_or(0.0).max(0.0);
                let side = request["side"].as_u64().unwrap_or(640).clamp(120, 1920) as u32;
                let folder = config::cache_folder().join("frames");
                std::fs::create_dir_all(&folder)?;
                let name = format!("{}-{}-{side}.jpg", &digest_hex(path)[..20], (second * 10.0).round() as u64);
                let target = folder.join(name);
                if !target.exists() {
                    let picture = media::frame_at(Path::new(path), second, side)?;
                    std::fs::write(&target, images::jpeg(&picture, 82)?)?;
                }
                Ok(json!({"path": target}))
            }
            "config" => {
                self.config = Config::load();
                Ok(json!({}))
            }
            "reload" => {
                self.searcher.reload()?;
                Ok(json!({"vectors": self.searcher.vector_count()}))
            }
            "ping" => Ok(json!({"vectors": self.searcher.vector_count(), "model": self.text.is_some()})),
            other => bail!("unknown op: {other}"),
        }
    }
}

fn digest_hex(text: &str) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(text.as_bytes()).iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(windows)]
fn render_page(path: &Path, index: usize, side: u32) -> Result<Picture> {
    glint_core::readers::winrt::PdfFile::open(path)?.render(index, side)
}

#[cfg(not(windows))]
fn render_page(_path: &Path, _index: usize, _side: u32) -> Result<Picture> {
    bail!("PDF pages are rendered on Windows only for now")
}

pub fn run(database: &Path, config: Config) -> Result<()> {
    let mut session = Session::open(database, config)?;
    let (sender, receiver) = mpsc::channel::<String>();
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { break };
            if sender.send(line).is_err() {
                break;
            }
        }
    });
    emit(&json!({"event": "ready"}));
    let mut last = Instant::now();
    loop {
        match receiver.recv_timeout(Duration::from_secs(20)) {
            Ok(line) => {
                last = Instant::now();
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let request: Value = match serde_json::from_str(line) {
                    Ok(request) => request,
                    Err(error) => {
                        emit(&json!({"ok": false, "error": format!("not JSON: {error}")}));
                        continue;
                    }
                };
                let id = request["id"].clone();
                // A newer search for the same panel makes this one pointless: skip to the last one waiting.
                match session.handle(&request) {
                    Ok(result) => emit(&json!({"id": id, "ok": true, "result": result})),
                    Err(error) => emit(&json!({"id": id, "ok": false, "error": format!("{error:#}")})),
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                session.release_idle();
                if last.elapsed() > IDLE {
                    break;
                }
            }
            // The app closed the pipe.
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }
    Ok(())
}
