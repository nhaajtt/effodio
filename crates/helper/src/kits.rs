//! Two things Effodio can do with a download more, each run as a program of its own on this computer:
//! the words spoken in recordings and videos (whisper.cpp), and answers written from the files (llama.cpp with a
//! small language model). Neither is fetched until someone asks for it.

use crate::emit;
use anyhow::{anyhow, bail, Result};
use effodio_core::config;
use effodio_core::query;
use effodio_core::search::{Searcher, Settings};
use effodio_core::store::Store;
use effodio_core::text;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

struct Part {
    name: &'static str,
    url: &'static str,
    bytes: u64,
    sha256: &'static str,
    /// A zip of programs (unpacked into the kit's folder) rather than a model (kept with the models).
    programs: bool,
}

struct Kit {
    name: &'static str,
    /// The program to run, somewhere in the unpacked zip.
    program: &'static str,
    parts: &'static [Part],
}

const SPEECH: Kit = Kit {
    name: "speech",
    program: "whisper-cli.exe",
    parts: &[
        Part {
            name: "whisper-bin-x64.zip",
            url: "https://github.com/ggml-org/whisper.cpp/releases/download/b5454/whisper-bin-x64.zip",
            bytes: 8_928_640,
            sha256: "6ba69e3482d7826214f90a6a9c84ca07782aec1e1d0c6a7c30c994fd5d816ccb",
            programs: true,
        },
        Part {
            name: "ggml-small.bin",
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-small.bin",
            bytes: 487_601_967,
            sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
            programs: false,
        },
    ],
};

const ANSWERS: Kit = Kit {
    name: "answers",
    program: "llama-completion.exe",
    parts: &[
        Part {
            name: "llama-b12283-bin-win-cpu-x64.zip",
            url: "https://github.com/ggml-org/llama.cpp/releases/download/b12283/llama-b12283-bin-win-cpu-x64.zip",
            bytes: 25_058_174,
            sha256: "dafb19ac757bfb7cfc1f91c6e26703264df861c4331e3ef6dc9543ae8d010c5e",
            programs: true,
        },
        Part {
            name: "gemma-3-4b-it-Q4_K_M.gguf",
            url: "https://huggingface.co/ggml-org/gemma-3-4b-it-GGUF/resolve/d0976223747697cb51e056d85c532013931fe52e/gemma-3-4b-it-Q4_K_M.gguf",
            bytes: 2_489_757_856,
            sha256: "882e8d2db44dc554fb0ea5077cb7e4bc49e7342a1f0da57901c0802ea21a0863",
            programs: false,
        },
    ],
};

/// FFmpeg, for computers that don't have it: videos and recordings can't be read without.
const MEDIA: Kit = Kit {
    name: "media",
    program: "ffmpeg.exe",
    parts: &[Part {
        name: "ffmpeg-9.0.2-essentials_build.zip",
        url: "https://github.com/GyanD/codexffmpeg/releases/download/9.0.2/ffmpeg-9.0.2-essentials_build.zip",
        bytes: 114_768_076,
        sha256: "60f467265b1e312373dbcd92200c2618a74850f98d3d078e94296bb3fa2047ba",
        programs: true,
    }],
};

/// Fetches FFmpeg if this computer has none.
pub fn get_media() -> Result<()> {
    if effodio_core::readers::media::ffmpeg().is_some() {
        return Ok(());
    }
    get(&MEDIA)
}

fn kit(name: &str) -> Result<&'static Kit> {
    match name {
        "media" => Ok(&MEDIA),
        "speech" => Ok(&SPEECH),
        "answers" => Ok(&ANSWERS),
        _ => bail!("no such kit: {name}"),
    }
}

fn tools(kit: &Kit) -> PathBuf {
    config::models_folder().join("Tools").join(kit.name)
}

fn model(kit: &Kit) -> Option<PathBuf> {
    kit.parts.iter().find(|part| !part.programs).map(|part| config::models_folder().join(part.name))
}

/// The kit's program, wherever the zip put it.
fn program(kit: &Kit) -> Option<PathBuf> {
    fn find(folder: &Path, name: &str, depth: usize) -> Option<PathBuf> {
        for entry in std::fs::read_dir(folder).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() && depth < 3 {
                if let Some(found) = find(&path, name, depth + 1) {
                    return Some(found);
                }
            } else if path.file_name().is_some_and(|file| file.eq_ignore_ascii_case(name)) {
                return Some(path);
            }
        }
        None
    }
    find(&tools(kit), kit.program, 0)
}

fn ready(kit: &Kit) -> bool {
    tools(kit).join(".whole").exists() && model(kit).is_none_or(|file| file.exists()) && program(kit).is_some()
}

#[derive(Clone, Default)]
struct Progress {
    getting: bool,
    done: u64,
    error: Option<String>,
}

const IDLE: Progress = Progress { getting: false, done: 0, error: None };
static PROGRESS: Mutex<[Progress; 3]> = Mutex::new([IDLE, IDLE, IDLE]);

fn slot(kit: &Kit) -> usize {
    match kit.name {
        "speech" => 0,
        "answers" => 1,
        _ => 2,
    }
}

/// One part to `target`, from wherever `fetch` finds it whole.
fn fetch(part: &Part, target: &Path, before: u64, kit: &Kit) -> Result<()> {
    let wanted = crate::fetch::Wanted { name: part.name, bytes: part.bytes, sha256: part.sha256, origin: part.url };
    crate::fetch::obtain(&wanted, target, &mut |step| {
        if let crate::fetch::Step::Have(have) = step {
            PROGRESS.lock().unwrap()[slot(kit)].done = before + have;
        }
    })
}

/// Every file the app can need, for a pack: the search model, the face models and FFmpeg, and with `all` the speech and
/// answers kits too. Each is (name, bytes, SHA-256, where its makers publish it).
pub fn every_file(all: bool) -> Vec<(&'static str, u64, &'static str, String)> {
    let mut list: Vec<_> = effodio_embed::files::ALL.iter().map(|f| (f.name, f.bytes, f.sha256, f.url())).collect();
    list.extend(effodio_faces::files::ALL.iter().map(|f| (f.name, f.bytes, f.sha256, f.url())));
    let kits: &[&Kit] = if all { &[&MEDIA, &SPEECH, &ANSWERS] } else { &[&MEDIA] };
    for kit in kits {
        list.extend(kit.parts.iter().map(|p| (p.name, p.bytes, p.sha256, p.url.to_string())));
    }
    list
}

/// What a file is, who made it and under what licence, and what in Effodio needs it: said to the person installing.
pub fn what(name: &str) -> &'static str {
    match name {
        "embeddinggemma-2-Q8_0.gguf" => "EmbeddingGemma 2, text (Google DeepMind, Gemma terms, ai.google.dev/gemma/terms): search by meaning",
        "mmproj-embeddinggemma-2-Q8_0.gguf" => {
            "EmbeddingGemma 2, pictures and sound (Google DeepMind, Gemma terms, ai.google.dev/gemma/terms): photos and audio by meaning"
        }
        "face_detection_yunet_2023mar.onnx" => "YuNet face finder (OpenCV, MIT): People",
        "face_recognition_sface_2021dec.onnx" => "SFace face recognizer (OpenCV, Apache-2.0): People",
        "ffmpeg-9.0.2-essentials_build.zip" => {
            "FFmpeg 9.0.2 essentials (gyan.dev build, GPL-3.0): reads videos and recordings"
        }
        "whisper-bin-x64.zip" => "whisper.cpp b5454 (ggml.ai, MIT): the program that turns speech into text",
        "ggml-small.bin" => "Whisper small (OpenAI, MIT): the speech-to-text model",
        "llama-b12283-bin-win-cpu-x64.zip" => "llama.cpp b12283 for CPUs (ggml.ai, MIT): runs the answers model",
        "gemma-3-4b-it-Q4_K_M.gguf" => {
            "Gemma 3 4B instruct (Google DeepMind, Gemma terms, ai.google.dev/gemma/terms): answers questions about your files"
        }
        _ => "",
    }
}

/// Every file, as JSON for the website's install script: what it is and every place to get it, the first first.
pub fn manifest() -> Value {
    Value::Array(
        every_file(true)
            .into_iter()
            .map(|(name, bytes, sha256, origin)| {
                let wanted = crate::fetch::Wanted { name, bytes, sha256, origin: &origin };
                json!({"name": name, "bytes": bytes, "sha256": sha256, "what": what(name), "urls": crate::fetch::sources(&wanted)})
            })
            .collect(),
    )
}

/// Fills `folder` with a pack: every file, checked. A pack beside the app (the full installer's `models` folder) or
/// in `Models\Pack` is used instead of the web.
pub fn pack(folder: &Path, all: bool) -> Result<()> {
    for (name, bytes, sha256, origin) in every_file(all) {
        let wanted = crate::fetch::Wanted { name, bytes, sha256, origin: &origin };
        crate::fetch::obtain(&wanted, &folder.join(name), &mut |_| {})?;
        emit(&json!({"event": "packed", "file": name, "bytes": bytes, "sha256": sha256}));
    }
    Ok(())
}

fn get(kit: &'static Kit) -> Result<()> {
    let folder = tools(kit);
    std::fs::create_dir_all(&folder)?;
    let mut before = 0;
    for part in kit.parts {
        if part.programs {
            if !folder.join(".whole").exists() {
                let archive = folder.join(part.name);
                fetch(part, &archive, before, kit)?;
                let mut zip = zip::ZipArchive::new(std::fs::File::open(&archive)?)?;
                zip.extract(&folder)?;
                std::fs::remove_file(&archive).ok();
                std::fs::write(folder.join(".whole"), part.sha256)?;
            }
        } else {
            let target = config::models_folder().join(part.name);
            if !target.exists() {
                fetch(part, &target, before, kit)?;
            }
        }
        before += part.bytes;
        PROGRESS.lock().unwrap()[slot(kit)].done = before;
    }
    Ok(())
}

fn status_of(kit: &Kit) -> Value {
    let progress = PROGRESS.lock().unwrap()[slot(kit)].clone();
    json!({
        "ready": ready(kit), "getting": progress.getting, "done": progress.done,
        "total": kit.parts.iter().map(|part| part.bytes).sum::<u64>(), "error": progress.error,
    })
}

fn quiet(program: &Path) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // No console window flashing up.
        command.creation_flags(0x0800_0000);
    }
    command
}

fn threads() -> String {
    std::thread::available_parallelism().map(|count| (count.get() / 2).clamp(2, 12)).unwrap_or(4).to_string()
}

// ---- The words spoken in recordings ------------------------------------------------------------------------

/// The most of one recording that is listened to, in seconds.
const LISTEN: u32 = 40 * 60;

pub fn prepare(store: &Store) -> Result<()> {
    store.db.execute_batch(
        "CREATE TABLE IF NOT EXISTS speech_scans(file INTEGER PRIMARY KEY REFERENCES files(id) ON DELETE CASCADE, mtime REAL NOT NULL);",
    )?;
    Ok(())
}

/// What was said in a recording: (from, to, words), in seconds.
fn listen(kit: &Kit, path: &Path) -> Result<Vec<(f64, f64, String)>> {
    let ffmpeg = effodio_core::readers::media::ffmpeg().ok_or_else(|| anyhow!("FFmpeg isn't installed"))?;
    let folder = config::cache_folder().join("speech");
    std::fs::create_dir_all(&folder)?;
    let wave = folder.join(format!("{}.wav", std::process::id()));
    let made = quiet(&ffmpeg)
        .args(["-v", "error", "-nostdin", "-y", "-protocol_whitelist", effodio_core::readers::media::LOCAL_ONLY, "-i"])
        .arg(path)
        .args(["-vn", "-ac", "1", "-ar", "16000", "-t", &LISTEN.to_string()])
        .arg(&wave)
        .stdout(Stdio::null())
        .status()?;
    if !made.success() || !wave.exists() {
        std::fs::remove_file(&wave).ok();
        return Ok(Vec::new());
    }
    let base = folder.join(format!("{}", std::process::id()));
    let ran = quiet(&program(kit).ok_or_else(|| anyhow!("the speech kit isn't there"))?)
        .arg("-m")
        .arg(model(kit).unwrap_or_default())
        .arg("-f")
        .arg(&wave)
        .args(["-l", "auto", "-oj", "-np", "-t", &threads(), "-of"])
        .arg(&base)
        .stdout(Stdio::null())
        .status();
    std::fs::remove_file(&wave).ok();
    let written = base.with_extension("json");
    let read = std::fs::read(&written);
    std::fs::remove_file(&written).ok();
    ran?;
    let value: Value = serde_json::from_slice(&read?)?;
    let mut out = Vec::new();
    for line in value["transcription"].as_array().map(Vec::as_slice).unwrap_or_default() {
        let words = line["text"].as_str().unwrap_or_default().trim();
        // What it writes for silence and music.
        if words.is_empty()
            || (words.starts_with('[') && words.ends_with(']'))
            || (words.starts_with('(') && words.ends_with(')'))
        {
            continue;
        }
        // What it makes up over music and silence.
        let folded = text::fold(words);
        if ["subscribe", "ghien mi go", "thanks for watching", "subtitles by", "amara.org"]
            .iter()
            .any(|made| folded.contains(made))
        {
            continue;
        }
        let from = line["offsets"]["from"].as_f64().unwrap_or(0.0) / 1000.0;
        let to = line["offsets"]["to"].as_f64().unwrap_or(0.0) / 1000.0;
        out.push((from, to, words.to_string()));
    }
    Ok(out)
}

/// Listens to the recordings and videos that haven't been listened to (or have changed since), and keeps what
/// was said, half a minute at a time, where a search for the words finds it at its second.
pub fn listen_all(store: &mut Store, stop: &AtomicBool) -> Result<usize> {
    if !ready(&SPEECH) {
        return Ok(0);
    }
    prepare(store)?;
    let waiting: Vec<(i64, String, f64)> = {
        let mut select = store.db.prepare(
            "SELECT files.id, files.path, files.mtime FROM files
             LEFT JOIN speech_scans ON speech_scans.file = files.id
             WHERE files.kind IN ('audio', 'video') AND files.state = 'done'
               AND (speech_scans.file IS NULL OR speech_scans.mtime != files.mtime) ORDER BY files.id",
        )?;
        let rows = select.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))?;
        rows.collect::<effodio_core::rusqlite::Result<_>>()?
    };
    if waiting.is_empty() {
        return Ok(0);
    }
    emit(&json!({"event": "deepening", "total": waiting.len()}));
    let mut done = 0;
    for (file, path, mtime) in &waiting {
        if stop.load(Ordering::Relaxed) {
            break;
        }
        emit(&json!({"event": "file", "done": done, "total": waiting.len(), "path": path}));
        let said = listen(&SPEECH, Path::new(path)).unwrap_or_default();
        // Half a minute of talk to a passage.
        let mut passages: Vec<(f64, f64, String)> = Vec::new();
        for (from, to, words) in said {
            match passages.last_mut() {
                Some(last) if to - last.0 <= 30.0 => {
                    last.1 = to;
                    last.2.push(' ');
                    last.2.push_str(&words);
                }
                _ => passages.push((from, to, words)),
            }
        }
        let change = store.db.transaction()?;
        change.execute(
            "DELETE FROM fts WHERE rowid IN (SELECT id FROM segments WHERE file = ? AND kind = 'sound' AND modality = 'none')",
            [file],
        )?;
        change.execute("DELETE FROM segments WHERE file = ? AND kind = 'sound' AND modality = 'none'", [file])?;
        for (from, to, words) in &passages {
            change.execute(
                "INSERT INTO segments(file, kind, modality, loc, loc_end, excerpt, late, vec) VALUES(?, 'sound', 'none', ?, ?, ?, 1, NULL)",
                (file, from, to, text::excerpt(words, 200)),
            )?;
            change.execute(
                "INSERT INTO fts(rowid, name, body, original) VALUES(?, '', ?, ?)",
                (change.last_insert_rowid(), text::fold(words), words),
            )?;
        }
        change.execute("INSERT OR REPLACE INTO speech_scans(file, mtime) VALUES(?, ?)", (file, mtime))?;
        change.commit()?;
        done += 1;
    }
    Ok(done)
}

// ---- Answers from the files ---------------------------------------------------------------------------------

/// Answers a question from the passages a search for it finds. `vector`: the question's embedding.
pub fn answer(searcher: &mut Searcher, question: &str, vector: Option<&[f32]>, vietnamese: bool) -> Result<Value> {
    let kit = &ANSWERS;
    let program = program(kit).filter(|_| ready(kit)).ok_or_else(|| anyhow!("the answers kit isn't there"))?;
    let started = std::time::Instant::now();
    let parsed = query::parse(question, 0.0);
    let hits = searcher.search(&parsed, vector, None, None, &Settings { limit: 24, ..Settings::default() })?;
    let mut sources: Vec<(Value, String)> = Vec::new();
    for hit in &hits {
        if sources.len() == 6 {
            break;
        }
        let words = searcher.text_of(hit.segment)?.or_else(|| hit.excerpt.clone()).unwrap_or_default();
        let words: String = words.replace(['«', '»'], "").chars().take(1400).collect();
        // One passage a file is enough to answer from, and leaves room for other files.
        if words.trim().len() < 40 || sources.iter().any(|(known, _)| known["file"] == json!(hit.file)) {
            continue;
        }
        sources.push((serde_json::to_value(hit)?, words));
    }
    if sources.is_empty() {
        return Ok(json!({"answer": null, "sources": [], "seconds": 0.0}));
    }
    let mut prompt = String::from("<start_of_turn>user\n");
    prompt.push_str(
        "Answer the question using only the numbered sources below, which come from the user's own files. \
         After each statement give the number of the source it comes from in square brackets, like [2]. \
         If the sources don't contain the answer, say that they don't. Be brief: a few sentences.",
    );
    prompt.push_str(if vietnamese {
        " Answer in Vietnamese.\n\n"
    } else {
        " Answer in the language of the question.\n\n"
    });
    for (index, (hit, words)) in sources.iter().enumerate() {
        let name = Path::new(hit["path"].as_str().unwrap_or_default())
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        prompt.push_str(&format!("[{}] {name}\n{words}\n\n", index + 1));
    }
    prompt.push_str(&format!("Question: {question}<end_of_turn>\n<start_of_turn>model\n"));
    let folder = config::cache_folder().join("answers");
    std::fs::create_dir_all(&folder)?;
    let file = folder.join(format!("{}.txt", std::process::id()));
    std::fs::write(&file, &prompt)?;
    let output = quiet(&program)
        .arg("-m")
        .arg(model(kit).unwrap_or_default())
        .arg("-f")
        .arg(&file)
        .args([
            "-n",
            "400",
            "--temp",
            "0.2",
            "-c",
            "8192",
            "-t",
            &threads(),
            "-no-cnv",
            "--no-display-prompt",
            "--simple-io",
        ])
        .output();
    std::fs::remove_file(&file).ok();
    let output = output?;
    // The program colours its output for a terminal: the colour codes go.
    let raw = String::from_utf8_lossy(&output.stdout).replace('\r', "");
    let mut answer = String::new();
    let mut letters = raw.chars();
    while let Some(letter) = letters.next() {
        if letter == '\u{1b}' {
            for code in letters.by_ref() {
                if code.is_ascii_alphabetic() {
                    break;
                }
            }
        } else {
            answer.push(letter);
        }
    }
    for end in ["<end_of_turn>", "[end of text]", "<eos>"] {
        if let Some(at) = answer.find(end) {
            answer.truncate(at);
        }
    }
    let answer = answer.trim();
    if answer.is_empty() {
        bail!("the language model gave no answer");
    }
    Ok(json!({
        "answer": answer,
        "sources": sources.into_iter().map(|(hit, _)| hit).collect::<Vec<_>>(),
        "seconds": started.elapsed().as_secs_f64(),
    }))
}

/// The requests about the kits themselves. `None`: not one of them.
pub fn handle(request: &Value) -> Option<Result<Value>> {
    match request["op"].as_str().unwrap_or_default() {
        "kits" => {
            let mut media = status_of(&MEDIA);
            // FFmpeg from anywhere will do: installed on the computer, or the copy fetched here.
            media["ready"] = json!(effodio_core::readers::media::ffmpeg().is_some());
            Some(Ok(json!({"speech": status_of(&SPEECH), "answers": status_of(&ANSWERS), "media": media})))
        }
        // Is there a newer Effodio? Asks the website for its version number, and sends nothing.
        "update_check" => Some((|| {
            let current = env!("CARGO_PKG_VERSION");
            let reply = crate::web::get("https://effodio.nhaajt.com/version.json")
                .set("User-Agent", "effodio")
                .timeout(std::time::Duration::from_secs(8))
                .call()?;
            let mut body = String::new();
            reply.into_reader().take(4096).read_to_string(&mut body)?;
            let latest = serde_json::from_str::<Value>(&body)?["version"].as_str().unwrap_or_default().to_string();
            let parts =
                |version: &str| -> Vec<u64> { version.split('.').map(|part| part.parse().unwrap_or(0)).collect() };
            Ok(
                json!({"current": current, "latest": latest, "newer": !latest.is_empty() && parts(&latest) > parts(current)}),
            )
        })()),
        // Starts the download and returns at once: `kits` says how far it is.
        "kit_get" => Some((|| {
            let kit = kit(request["kit"].as_str().unwrap_or_default())?;
            {
                let mut all = PROGRESS.lock().unwrap();
                if all[slot(kit)].getting {
                    return Ok(json!({}));
                }
                all[slot(kit)] = Progress { getting: true, done: 0, error: None };
            }
            std::thread::spawn(move || {
                let result = get(kit);
                let mut all = PROGRESS.lock().unwrap();
                all[slot(kit)].getting = false;
                all[slot(kit)].error = result.err().map(|error| format!("{error:#}"));
            });
            Ok(json!({}))
        })()),
        "kit_remove" => Some((|| {
            let kit = kit(request["kit"].as_str().unwrap_or_default())?;
            std::fs::remove_dir_all(tools(kit)).ok();
            if let Some(file) = model(kit) {
                std::fs::remove_file(file).ok();
            }
            Ok(json!({}))
        })()),
        _ => None,
    }
}

#[cfg(test)]
mod pack_tests {
    /// Every file the install script fetches says what it is.
    #[test]
    fn every_file_is_described() {
        for (name, ..) in super::every_file(true) {
            assert!(!super::what(name).is_empty(), "{name} has no description");
        }
    }
}
