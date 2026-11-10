//! The index: one SQLite file with every file, its segments (each with a half-precision vector or none) and a keyword
//! index over names and text.

use crate::kinds::{FileKind, Modality, SegmentKind};
use crate::text;
use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::{Path, PathBuf};

pub const DIMENSION: usize = 768;

/// A vector as stored: 768 × float16, little endian (1.5 KB).
pub fn to_half(vector: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(vector.len() * 2);
    for &value in vector {
        out.extend_from_slice(&half::f16::from_f32(value).to_le_bytes());
    }
    out
}

pub fn from_half(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(2).map(|pair| half::f16::from_le_bytes([pair[0], pair[1]]).to_f32()).collect()
}

/// A file as the crawler saw it.
#[derive(Clone, Debug, PartialEq)]
pub struct Seen {
    pub path: PathBuf,
    pub root: PathBuf,
    pub kind: FileKind,
    pub size: u64,
    /// Seconds since 1970.
    pub modified: f64,
}

/// One piece of a file, ready to store.
#[derive(Clone, Debug)]
pub struct Segment {
    pub kind: SegmentKind,
    pub modality: Modality,
    /// A PDF page (from 1), or seconds into a recording.
    pub loc: Option<f64>,
    pub loc_end: Option<f64>,
    /// What a result shows under the file's name.
    pub excerpt: Option<String>,
    /// The text keyword search matches, as written.
    pub text: Option<String>,
    pub vector: Option<Vec<f32>>,
    /// Added by the later pass over a long file: scored, but it doesn't set a query's scale.
    pub late: bool,
}

impl Segment {
    pub fn new(kind: SegmentKind, modality: Modality) -> Segment {
        Segment { kind, modality, loc: None, loc_end: None, excerpt: None, text: None, vector: None, late: false }
    }
}

#[derive(Clone, Debug)]
pub struct FileRow {
    pub id: i64,
    pub path: String,
    pub kind: FileKind,
    pub size: u64,
    pub modified: f64,
    pub state: String,
    pub progress: i64,
    pub info: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Counts {
    pub files: i64,
    pub pending: i64,
    pub partial: i64,
    pub failed: i64,
    pub segments: i64,
    pub vectors: i64,
    pub by_kind: Vec<(String, i64)>,
}

pub struct Store {
    pub db: Connection,
    pub file: PathBuf,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta(key TEXT PRIMARY KEY, value TEXT);
CREATE TABLE IF NOT EXISTS files(
    id INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,
    root TEXT NOT NULL,
    kind TEXT NOT NULL,
    size INTEGER NOT NULL,
    mtime REAL NOT NULL,
    info TEXT NOT NULL DEFAULT '{}',
    state TEXT NOT NULL DEFAULT 'pending',
    progress INTEGER NOT NULL DEFAULT 0,
    error TEXT,
    hash TEXT,
    indexed_at REAL
);
CREATE INDEX IF NOT EXISTS files_state ON files(state);
CREATE INDEX IF NOT EXISTS files_hash ON files(hash);
CREATE TABLE IF NOT EXISTS segments(
    id INTEGER PRIMARY KEY,
    file INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    kind TEXT NOT NULL,
    modality TEXT NOT NULL,
    loc REAL,
    loc_end REAL,
    excerpt TEXT,
    late INTEGER NOT NULL DEFAULT 0,
    vec BLOB
);
CREATE INDEX IF NOT EXISTS segments_file ON segments(file);
CREATE VIRTUAL TABLE IF NOT EXISTS fts USING fts5(
    name, body, original UNINDEXED, tokenize = 'porter unicode61 remove_diacritics 2'
);
CREATE TABLE IF NOT EXISTS collections(
    id INTEGER PRIMARY KEY, name TEXT NOT NULL, query TEXT NOT NULL, created REAL NOT NULL
);
";

impl Store {
    pub fn open(file: &Path) -> Result<Store> {
        if let Some(folder) = file.parent() {
            std::fs::create_dir_all(folder).ok();
        }
        let db = Connection::open(file).with_context(|| format!("can't open {}", file.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        db.pragma_update(None, "journal_mode", "WAL")?;
        db.pragma_update(None, "synchronous", "NORMAL")?;
        db.pragma_update(None, "foreign_keys", "ON")?;
        db.execute_batch(SCHEMA)?;
        Ok(Store { db, file: file.to_path_buf() })
    }

    pub fn open_read_only(file: &Path) -> Result<Store> {
        let db = Connection::open_with_flags(
            file,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .with_context(|| format!("can't open {}", file.display()))?;
        db.busy_timeout(std::time::Duration::from_secs(10))?;
        Ok(Store { db, file: file.to_path_buf() })
    }

    pub fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self.db.query_row("SELECT value FROM meta WHERE key = ?", [key], |row| row.get(0)).optional()?)
    }

    pub fn set_meta(&self, key: &str, value: &str) -> Result<()> {
        self.db.execute(
            "INSERT INTO meta(key, value) VALUES(?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    /// Makes sure every vector in the index came from `source` (the model, its revision, the vector version). An
    /// index made by another one is emptied, so its files are read again. Returns whether it was.
    pub fn claim_vector_source(&self, source: &str) -> Result<bool> {
        match self.meta("vector_source")? {
            Some(current) if current == source => Ok(false),
            Some(_) => {
                self.db.execute_batch(
                    "DELETE FROM fts; DELETE FROM segments;
                     UPDATE files SET state = 'pending', progress = 0, error = NULL, indexed_at = NULL;",
                )?;
                self.set_meta("vector_source", source)?;
                Ok(true)
            }
            None => {
                self.set_meta("vector_source", source)?;
                Ok(false)
            }
        }
    }

    /// Records what the crawler saw: new files and changed ones become pending, and files of `roots` that are gone
    /// leave the index. Returns (added, changed, removed).
    pub fn reconcile(&mut self, roots: &[PathBuf], seen: &[Seen]) -> Result<(usize, usize, usize)> {
        let transaction = self.db.transaction()?;
        let (mut added, mut changed, mut removed) = (0, 0, 0);
        {
            let mut known = std::collections::HashMap::new();
            let mut select = transaction.prepare("SELECT id, path, size, mtime, root FROM files")?;
            let rows = select.query_map([], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?, row.get::<_, f64>(3)?,
                    row.get::<_, String>(4)?))
            })?;
            for row in rows {
                let (id, path, size, mtime, root) = row?;
                known.insert(path, (id, size, mtime, root));
            }
            let mut insert = transaction
                .prepare("INSERT INTO files(path, root, kind, size, mtime) VALUES(?, ?, ?, ?, ?)")?;
            let mut update = transaction.prepare(
                "UPDATE files SET size = ?, mtime = ?, kind = ?, root = ?, state = 'pending', progress = 0,
                 error = NULL, hash = NULL WHERE id = ?",
            )?;
            for file in seen {
                let path = file.path.to_string_lossy().into_owned();
                let root = file.root.to_string_lossy().into_owned();
                match known.remove(&path) {
                    None => {
                        insert.execute(params![path, root, file.kind.as_str(), file.size as i64, file.modified])?;
                        added += 1;
                    }
                    Some((id, size, mtime, _)) => {
                        if size != file.size as i64 || (mtime - file.modified).abs() > 1.0 {
                            delete_segments(&transaction, id)?;
                            update.execute(params![file.size as i64, file.modified, file.kind.as_str(), root, id])?;
                            changed += 1;
                        }
                    }
                }
            }
            // What's left wasn't seen: gone, or its folder isn't searched any more. A root that wasn't crawled this
            // time (an unplugged drive) keeps its files.
            let crawled: Vec<String> = roots.iter().map(|r| r.to_string_lossy().into_owned()).collect();
            for (_, (id, _, _, root)) in known {
                let folder_still_chosen = crawled.iter().any(|r| *r == root);
                let root_exists = Path::new(&root).exists();
                if folder_still_chosen && !root_exists {
                    continue;
                }
                delete_segments(&transaction, id)?;
                transaction.execute("DELETE FROM files WHERE id = ?", [id])?;
                removed += 1;
            }
        }
        transaction.commit()?;
        Ok((added, changed, removed))
    }

    /// One file seen again (the watcher): pending when new or changed. Returns whether it needs indexing.
    pub fn touch(&mut self, file: &Seen) -> Result<bool> {
        let path = file.path.to_string_lossy().into_owned();
        let known: Option<(i64, i64, f64)> = self
            .db
            .query_row("SELECT id, size, mtime FROM files WHERE path = ?", [&path], |row| {
                Ok((row.get(0)?, row.get(1)?, row.get(2)?))
            })
            .optional()?;
        match known {
            None => {
                self.db.execute(
                    "INSERT INTO files(path, root, kind, size, mtime) VALUES(?, ?, ?, ?, ?)",
                    params![path, file.root.to_string_lossy(), file.kind.as_str(), file.size as i64, file.modified],
                )?;
                Ok(true)
            }
            Some((id, size, mtime)) if size != file.size as i64 || (mtime - file.modified).abs() > 1.0 => {
                let transaction = self.db.transaction()?;
                delete_segments(&transaction, id)?;
                transaction.execute(
                    "UPDATE files SET size = ?, mtime = ?, state = 'pending', progress = 0, error = NULL, hash = NULL
                     WHERE id = ?",
                    params![file.size as i64, file.modified, id],
                )?;
                transaction.commit()?;
                Ok(true)
            }
            Some(_) => Ok(false),
        }
    }

    /// A file that's gone (the watcher), or a whole folder (`path` as a prefix).
    pub fn forget(&mut self, path: &Path) -> Result<usize> {
        let exact = path.to_string_lossy().into_owned();
        let prefix = format!("{}%", like_escape(&format!("{exact}{}", std::path::MAIN_SEPARATOR)));
        let transaction = self.db.transaction()?;
        let ids: Vec<i64> = {
            let mut select =
                transaction.prepare("SELECT id FROM files WHERE path = ? OR path LIKE ? ESCAPE '\\'")?;
            let rows = select.query_map(params![exact, prefix], |row| row.get(0))?;
            rows.collect::<rusqlite::Result<_>>()?
        };
        for id in &ids {
            delete_segments(&transaction, *id)?;
            transaction.execute("DELETE FROM files WHERE id = ?", [id])?;
        }
        transaction.commit()?;
        Ok(ids.len())
    }

    /// One file leaves the index; nothing else, whatever else shares the start of its path.
    pub fn forget_file(&mut self, path: &Path) -> Result<bool> {
        let exact = path.to_string_lossy().into_owned();
        let transaction = self.db.transaction()?;
        let id: Option<i64> =
            transaction.query_row("SELECT id FROM files WHERE path = ?", [&exact], |row| row.get(0)).optional()?;
        if let Some(id) = id {
            delete_segments(&transaction, id)?;
            transaction.execute("DELETE FROM files WHERE id = ?", [id])?;
        }
        transaction.commit()?;
        Ok(id.is_some())
    }

    /// Files waiting to be read (`pending`), or long files waiting for their later pages (`partial`), smallest first
    /// within a kind so that the index fills quickly.
    pub fn waiting(&self, state: &str, limit: usize) -> Result<Vec<FileRow>> {
        let mut select = self.db.prepare(
            "SELECT id, path, kind, size, mtime, state, progress, info FROM files WHERE state = ?
             ORDER BY CASE kind WHEN 'screenshot' THEN 0 WHEN 'image' THEN 1 WHEN 'doc' THEN 2 WHEN 'pdf' THEN 3
                                WHEN 'audio' THEN 4 ELSE 5 END, size LIMIT ?",
        )?;
        let rows = select.query_map(params![state, limit as i64], file_row)?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn file(&self, id: i64) -> Result<Option<FileRow>> {
        Ok(self
            .db
            .query_row(
                "SELECT id, path, kind, size, mtime, state, progress, info FROM files WHERE id = ?",
                [id],
                file_row,
            )
            .optional()?)
    }

    pub fn file_by_path(&self, path: &str) -> Result<Option<FileRow>> {
        Ok(self
            .db
            .query_row(
                "SELECT id, path, kind, size, mtime, state, progress, info FROM files WHERE path = ?",
                [path],
                file_row,
            )
            .optional()?)
    }

    /// Stores a file's segments. `state`: `done`, or `partial` with `progress` (the last page or passage read) when
    /// a later pass has more to do. `replace` drops what the file had (a first pass); a later pass adds to it.
    #[allow(clippy::too_many_arguments)]
    pub fn save(
        &mut self, file: i64, name: &str, segments: &[Segment], info: &serde_json::Map<String, serde_json::Value>,
        state: &str, progress: i64, hash: Option<&str>, replace: bool,
    ) -> Result<()> {
        let transaction = self.db.transaction()?;
        if replace {
            delete_segments(&transaction, file)?;
        }
        {
            let mut insert = transaction.prepare(
                "INSERT INTO segments(file, kind, modality, loc, loc_end, excerpt, late, vec)
                 VALUES(?, ?, ?, ?, ?, ?, ?, ?)",
            )?;
            let mut index = transaction.prepare("INSERT INTO fts(rowid, name, body, original) VALUES(?, ?, ?, ?)")?;
            for segment in segments {
                insert.execute(params![
                    file,
                    segment.kind.as_str(),
                    segment.modality.as_str(),
                    segment.loc,
                    segment.loc_end,
                    segment.excerpt,
                    segment.late as i64,
                    segment.vector.as_deref().map(to_half),
                ])?;
                let id = transaction.last_insert_rowid();
                let folded_name = if segment.kind == SegmentKind::Name { text::fold(name) } else { String::new() };
                match &segment.text {
                    Some(original) if !original.trim().is_empty() => {
                        index.execute(params![id, folded_name, text::fold(original), original])?;
                    }
                    _ if segment.kind == SegmentKind::Name => {
                        index.execute(params![id, folded_name, "", name])?;
                    }
                    _ => {}
                }
            }
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        transaction.execute(
            "UPDATE files SET state = ?, progress = ?, info = ?, error = NULL, indexed_at = ?,
             hash = coalesce(?, hash) WHERE id = ?",
            params![state, progress, serde_json::Value::Object(info.clone()).to_string(), now, hash, file],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn fail(&self, file: i64, error: &str) -> Result<()> {
        let error: String = error.chars().take(300).collect();
        self.db.execute("UPDATE files SET state = 'failed', error = ? WHERE id = ?", params![error, file])?;
        Ok(())
    }

    /// Failed files get another go (after an update that may read them).
    pub fn retry_failed(&self) -> Result<usize> {
        Ok(self.db.execute("UPDATE files SET state = 'pending', error = NULL WHERE state = 'failed'", [])?)
    }

    pub fn counts(&self) -> Result<Counts> {
        let mut counts = Counts::default();
        let mut select = self.db.prepare("SELECT kind, state, COUNT(*) FROM files GROUP BY kind, state")?;
        let rows = select.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, i64>(2)?))
        })?;
        let mut by_kind = std::collections::BTreeMap::new();
        for row in rows {
            let (kind, state, count) = row?;
            counts.files += count;
            match state.as_str() {
                "pending" => counts.pending += count,
                "partial" => counts.partial += count,
                "failed" => counts.failed += count,
                _ => {}
            }
            *by_kind.entry(kind).or_insert(0) += count;
        }
        counts.by_kind = by_kind.into_iter().collect();
        counts.segments = self.db.query_row("SELECT COUNT(*) FROM segments", [], |row| row.get(0))?;
        counts.vectors =
            self.db.query_row("SELECT COUNT(*) FROM segments WHERE vec IS NOT NULL", [], |row| row.get(0))?;
        Ok(counts)
    }

    pub fn failures(&self, limit: usize) -> Result<Vec<(String, String)>> {
        let mut select =
            self.db.prepare("SELECT path, coalesce(error, '') FROM files WHERE state = 'failed' LIMIT ?")?;
        let rows = select.query_map([limit as i64], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }
}

fn file_row(row: &rusqlite::Row) -> rusqlite::Result<FileRow> {
    let info: String = row.get(7)?;
    Ok(FileRow {
        id: row.get(0)?,
        path: row.get(1)?,
        kind: FileKind::parse(&row.get::<_, String>(2)?).unwrap_or(FileKind::Doc),
        size: row.get::<_, i64>(3)? as u64,
        modified: row.get(4)?,
        state: row.get(5)?,
        progress: row.get(6)?,
        info: serde_json::from_str(&info).unwrap_or_default(),
    })
}

fn delete_segments(db: &Connection, file: i64) -> rusqlite::Result<()> {
    db.execute("DELETE FROM fts WHERE rowid IN (SELECT id FROM segments WHERE file = ?)", [file])?;
    db.execute("DELETE FROM segments WHERE file = ?", [file])?;
    Ok(())
}

fn like_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('%', "\\%").replace('_', "\\_")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(path: &str, size: u64) -> Seen {
        Seen { path: path.into(), root: "C:\\root".into(), kind: FileKind::Doc, size, modified: 100.0 }
    }

    #[test]
    fn half_precision_round_trips_closely() {
        let vector: Vec<f32> = (0..DIMENSION).map(|i| ((i as f32) * 0.37).sin() * 0.05).collect();
        let back = from_half(&to_half(&vector));
        assert_eq!(back.len(), DIMENSION);
        assert!(vector.iter().zip(&back).all(|(a, b)| (a - b).abs() < 1e-4));
    }

    #[test]
    fn reconcile_adds_changes_and_removes() {
        let folder = tempfile::tempdir().unwrap();
        let mut store = Store::open(&folder.path().join("index.db")).unwrap();
        let roots = vec![PathBuf::from("C:\\root")];
        assert_eq!(store.reconcile(&roots, &[seen("C:\\root\\a.txt", 10), seen("C:\\root\\b.txt", 20)]).unwrap(), (2, 0, 0));
        let a = store.file_by_path("C:\\root\\a.txt").unwrap().unwrap();
        let mut segment = Segment::new(SegmentKind::Chunk, Modality::Text);
        segment.text = Some("the lease allows pets".into());
        segment.vector = Some(vec![0.1; DIMENSION]);
        store.save(a.id, "a.txt", &[Segment::new(SegmentKind::Name, Modality::None), segment], &Default::default(), "done", 0, None, true).unwrap();
        assert_eq!(store.counts().unwrap().vectors, 1);
        // b is gone and a changed: a's segments go, and it's pending again.
        let roots: Vec<PathBuf> = vec![];
        assert_eq!(store.reconcile(&roots, &[seen("C:\\root\\a.txt", 11)]).unwrap(), (0, 1, 1));
        let counts = store.counts().unwrap();
        assert_eq!((counts.files, counts.pending, counts.segments), (1, 1, 0));
        let matches: i64 = store.db.query_row("SELECT COUNT(*) FROM fts WHERE fts MATCH 'lease'", [], |r| r.get(0)).unwrap();
        assert_eq!(matches, 0);
    }

    #[test]
    fn another_model_empties_the_index() {
        let folder = tempfile::tempdir().unwrap();
        let mut store = Store::open(&folder.path().join("index.db")).unwrap();
        assert!(!store.claim_vector_source("model-a").unwrap());
        store.reconcile(&[], &[seen("C:\\root\\a.txt", 10)]).unwrap();
        let a = store.file_by_path("C:\\root\\a.txt").unwrap().unwrap();
        store.save(a.id, "a.txt", &[Segment::new(SegmentKind::Name, Modality::None)], &Default::default(), "done", 0, None, true).unwrap();
        assert!(!store.claim_vector_source("model-a").unwrap());
        assert!(store.claim_vector_source("model-b").unwrap());
        let counts = store.counts().unwrap();
        assert_eq!((counts.pending, counts.segments), (1, 0));
    }

    #[test]
    fn forgetting_a_folder_takes_its_files() {
        let folder = tempfile::tempdir().unwrap();
        let mut store = Store::open(&folder.path().join("index.db")).unwrap();
        let sep = std::path::MAIN_SEPARATOR;
        let inside = format!("C:{sep}root{sep}sub{sep}a.txt");
        let beside = format!("C:{sep}root{sep}sub_other.txt");
        store.reconcile(&[], &[seen(&inside, 1), seen(&beside, 1)]).unwrap();
        assert_eq!(store.forget(Path::new(&format!("C:{sep}root{sep}sub"))).unwrap(), 1);
        assert_eq!(store.counts().unwrap().files, 1);
        // One file by its exact path: a folder's path takes nothing with it.
        store.reconcile(&[], &[seen(&inside, 1), seen(&beside, 1)]).unwrap();
        assert!(!store.forget_file(Path::new(&format!("C:{sep}root{sep}sub"))).unwrap());
        assert_eq!(store.counts().unwrap().files, 2);
        assert!(store.forget_file(Path::new(&inside)).unwrap());
        assert_eq!(store.counts().unwrap().files, 1);
    }
}
