//! Glint's helper process. It runs the model, so the app itself stays small and a broken file can't take it down.
//!
//!   glint-helper index            read the chosen folders into the index (events as JSON lines on stdout)
//!   glint-helper serve            answer searches: JSON lines in on stdin, JSON lines out on stdout
//!   glint-helper search <words>   one search, printed
//!   glint-helper speed            measure this computer, choose processor or graphics card
//!   glint-helper estimate         how many files the chosen folders hold, and how long reading them takes
//!   glint-helper model            download the model files (progress as JSON lines)
//!   glint-helper status           what's in the index

mod model;
mod serve;
mod speed;

use anyhow::{bail, Result};
use glint_core::config::{self, Config, Speed};
use glint_core::crawler::crawl;
use glint_core::indexer::{Embed, Event, Indexer, Options};
use glint_core::kinds::FileKind;
use glint_core::readers::Picture;
use glint_core::store::Store;
use glint_embed::{Device, Embedder};
use serde_json::json;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// The model behind the engine's `Embed`.
pub struct Model(pub Embedder);

impl Embed for Model {
    fn texts(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.0.embed_texts(texts)
    }

    fn image(&mut self, picture: &Picture, budget: usize) -> Result<Vec<f32>> {
        self.0.embed_image(&picture.rgb, picture.width, picture.height, budget)
    }

    fn audio(&mut self, samples: &[f32]) -> Result<Vec<f32>> {
        self.0.embed_audio(samples)
    }
}

/// What every vector in an index must have come from.
pub fn vector_source() -> String {
    format!("embeddinggemma-2@{}/q8_0/v{}", &glint_embed::files::REVISION[..12], glint_embed::VECTOR_VERSION)
}

/// The device to run on: what the settings say, or what the speed check found faster.
pub fn device(config: &Config) -> Device {
    match config.device.as_str() {
        "cpu" => Device::Cpu,
        "gpu" => Device::Gpu,
        _ => match Speed::load() {
            Some(speed) if speed.device == "gpu" => Device::Gpu,
            _ => Device::Cpu,
        },
    }
}

pub fn open_model(text_only: bool, device: Device) -> Result<Embedder> {
    Embedder::open(&glint_embed::Options::in_folder(&config::models_folder(), text_only, device))
}

/// One JSON value per line on stdout, flushed: the app reads them as they come.
pub fn emit(value: &serde_json::Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{value}");
    let _ = out.flush();
}

/// Arguments after the command: `--name value` pairs and bare words.
struct Arguments {
    named: Vec<(String, String)>,
    bare: Vec<String>,
}

impl Arguments {
    fn parse(arguments: &[String]) -> Arguments {
        let (mut named, mut bare) = (Vec::new(), Vec::new());
        let mut index = 0;
        while index < arguments.len() {
            match arguments[index].strip_prefix("--") {
                Some(name) if index + 1 < arguments.len() && !arguments[index + 1].starts_with("--") => {
                    named.push((name.to_string(), arguments[index + 1].clone()));
                    index += 1;
                }
                Some(name) => named.push((name.to_string(), "true".to_string())),
                None => bare.push(arguments[index].clone()),
            }
            index += 1;
        }
        Arguments { named, bare }
    }

    fn get(&self, name: &str) -> Option<&str> {
        self.named.iter().find(|(key, _)| key == name).map(|(_, value)| value.as_str())
    }

    fn database(&self) -> PathBuf {
        self.get("db").map(PathBuf::from).unwrap_or_else(config::index_file)
    }

    fn config(&self) -> Config {
        match self.get("config") {
            Some(file) => Config::load_from(std::path::Path::new(file)),
            None => Config::load(),
        }
    }
}

/// Lines on stdin steer a run: `pause`, `resume`, `stop`. The app closing the pipe stops it too.
fn listen(stop: Arc<AtomicBool>, pause: Arc<AtomicBool>) {
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line.as_deref().map(str::trim) {
                Ok("pause") => pause.store(true, Ordering::Relaxed),
                Ok("resume") => pause.store(false, Ordering::Relaxed),
                Ok("stop") | Err(_) => break,
                _ => {}
            }
        }
        pause.store(false, Ordering::Relaxed);
        stop.store(true, Ordering::Relaxed);
    });
}

fn index(arguments: &Arguments) -> Result<()> {
    let config = arguments.config();
    let stop = Arc::new(AtomicBool::new(false));
    let pause = Arc::new(AtomicBool::new(false));
    if arguments.get("detached").is_none() {
        listen(stop.clone(), pause.clone());
    }
    let mut store = Store::open(&arguments.database())?;
    if store.claim_vector_source(&vector_source())? {
        emit(&json!({"event": "reset", "reason": "the index was made by another model version"}));
    }
    emit(&json!({"event": "crawling"}));
    let found = crawl(&config.roots, &config.rules, &stop);
    if stop.load(Ordering::Relaxed) {
        emit(&json!({"event": "finished", "indexed": 0, "failed": 0, "seconds": 0.0, "stopped": true}));
        return Ok(());
    }
    let (added, changed, removed) = store.reconcile(&config.root_paths(), &found.files)?;
    emit(&json!({
        "event": "crawled", "files": found.files.len(), "added": added, "changed": changed, "removed": removed,
        "cloud_only": found.cloud_only, "datasets": found.datasets,
    }));
    glint_core::insight::backfill_tones(&mut store, &stop)?;
    let counts = store.counts()?;
    if counts.pending == 0 && counts.partial == 0 {
        emit(&json!({"event": "finished", "indexed": 0, "failed": 0, "seconds": 0.0, "stopped": false}));
        return Ok(());
    }
    emit(&json!({"event": "loading"}));
    let mut model = Model(open_model(false, device(&config))?);
    emit(&json!({"event": "loaded", "gpu": model.0.gpu, "backends": Embedder::backends()}));
    let options = Options { ocr: config.ocr, ..Options::default() };
    let mut indexer = Indexer { store: &mut store, embed: &mut model, options, stop: &stop, pause: &pause };
    indexer.run(|event: Event| emit(&serde_json::to_value(&event).unwrap_or_default()))?;
    Ok(())
}

/// Reads single files the watcher saw change (paths on the command line), without a crawl.
fn touch(arguments: &Arguments) -> Result<()> {
    use glint_core::crawler::{change, Change};
    let config = arguments.config();
    let mut store = Store::open(&arguments.database())?;
    store.claim_vector_source(&vector_source())?;
    let stop = AtomicBool::new(false);
    let mut walk = false;
    for path in &arguments.bare {
        let path = PathBuf::from(path);
        match change(&path, &config.roots, &config.rules) {
            Change::Read(seen) => {
                store.touch(&seen)?;
            }
            Change::Gone => {
                store.forget(&path)?;
            }
            Change::LeftOut => {
                store.forget_file(&path)?;
            }
            // A folder was renamed, moved in or changed in a way only a walk can tell.
            Change::Folder => walk = true,
        }
    }
    if walk {
        let found = crawl(&config.roots, &config.rules, &stop);
        store.reconcile(&config.root_paths(), &found.files)?;
    }
    let counts = store.counts()?;
    if counts.pending == 0 && counts.partial == 0 {
        emit(&json!({"event": "finished", "indexed": 0, "failed": 0, "seconds": 0.0, "stopped": false}));
        return Ok(());
    }
    let pause = AtomicBool::new(false);
    let mut model = Model(open_model(false, device(&config))?);
    let options = Options { ocr: config.ocr, ..Options::default() };
    let mut indexer = Indexer { store: &mut store, embed: &mut model, options, stop: &stop, pause: &pause };
    indexer.run(|event: Event| emit(&serde_json::to_value(&event).unwrap_or_default()))?;
    Ok(())
}

fn estimate(arguments: &Arguments) -> Result<()> {
    let config = arguments.config();
    let found = crawl(&config.roots, &config.rules, &AtomicBool::new(false));
    let speed = Speed::load().unwrap_or_else(Speed::guess);
    let mut by_kind = serde_json::Map::new();
    let mut seconds = 0.0;
    for kind in FileKind::ALL {
        let files: Vec<_> = found.files.iter().filter(|file| file.kind == kind).collect();
        let bytes: u64 = files.iter().map(|file| file.size).sum();
        // Rough sizes: a PDF page is about 100 KB and a passage 2 KB of text; a minute of sound about 1 MB and of
        // video 10 MB. Each file's first pass is capped, as the indexer caps it.
        let each = |file: &&glint_core::store::Seen| -> f64 {
            match kind {
                FileKind::Image => speed.image,
                FileKind::Screenshot => speed.screenshot,
                FileKind::Pdf => (file.size as f64 / 100e3).clamp(1.0, 30.0) * speed.passage * 1.5,
                FileKind::Doc => (file.size as f64 / 2e3).clamp(1.0, 40.0) * speed.passage,
                FileKind::Audio => (file.size as f64 / 1e6).clamp(0.5, 30.0) * 2.4 * speed.sound,
                FileKind::Video => {
                    let minutes = (file.size as f64 / 10e6).clamp(0.5, 30.0);
                    minutes * 2.4 * speed.sound + minutes * 6.0 * speed.image * 0.6
                }
            }
        };
        let time: f64 = files.iter().map(each).sum();
        seconds += time;
        by_kind.insert(kind.as_str().into(), json!({"files": files.len(), "bytes": bytes, "seconds": time.round()}));
    }
    emit(&json!({
        "files": found.files.len(), "seconds": seconds.round(), "kinds": by_kind, "cloud_only": found.cloud_only,
        "datasets": found.datasets, "measured": Speed::load().is_some(), "device": speed.device,
    }));
    Ok(())
}

fn status(arguments: &Arguments) -> Result<()> {
    let database = arguments.database();
    let counts = if database.exists() { Store::open(&database)?.counts()? } else { Default::default() };
    let models = config::models_folder();
    let missing: Vec<&str> = glint_embed::files::ALL
        .iter()
        .filter(|file| !models.join(file.name).exists())
        .map(|file| file.name)
        .collect();
    emit(&json!({
        "counts": counts,
        "index": database,
        "index_bytes": std::fs::metadata(&database).map(|m| m.len()).unwrap_or(0),
        "model_ready": missing.is_empty(),
        "model_missing": missing,
        "ffmpeg": glint_core::readers::media::ffmpeg(),
        "speed": Speed::load(),
        "vector_source": vector_source(),
    }));
    Ok(())
}

fn search(arguments: &Arguments) -> Result<()> {
    let words = arguments.bare.join(" ");
    if words.trim().is_empty() {
        bail!("say what to look for: glint-helper search <words>");
    }
    let mut session = serve::Session::open(&arguments.database(), arguments.config())?;
    let limit = arguments.get("limit").and_then(|value| value.parse().ok()).unwrap_or(10);
    let result = session.handle(&json!({"op": "search", "q": words, "limit": limit}))?;
    if arguments.get("json").is_some() {
        emit(&result);
        return Ok(());
    }
    let hits = result["hits"].as_array().cloned().unwrap_or_default();
    if hits.is_empty() {
        println!("Nothing found.");
    }
    for hit in hits {
        let place = match (hit["segment_kind"].as_str(), hit["loc"].as_f64()) {
            (Some("page"), Some(page)) => format!(" · page {page}"),
            (Some("sound" | "frame"), Some(second)) => {
                format!(" · {}:{:02}", second as u64 / 60, second as u64 % 60)
            }
            _ => String::new(),
        };
        println!(
            "{:6.2} {} {}{}{}",
            hit["score"].as_f64().unwrap_or(0.0),
            if hit["strong"].as_bool() == Some(true) { " " } else { "~" },
            hit["path"].as_str().unwrap_or_default(),
            place,
            hit["excerpt"].as_str().map(|e| format!("\n         {}", e.chars().take(140).collect::<String>())).unwrap_or_default(),
        );
    }
    println!("({} ms)", result["ms"].as_f64().unwrap_or(0.0).round());
    Ok(())
}

/// Reads the text in one picture and says which languages Windows can read: for checking a computer.
fn ocr(arguments: &Arguments) -> Result<()> {
    #[cfg(windows)]
    {
        use glint_core::readers::{images, winrt};
        emit(&json!({"languages": winrt::ocr_languages()}));
        for path in &arguments.bare {
            let picture = images::load(std::path::Path::new(path))?;
            match winrt::ocr(&picture) {
                Ok(text) => emit(&json!({"path": path, "width": picture.width, "height": picture.height, "text": text})),
                Err(error) => emit(&json!({"path": path, "error": format!("{error:#}")})),
            }
        }
    }
    #[cfg(not(windows))]
    let _ = arguments;
    Ok(())
}

fn main() {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    let command = arguments.first().map(String::as_str).unwrap_or("help");
    let rest = Arguments::parse(arguments.get(1..).unwrap_or_default());
    let result = match command {
        "index" => index(&rest),
        "touch" => touch(&rest),
        "serve" => serve::run(&rest.database(), rest.config()),
        "search" => search(&rest),
        "speed" => speed::run(rest.get("quick").is_some()),
        "speed-gpu" => speed::gpu_child(rest.get("quick").is_some()),
        "estimate" => estimate(&rest),
        "model" => model::download(&config::models_folder()),
        "status" => status(&rest),
        "ocr" => ocr(&rest),
        _ => {
            println!(
                "glint-helper <index|touch|serve|search|speed|estimate|model|status> [--db file] [--config file]"
            );
            Ok(())
        }
    };
    if let Err(error) = result {
        emit(&json!({"event": "error", "error": format!("{error:#}")}));
        std::process::exit(1);
    }
}
