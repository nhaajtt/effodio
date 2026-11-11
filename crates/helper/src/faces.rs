//! Who is in the pictures. Faces are found and described on this computer, kept in the index beside the files, and
//! only once it's turned on. A face gets a name from one picture of it; after that every face close enough to a named
//! one takes the name too, in the pictures already read and in those read later.

use crate::emit;
use anyhow::{anyhow, bail, Context, Result};
use glint_core::config;
use glint_core::readers::{images, Picture};
use glint_core::rusqlite::Connection;
use glint_core::store::Store;
use glint_faces::{cosine, files, Faces, DIMENSION, SAME};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

/// A face takes a named person's name from this closeness up: a little stricter than "the same person" by the
/// model's own measure, since a wrong name is worse than a missing one.
const TAKES_NAME: f32 = 0.42;
/// Unnamed faces gather into one "who is this?" from this closeness to the group's middle.
const GATHERS: f32 = 0.45;
/// A search by a sample face shows faces from this closeness up, the surest marked as matches.
const SHOWS: f32 = 0.30;

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS people(id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE, created REAL NOT NULL);
CREATE TABLE IF NOT EXISTS faces(
    id INTEGER PRIMARY KEY,
    file INTEGER NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    x REAL NOT NULL, y REAL NOT NULL, w REAL NOT NULL, h REAL NOT NULL,
    score REAL NOT NULL,
    vec BLOB NOT NULL,
    person INTEGER REFERENCES people(id) ON DELETE SET NULL,
    -- 1: someone gave this face its name by hand (or took it away): it isn't renamed by likeness.
    fixed INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS faces_file ON faces(file);
CREATE TABLE IF NOT EXISTS face_scans(file INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE);
";

pub fn prepare(db: &Connection) -> Result<()> {
    db.execute_batch(SCHEMA)?;
    Ok(())
}

fn blob(vector: &[f32]) -> Vec<u8> {
    vector.iter().flat_map(|value| value.to_le_bytes()).collect()
}

fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|four| f32::from_le_bytes([four[0], four[1], four[2], four[3]])).collect()
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

/// Downloads the two face models (39 MB), each checked against its SHA-256.
pub fn download(folder: &Path) -> Result<()> {
    std::fs::create_dir_all(folder)?;
    for file in files::ALL {
        let target = folder.join(file.name);
        if target.exists() {
            continue;
        }
        let response =
            ureq::get(&file.url()).set("User-Agent", "glint").call().with_context(|| format!("can't download {}", file.name))?;
        let mut bytes = Vec::with_capacity(file.bytes as usize);
        response.into_reader().take(file.bytes + 1).read_to_end(&mut bytes)?;
        let digest: String = Sha256::digest(&bytes).iter().map(|byte| format!("{byte:02x}")).collect();
        if bytes.len() as u64 != file.bytes || digest != file.sha256 {
            bail!("{} didn't arrive whole: try again", file.name);
        }
        let part = folder.join(format!("{}.part", file.name));
        std::fs::write(&part, &bytes)?;
        std::fs::rename(&part, &target)?;
    }
    Ok(())
}

/// Every face in the index: its id, its file, whose it is, whether that was said by hand, and its vector.
struct Known {
    id: i64,
    file: i64,
    person: Option<i64>,
    fixed: bool,
    vector: Vec<f32>,
}

fn known(db: &Connection) -> Result<Vec<Known>> {
    let mut select = db.prepare("SELECT id, file, person, fixed, vec FROM faces")?;
    let rows = select.query_map([], |row| {
        Ok(Known {
            id: row.get(0)?,
            file: row.get(1)?,
            person: row.get(2)?,
            fixed: row.get::<_, i64>(3)? != 0,
            vector: floats(row.get_ref(4)?.as_blob()?),
        })
    })?;
    Ok(rows.filter_map(|row| row.ok()).filter(|face| face.vector.len() == DIMENSION).collect())
}

/// Gives every face without a name the name of the named face it is closest to, if that is close enough.
/// Returns how many took one.
pub fn spread_names(db: &Connection) -> Result<usize> {
    let all = known(db)?;
    let named: Vec<&Known> = all.iter().filter(|face| face.person.is_some()).collect();
    let mut took = 0;
    for face in all.iter().filter(|face| face.person.is_none() && !face.fixed) {
        let best = named
            .iter()
            .map(|other| (other.person, cosine(&face.vector, &other.vector)))
            .max_by(|a, b| a.1.total_cmp(&b.1));
        if let Some((person, closeness)) = best {
            if closeness >= TAKES_NAME {
                db.execute("UPDATE faces SET person = ? WHERE id = ?", (person, face.id))?;
                took += 1;
            }
        }
    }
    Ok(took)
}

/// Reads the faces of the photos that haven't been looked at yet. (Photos only: screenshots are left alone.)
pub fn scan(store: &mut Store, stop: &AtomicBool) -> Result<usize> {
    prepare(&store.db)?;
    let waiting: Vec<(i64, String)> = {
        let mut select = store.db.prepare(
            "SELECT id, path FROM files WHERE kind = 'image' AND id NOT IN (SELECT file FROM face_scans) ORDER BY id",
        )?;
        let rows = select.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        rows.collect::<glint_core::rusqlite::Result<_>>()?
    };
    if waiting.is_empty() {
        return Ok(0);
    }
    let faces = Faces::open(&config::models_folder())?;
    emit(&json!({"event": "deepening", "total": waiting.len()}));
    let mut done = 0;
    for batch in waiting.chunks(32) {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        let found: Vec<(i64, Vec<glint_faces::Face>)> = std::thread::scope(|scope| {
            let workers: Vec<_> = batch
                .chunks(batch.len().div_ceil(8).max(1))
                .map(|part| {
                    let faces = &faces;
                    scope.spawn(move || {
                        part.iter()
                            .map(|(id, path)| {
                                let seen = images::load(Path::new(path))
                                    .and_then(|picture| faces.find(&picture.rgb, picture.width, picture.height))
                                    .unwrap_or_default();
                                (*id, seen)
                            })
                            .collect::<Vec<_>>()
                    })
                })
                .collect();
            workers.into_iter().flat_map(|worker| worker.join().unwrap_or_default()).collect()
        });
        let change = store.db.transaction()?;
        for (file, seen) in &found {
            for face in seen {
                change.execute(
                    "INSERT INTO faces(file, x, y, w, h, score, vec) VALUES(?, ?, ?, ?, ?, ?, ?)",
                    (file, face.x, face.y, face.width, face.height, face.score, blob(&face.vector)),
                )?;
            }
            change.execute("INSERT OR IGNORE INTO face_scans(file) VALUES(?)", [file])?;
        }
        change.commit()?;
        done += found.len();
        emit(&json!({"event": "file", "done": done, "total": waiting.len(), "path": batch.last().map(|(_, path)| path)}));
    }
    spread_names(&store.db)?;
    Ok(done)
}

/// A face cut out of its picture with some room around it, as a small JPEG in the cache. Returns its path.
fn crop(db: &Connection, face: i64) -> Result<std::path::PathBuf> {
    let folder = config::cache_folder().join("faces");
    let target = folder.join(format!("{face}.jpg"));
    if target.exists() {
        return Ok(target);
    }
    let (path, x, y, w, h): (String, f64, f64, f64, f64) = db.query_row(
        "SELECT files.path, faces.x, faces.y, faces.w, faces.h FROM faces JOIN files ON files.id = faces.file WHERE faces.id = ?",
        [face],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?)),
    )?;
    let picture = images::load(Path::new(&path))?;
    let (width, height) = (picture.width as f64, picture.height as f64);
    let side = (w * width).max(h * height) * 1.5;
    let (centre_x, centre_y) = ((x + w / 2.0) * width, (y + h / 2.0) * height);
    let left = (centre_x - side / 2.0).clamp(0.0, (width - 1.0).max(0.0)) as usize;
    let top = (centre_y - side / 2.0).clamp(0.0, (height - 1.0).max(0.0)) as usize;
    let across = (side as usize).clamp(1, picture.width - left);
    let down = (side as usize).clamp(1, picture.height - top);
    let mut rgb = Vec::with_capacity(across * down * 3);
    for row in top..top + down {
        rgb.extend_from_slice(&picture.rgb[(row * picture.width + left) * 3..(row * picture.width + left + across) * 3]);
    }
    let cut = Picture { width: across, height: down, rgb }.fit(192);
    std::fs::create_dir_all(&folder)?;
    std::fs::write(&target, images::jpeg(&cut, 86)?)?;
    Ok(target)
}

fn face_json(db: &Connection, id: i64, name: Option<String>) -> Value {
    json!({"face": id, "name": name, "crop": crop(db, id).ok()})
}

fn file_json(store: &Store, id: i64) -> Result<Option<Value>> {
    Ok(store.file(id)?.map(|row| {
        json!({
            "file": row.id, "path": row.path, "kind": row.kind, "size": row.size,
            "modified": row.modified, "info": row.info,
        })
    }))
}

/// The requests about faces. `None`: the request isn't one of them.
pub fn handle(store: &Store, request: &Value) -> Option<Result<Value>> {
    let op = request["op"].as_str().unwrap_or_default();
    if !matches!(
        op,
        "faces_status" | "faces_enable" | "faces_wipe" | "faces_of" | "face_name" | "people" | "person_rename"
            | "person_forget" | "face_example"
    ) {
        return None;
    }
    Some(answer(store, op, request))
}

fn answer(store: &Store, op: &str, request: &Value) -> Result<Value> {
    let db = &store.db;
    prepare(db)?;
    let count = |sql: &str| -> i64 { db.query_row(sql, [], |row| row.get(0)).unwrap_or(0) };
    match op {
        "faces_status" => Ok(json!({
            "models": Faces::ready(&config::models_folder()),
            "faces": count("SELECT COUNT(*) FROM faces"),
            "people": count("SELECT COUNT(*) FROM people"),
            "scanned": count("SELECT COUNT(*) FROM face_scans"),
            "photos": count("SELECT COUNT(*) FROM files WHERE kind = 'image'"),
        })),
        "faces_enable" => {
            download(&config::models_folder())?;
            Ok(json!({"models": true}))
        }
        // Forgets every face and every name.
        "faces_wipe" => {
            db.execute_batch("DELETE FROM faces; DELETE FROM people; DELETE FROM face_scans;")?;
            std::fs::remove_dir_all(config::cache_folder().join("faces")).ok();
            Ok(json!({}))
        }
        "faces_of" => {
            let file = request["file"].as_i64().ok_or_else(|| anyhow!("faces_of needs a file"))?;
            let mut select = db.prepare(
                "SELECT faces.id, people.name FROM faces LEFT JOIN people ON people.id = faces.person
                 WHERE faces.file = ? ORDER BY faces.w * faces.h DESC",
            )?;
            let rows: Vec<(i64, Option<String>)> =
                select.query_map([file], |row| Ok((row.get(0)?, row.get(1)?)))?.filter_map(|row| row.ok()).collect();
            Ok(json!({"faces": rows.into_iter().map(|(id, name)| face_json(db, id, name)).collect::<Vec<_>>()}))
        }
        // Names a face (an empty name takes its name away). The name then spreads to the faces like it.
        "face_name" => {
            let face = request["face"].as_i64().ok_or_else(|| anyhow!("which face?"))?;
            let name = request["name"].as_str().unwrap_or_default().trim();
            if name.is_empty() {
                db.execute("UPDATE faces SET person = NULL, fixed = 1 WHERE id = ?", [face])?;
                return Ok(json!({"spread": 0}));
            }
            db.execute("INSERT OR IGNORE INTO people(name, created) VALUES(?, ?)", (name, now()))?;
            let person: i64 = db.query_row("SELECT id FROM people WHERE name = ?", [name], |row| row.get(0))?;
            db.execute("UPDATE faces SET person = ?, fixed = 1 WHERE id = ?", (person, face))?;
            // With `group`, the faces gathered with this one take the name too.
            if let Some(group) = request["group"].as_array() {
                for other in group.iter().filter_map(Value::as_i64) {
                    db.execute("UPDATE faces SET person = ? WHERE id = ? AND fixed = 0", (person, other))?;
                }
            }
            Ok(json!({"person": person, "spread": spread_names(db)?}))
        }
        "person_rename" => {
            let person = request["person"].as_i64().ok_or_else(|| anyhow!("which person?"))?;
            let name = request["name"].as_str().unwrap_or_default().trim();
            if name.is_empty() {
                bail!("a person needs a name");
            }
            db.execute("UPDATE people SET name = ? WHERE id = ?", (name, person))?;
            Ok(json!({}))
        }
        "person_forget" => {
            let person = request["person"].as_i64().ok_or_else(|| anyhow!("which person?"))?;
            db.execute("UPDATE faces SET person = NULL, fixed = 0 WHERE person = ?", [person])?;
            db.execute("DELETE FROM people WHERE id = ?", [person])?;
            Ok(json!({}))
        }
        // The people with names (each with a few of their faces and how many photos they're in), and the faces
        // without one, gathered by likeness: "who is this?"
        "people" => {
            let all = known(db)?;
            let mut named = Vec::new();
            {
                let mut select = db.prepare("SELECT id, name FROM people ORDER BY name")?;
                let rows: Vec<(i64, String)> =
                    select.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?.filter_map(|row| row.ok()).collect();
                for (id, name) in rows {
                    let theirs: Vec<&Known> = all.iter().filter(|face| face.person == Some(id)).collect();
                    let photos: std::collections::HashSet<i64> = theirs.iter().map(|face| face.file).collect();
                    let shown: Vec<Value> = theirs.iter().take(5).map(|face| face_json(db, face.id, None)).collect();
                    named.push(json!({"person": id, "name": name, "photos": photos.len(), "faces": shown}));
                }
            }
            let mut groups: Vec<(Vec<f32>, Vec<&Known>)> = Vec::new();
            for face in all.iter().filter(|face| face.person.is_none() && !face.fixed) {
                let best = groups
                    .iter()
                    .enumerate()
                    .map(|(index, (sum, members))| {
                        let middle: Vec<f32> = sum.iter().map(|value| value / members.len() as f32).collect();
                        let length = cosine(&middle, &middle).sqrt().max(1e-9);
                        (index, cosine(&middle, &face.vector) / length)
                    })
                    .max_by(|a, b| a.1.total_cmp(&b.1));
                match best {
                    Some((index, closeness)) if closeness >= GATHERS => {
                        for (sum, value) in groups[index].0.iter_mut().zip(&face.vector) {
                            *sum += value;
                        }
                        groups[index].1.push(face);
                    }
                    _ => groups.push((face.vector.clone(), vec![face])),
                }
            }
            groups.sort_by(|a, b| b.1.len().cmp(&a.1.len()));
            let unnamed: Vec<Value> = groups
                .iter()
                .take(40)
                .map(|(_, members)| {
                    let photos: std::collections::HashSet<i64> = members.iter().map(|face| face.file).collect();
                    json!({
                        "photos": photos.len(),
                        "group": members.iter().map(|face| face.id).collect::<Vec<_>>(),
                        "faces": members.iter().take(5).map(|face| face_json(db, face.id, None)).collect::<Vec<_>>(),
                    })
                })
                .collect();
            Ok(json!({"named": named, "unnamed": unnamed}))
        }
        // The photos with a face like the (largest) one in a sample picture, the closest first.
        "face_example" => {
            let path = request["path"].as_str().ok_or_else(|| anyhow!("face_example needs a path"))?;
            let started = std::time::Instant::now();
            let picture = images::load(Path::new(path))?;
            let faces = Faces::open(&config::models_folder())?;
            let sample = faces.find(&picture.rgb, picture.width, picture.height)?;
            let Some(sample) = sample.first() else { bail!("no face in that picture") };
            let mut best: HashMap<i64, f32> = HashMap::new();
            for face in known(db)? {
                let closeness = cosine(&sample.vector, &face.vector);
                if closeness >= SHOWS {
                    let entry = best.entry(face.file).or_insert(f32::MIN);
                    *entry = entry.max(closeness);
                }
            }
            let mut ranked: Vec<(i64, f32)> = best.into_iter().collect();
            ranked.sort_by(|a, b| b.1.total_cmp(&a.1));
            ranked.truncate(request["limit"].as_u64().unwrap_or(120) as usize);
            let mut hits = Vec::new();
            for (file, closeness) in ranked {
                let Some(mut hit) = file_json(store, file)? else { continue };
                if hit["path"].as_str() == Some(path) {
                    continue;
                }
                let segment: i64 = db
                    .query_row("SELECT id FROM segments WHERE file = ? ORDER BY id LIMIT 1", [file], |row| row.get(0))
                    .unwrap_or(0);
                for (key, value) in [
                    ("score", json!(closeness)), ("z", Value::Null), ("cosine", json!(closeness)), ("words", json!(0.0)),
                    ("segment", json!(segment)), ("segment_kind", json!("image")), ("loc", Value::Null),
                    ("loc_end", Value::Null), ("excerpt", Value::Null), ("strong", json!(closeness >= SAME)),
                ] {
                    hit[key] = value;
                }
                hits.push(hit);
            }
            Ok(json!({
                "hits": hits, "ms": started.elapsed().as_secs_f64() * 1000.0, "vectors": 0,
                "filters": {"kinds": [], "folders": [], "extensions": [], "after": null, "before": null, "negative": [], "phrases": []},
            }))
        }
        _ => bail!("unknown op: {op}"),
    }
}

/// `glint-helper faces <picture>...`: how many faces each has, and how close the first face of each is to the
/// others' (to check the models on pictures whose people are known).
pub fn check(paths: &[String]) -> Result<()> {
    let faces = Faces::open(&config::models_folder())?;
    let mut firsts = Vec::new();
    for path in paths {
        let picture = images::load(Path::new(path))?;
        let started = std::time::Instant::now();
        let found = faces.find(&picture.rgb, picture.width, picture.height)?;
        println!(
            "{}: {} face(s) in {:.0} ms{}",
            Path::new(path).file_name().unwrap_or_default().to_string_lossy(),
            found.len(),
            started.elapsed().as_secs_f64() * 1000.0,
            found.first().map(|face| format!(", first {:.2} sure, {:.0}% wide", face.score, face.width * 100.0)).unwrap_or_default(),
        );
        firsts.push(found.into_iter().next());
    }
    for (index, one) in firsts.iter().enumerate() {
        let row: Vec<String> = firsts
            .iter()
            .map(|other| match (one, other) {
                (Some(a), Some(b)) => format!("{:5.2}", cosine(&a.vector, &b.vector)),
                _ => "    -".to_string(),
            })
            .collect();
        println!("{index:2} {}", row.join(" "));
    }
    Ok(())
}
