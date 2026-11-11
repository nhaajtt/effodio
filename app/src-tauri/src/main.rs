//! Effodio for Windows: the tray icon, the hotkey, the search panel and the window, over Effodio's helper.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod door;
mod helpers;
mod native;

use effodio_core::config::{self, Config};
use helpers::{Serve, Task};
use native::Job;
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::window::{Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_autostart::ManagerExt as _;
use tauri_plugin_dialog::DialogExt as _;
use tauri_plugin_global_shortcut::{GlobalShortcutExt as _, ShortcutState};

/// How indexing is going, as the interface shows it.
#[derive(Clone, Debug, Default, Serialize)]
pub struct IndexState {
    pub running: bool,
    pub paused: bool,
    /// `idle`, `crawling`, `loading`, `reading` or `deepening`.
    pub phase: String,
    pub done: usize,
    pub total: usize,
    pub current: Option<String>,
    pub failed: usize,
    /// Why indexing isn't running though there may be work: `model`, `battery`, `paused`, or nothing.
    pub waiting_for: Option<String>,
    pub error: Option<String>,
    /// Seconds since 1970 when the last run ended.
    pub finished_at: Option<f64>,
    pub last_indexed: usize,
}

pub struct App {
    pub serve: Serve,
    job: Arc<Option<Job>>,
    index: Mutex<Option<Task>>,
    index_state: Mutex<IndexState>,
    model: Mutex<Option<Task>>,
    speed: Mutex<Option<Task>>,
    /// Files the watcher saw change, and when the last one did.
    dirty: Mutex<(HashSet<PathBuf>, Instant)>,
    /// A full pass is wanted once the current run ends.
    again: AtomicBool,
    watcher: Mutex<Option<notify::RecommendedWatcher>>,
    /// The panel stays open when it loses the focus (a file dialog or a drag is under way).
    pinned: AtomicBool,
    pause_item: Mutex<Option<MenuItem<tauri::Wry>>>,
    hotkey: Mutex<String>,
    door: Mutex<Option<door::Door>>,
    /// Where the window should open (a view or a search), until its page picks it up.
    launch: Mutex<Option<Value>>,
}

fn state(app: &AppHandle) -> tauri::State<'_, App> {
    app.state::<App>()
}

fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0)
}

fn text<E: std::fmt::Display>(error: E) -> String {
    error.to_string()
}

fn model_ready() -> bool {
    let folder = config::models_folder();
    ["embeddinggemma-2-Q8_0.gguf", "mmproj-embeddinggemma-2-Q8_0.gguf"].iter().all(|name| folder.join(name).exists())
}

// ---------------------------------------------------------------------------------------------------------------
// Indexing

fn publish(app: &AppHandle) {
    let current = state(app).index_state.lock().unwrap().clone();
    let _ = app.emit("index", &current);
    if let Some(tray) = app.tray_by_id("main") {
        let tip = if current.running && current.total > 0 {
            format!("Effodio — reading {} of {}", current.done.min(current.total), current.total)
        } else if current.paused {
            "Effodio — paused".to_string()
        } else {
            "Effodio".to_string()
        };
        let _ = tray.set_tooltip(Some(&tip));
    }
    if let Some(item) = state(app).pause_item.lock().unwrap().as_ref() {
        let _ = item.set_text(if current.paused { "Resume indexing" } else { "Pause indexing" });
    }
}

fn on_index_event(app: &AppHandle, event: &Value) {
    {
        let shared = state(app);
        let mut current = shared.index_state.lock().unwrap();
        match event["event"].as_str().unwrap_or_default() {
            "crawling" => current.phase = "crawling".into(),
            "loading" => current.phase = "loading".into(),
            "started" => {
                current.phase = "reading".into();
                current.total = event["total"].as_u64().unwrap_or(0) as usize;
                current.done = 0;
                current.failed = 0;
            }
            "deepening" => {
                current.phase = "deepening".into();
                current.total = event["total"].as_u64().unwrap_or(0) as usize;
                current.done = 0;
            }
            "file" | "failed" => {
                current.done = event["done"].as_u64().unwrap_or(0) as usize;
                current.total = event["total"].as_u64().unwrap_or(current.total as u64) as usize;
                current.current = event["path"].as_str().map(str::to_string);
                if event["event"] == "failed" {
                    current.failed += 1;
                }
            }
            "finished" => {
                current.last_indexed = event["indexed"].as_u64().unwrap_or(0) as usize;
                current.finished_at = Some(now());
                current.error = None;
            }
            "error" => current.error = event["error"].as_str().map(str::to_string),
            _ => {}
        }
    }
    publish(app);
}

/// Starts a run unless one is under way: a full pass over the chosen folders, or just `paths` the watcher saw.
fn start_index(app: &AppHandle, paths: Option<Vec<PathBuf>>) {
    let shared = state(app);
    let settings = Config::load();
    let waiting_for = if !model_ready() {
        Some("model")
    } else if settings.paused {
        Some("paused")
    } else if settings.only_on_power && !native::on_power() {
        Some("battery")
    } else {
        None
    };
    {
        let mut current = shared.index_state.lock().unwrap();
        current.paused = settings.paused;
        current.waiting_for = waiting_for.map(str::to_string);
        if current.running {
            if paths.is_none() {
                shared.again.store(true, Ordering::Relaxed);
            } else if let Some(paths) = paths {
                shared.dirty.lock().unwrap().0.extend(paths);
            }
            return;
        }
        if waiting_for.is_some() || settings.roots.is_empty() {
            drop(current);
            publish(app);
            return;
        }
        current.running = true;
        current.phase = "crawling".into();
        current.error = None;
        current.current = None;
    }
    let mut arguments: Vec<String> = vec![if paths.is_some() { "touch".into() } else { "index".into() }];
    if let Some(paths) = &paths {
        arguments.extend(paths.iter().map(|path| path.to_string_lossy().into_owned()));
    }
    let arguments: Vec<&str> = arguments.iter().map(String::as_str).collect();
    let (events, ended) = (app.clone(), app.clone());
    let task = Task::start(
        &arguments,
        &shared.job,
        move |event| on_index_event(&events, &event),
        move || {
            let shared = state(&ended);
            *shared.index.lock().unwrap() = None;
            {
                let mut current = shared.index_state.lock().unwrap();
                current.running = false;
                current.phase = "idle".into();
                current.current = None;
            }
            publish(&ended);
            let _ = ended.emit("index-finished", ());
            if shared.again.swap(false, Ordering::Relaxed) {
                start_index(&ended, None);
            }
        },
    );
    match task {
        Ok(task) => *shared.index.lock().unwrap() = Some(task),
        Err(error) => {
            let mut current = shared.index_state.lock().unwrap();
            current.running = false;
            current.phase = "idle".into();
            current.error = Some(error.to_string());
        }
    }
    publish(app);
}

/// Watches the chosen folders; what changes is read a few seconds after things go quiet.
fn watch(app: &AppHandle) {
    use notify::{RecursiveMode, Watcher};
    let settings = Config::load();
    let handle = app.clone();
    let watcher = notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
        let Ok(event) = result else { return };
        if matches!(event.kind, notify::EventKind::Access(_)) {
            return;
        }
        let shared = state(&handle);
        let mut dirty = shared.dirty.lock().unwrap();
        for path in event.paths {
            // The index's own files and Office's lock files change all the time.
            let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with("~$") || name.ends_with(".tmp") || name.ends_with(".crdownload") {
                continue;
            }
            dirty.0.insert(path);
        }
        dirty.1 = Instant::now();
    });
    let Ok(mut watcher) = watcher else { return };
    for root in &settings.roots {
        let _ = watcher.watch(&root.path, RecursiveMode::Recursive);
    }
    *state(app).watcher.lock().unwrap() = Some(watcher);
}

fn watch_loop(app: AppHandle) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        let shared = state(&app);
        if shared.index_state.lock().unwrap().running {
            continue;
        }
        let paths: Vec<PathBuf> = {
            let mut dirty = shared.dirty.lock().unwrap();
            if dirty.0.is_empty() || dirty.1.elapsed() < Duration::from_secs(3) {
                continue;
            }
            dirty.0.drain().collect()
        };
        // A command line holds about 32,000 characters: past that, a full pass finds the changes itself.
        let length: usize = paths.iter().map(|path| path.as_os_str().len() + 3).sum();
        if paths.len() > 150 || length > 20_000 {
            start_index(&app, None);
        } else {
            start_index(&app, Some(paths));
        }
    });
}

// ---------------------------------------------------------------------------------------------------------------
// Windows

fn panel(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("panel")
}

fn build_panel(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, "panel", WebviewUrl::App("index.html#panel".into()))
        .title("Effodio Search")
        .inner_size(820.0, 560.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .effects(EffectsBuilder::new().effect(Effect::Acrylic).build())
        .build()
}

fn build_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html#main".into()))
        .title("Effodio")
        .inner_size(1220.0, 780.0)
        .min_inner_size(860.0, 560.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .visible(false)
        .effects(EffectsBuilder::new().effect(Effect::Mica).build())
        .build()
}

fn toggle_panel(app: &AppHandle) {
    let Some(window) = panel(app) else { return };
    if window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false) {
        let _ = window.hide();
        return;
    }
    // On the screen the pointer is on, a little above its middle.
    if let Ok(Some(monitor)) = window.current_monitor().map(|m| m.or(window.primary_monitor().ok().flatten())) {
        let cursor = app.cursor_position().ok();
        let monitors = window.available_monitors().unwrap_or_default();
        let target = cursor
            .and_then(|point| {
                monitors.iter().find(|m| {
                    let (position, size) = (m.position(), m.size());
                    point.x >= position.x as f64
                        && point.x < position.x as f64 + size.width as f64
                        && point.y >= position.y as f64
                        && point.y < position.y as f64 + size.height as f64
                })
            })
            .unwrap_or(&monitor);
        if let Ok(size) = window.outer_size() {
            let (position, area) = (target.position(), target.size());
            let x = position.x + (area.width as i32 - size.width as i32) / 2;
            let y = position.y + ((area.height as i32 - size.height as i32) as f64 * 0.34) as i32;
            let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
        }
    }
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("panel-shown", ());
}

fn show_main(app: &AppHandle, view: Option<&str>, query: Option<&str>) {
    let window = match app.get_webview_window("main") {
        Some(window) => window,
        None => match build_main(app) {
            Ok(window) => {
                // Its page isn't loaded yet: it asks for where to go once it is.
                *state(app).launch.lock().unwrap() = Some(json!({"view": view, "query": query}));
                window
            }
            Err(_) => return,
        },
    };
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    if view.is_some() || query.is_some() {
        let _ = window.emit("navigate", json!({"view": view, "query": query}));
    }
}

fn register_hotkey(app: &AppHandle, wanted: &str) -> Result<(), String> {
    let shared = state(app);
    let mut current = shared.hotkey.lock().unwrap();
    let shortcuts = app.global_shortcut();
    if !current.is_empty() {
        let _ = shortcuts.unregister(current.as_str());
    }
    match shortcuts.register(wanted) {
        Ok(()) => {
            *current = wanted.to_string();
            Ok(())
        }
        Err(error) => {
            // Back to the one that worked, so the panel still opens.
            if !current.is_empty() {
                let _ = shortcuts.register(current.as_str());
            }
            Err(format!("Windows wouldn't give Effodio that shortcut ({error}). Another app may hold it."))
        }
    }
}

/// What a change of settings takes effect through.
fn apply(app: &AppHandle, settings: &Config, before: Option<&Config>) -> Result<(), String> {
    let scope = app.asset_protocol_scope();
    let _ = scope.allow_directory(config::cache_folder(), true);
    for root in &settings.roots {
        let _ = scope.allow_directory(&root.path, true);
    }
    if before.map_or(true, |old| old.autostart != settings.autostart) {
        let auto = app.autolaunch();
        let _ = if settings.autostart { auto.enable() } else { auto.disable() };
    }
    if before.map_or(true, |old| old.roots != settings.roots) {
        watch(app);
    }
    if before.map_or(true, |old| old.door != settings.door) {
        let shared = state(app);
        let mut slot = shared.door.lock().unwrap();
        *slot = None; // the old one stops listening when dropped
        if settings.door.enabled {
            *slot = door::Door::open(app.clone(), &settings.door).ok();
        }
    }
    if before.map_or(true, |old| old.hotkey != settings.hotkey) {
        register_hotkey(app, &settings.hotkey)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------------------------
// Commands

/// A request to the search helper: `{"op": "search", "q": "…"}` and the others it knows.
#[tauri::command]
fn open_site(rest: String) -> Result<(), String> {
    native::open_site(&rest).map_err(text)
}

#[tauri::command]
fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[tauri::command(async)]
fn ask(app: AppHandle, request: Value) -> Result<Value, String> {
    let slow = matches!(request["op"].as_str(), Some("example" | "duplicates" | "map" | "page" | "frame" | "tidy" | "albums" | "thread" | "collections" | "people" | "faces_enable" | "face_example" | "faces_of" | "face_name" | "ask_files" | "update_check" | "people_merge"));
    state(&app).serve.ask(request, Duration::from_secs(if slow { 180 } else { 60 })).map_err(text)
}

#[tauri::command(async)]
fn get_config() -> Config {
    Config::load()
}

#[tauri::command(async)]
fn set_config(app: AppHandle, settings: Config) -> Result<Config, String> {
    let before = Config::load();
    let mut settings = settings;
    if settings.door.enabled && settings.door.token.len() < 24 {
        settings.door.token = door::new_token();
    }
    // A shortcut Windows refuses isn't saved; the rest is.
    let hotkey = apply(&app, &settings, Some(&before));
    if hotkey.is_err() {
        settings.hotkey = before.hotkey.clone();
    }
    settings.save().map_err(text)?;
    let _ = state(&app).serve.ask(json!({"op": "config"}), Duration::from_secs(10));
    let _ = app.emit("config", &settings);
    let shared = state(&app);
    if settings.paused != before.paused {
        if let Some(task) = shared.index.lock().unwrap().as_ref() {
            task.tell(if settings.paused { "pause" } else { "resume" });
        }
        shared.index_state.lock().unwrap().paused = settings.paused;
    }
    if settings.roots != before.roots || settings.rules != before.rules || settings.ocr != before.ocr
        || (before.paused && !settings.paused) || (settings.setup_done && !before.setup_done)
    {
        start_index(&app, None);
    }
    publish(&app);
    hotkey.map(|_| settings)
}

#[tauri::command(async)]
fn status(app: AppHandle) -> Result<Value, String> {
    let mut value = helpers::once(&["status"]).map_err(text)?;
    value["index_state"] = serde_json::to_value(state(&app).index_state.lock().unwrap().clone()).unwrap_or_default();
    value["version"] = json!(env!("CARGO_PKG_VERSION"));
    Ok(value)
}

#[tauri::command(async)]
fn index_state(app: AppHandle) -> IndexState {
    state(&app).index_state.lock().unwrap().clone()
}

#[tauri::command(async)]
fn index_now(app: AppHandle) {
    start_index(&app, None);
}

/// Failed files get another go, then a full pass.
#[tauri::command(async)]
fn index_retry(app: AppHandle) -> Result<(), String> {
    let store = effodio_core::store::Store::open(&config::index_file()).map_err(text)?;
    store.retry_failed().map_err(text)?;
    start_index(&app, None);
    Ok(())
}

/// How many files a choice of folders holds and how long reading them takes, before anything is read.
#[tauri::command(async)]
fn estimate(settings: Config) -> Result<Value, String> {
    let folder = config::cache_folder();
    std::fs::create_dir_all(&folder).map_err(text)?;
    let file = folder.join(format!("estimate-{}.json", std::process::id()));
    settings.save_to(&file).map_err(text)?;
    let result = helpers::once(&["estimate", "--config", &file.to_string_lossy()]).map_err(text);
    let _ = std::fs::remove_file(&file);
    result
}

fn run_task(
    app: &AppHandle, slot: impl Fn(&App) -> &Mutex<Option<Task>> + Send + Sync + Copy + 'static, arguments: &[&str],
    channel: &'static str,
) -> Result<(), String> {
    let shared = state(app);
    if slot(&shared).lock().unwrap().is_some() {
        return Ok(());
    }
    let (events, ended) = (app.clone(), app.clone());
    let task = Task::start(
        arguments,
        &shared.job,
        move |event| {
            let _ = events.emit(channel, &event);
        },
        move || {
            {
                let shared = state(&ended);
                *slot(&shared).lock().unwrap() = None;
            }
            if channel == "model" && model_ready() {
                start_index(&ended, None);
            }
        },
    )
    .map_err(text)?;
    *slot(&shared).lock().unwrap() = Some(task);
    Ok(())
}

#[tauri::command(async)]
fn model_download(app: AppHandle) -> Result<(), String> {
    if model_ready() {
        let _ = app.emit("model", json!({"event": "finished"}));
        return Ok(());
    }
    run_task(&app, |shared| &shared.model, &["model"], "model")
}

#[tauri::command(async)]
fn speed_check(app: AppHandle) -> Result<(), String> {
    run_task(&app, |shared| &shared.speed, &["speed"], "speed")
}

#[tauri::command(async)]
fn pick_folders(app: AppHandle) -> Vec<String> {
    let shared = state(&app);
    shared.pinned.store(true, Ordering::Relaxed);
    let picked = app.dialog().file().blocking_pick_folders().unwrap_or_default();
    shared.pinned.store(false, Ordering::Relaxed);
    picked.into_iter().filter_map(|path| path.into_path().ok()).map(|path| path.to_string_lossy().into_owned()).collect()
}

/// A picture, a recording or a video to search by.
#[tauri::command(async)]
fn pick_example(app: AppHandle) -> Option<String> {
    let shared = state(&app);
    shared.pinned.store(true, Ordering::Relaxed);
    let picked = app
        .dialog()
        .file()
        .add_filter("Pictures, sound and video", &[
            "jpg", "jpeg", "png", "webp", "gif", "bmp", "heic", "mp3", "wav", "m4a", "flac", "ogg", "mp4", "mov",
            "mkv", "webm",
        ])
        .blocking_pick_file();
    shared.pinned.store(false, Ordering::Relaxed);
    picked.and_then(|path| path.into_path().ok()).map(|path| path.to_string_lossy().into_owned())
}

/// The folders most people keep their files in, with whether each is chosen already.
#[tauri::command(async)]
fn suggested_folders() -> Vec<Value> {
    let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) else { return Vec::new() };
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let bases = [home.clone(), home.join("OneDrive")];
    for name in ["Pictures", "Documents", "Downloads", "Desktop", "Videos", "Music"] {
        for base in &bases {
            let path = base.join(name);
            if path.is_dir() && seen.insert(path.clone()) {
                out.push(json!({"name": name, "path": path, "cloud": base != &home}));
            }
        }
    }
    out
}

/// Saves a picture pasted or dropped into the search box, so it can be searched by. Returns its path.
#[tauri::command(async)]
fn save_example(bytes: Vec<u8>, extension: String) -> Result<String, String> {
    let extension: String = extension.chars().filter(char::is_ascii_alphanumeric).take(5).collect();
    if bytes.is_empty() || bytes.len() > 64 << 20 || extension.is_empty() {
        return Err("that can't be searched by".into());
    }
    let folder = config::cache_folder().join("examples");
    std::fs::create_dir_all(&folder).map_err(text)?;
    // One at a time is kept.
    if let Ok(entries) = std::fs::read_dir(&folder) {
        for entry in entries.flatten() {
            let _ = std::fs::remove_file(entry.path());
        }
    }
    let file = folder.join(format!("example-{}.{}", &blake3::hash(&bytes).to_hex()[..12], extension.to_lowercase()));
    std::fs::write(&file, bytes).map_err(text)?;
    Ok(file.to_string_lossy().into_owned())
}

/// The languages Windows can read text in on this computer.
#[tauri::command(async)]
fn ocr_languages() -> Vec<String> {
    effodio_core::readers::winrt::ocr_languages()
}

#[tauri::command(async)]
fn thumbnail(path: String, side: Option<u32>) -> Result<String, String> {
    native::thumbnail(Path::new(&path), side.unwrap_or(256).clamp(32, 1024), &config::cache_folder().join("thumbs"))
        .map(|file| file.to_string_lossy().into_owned())
        .map_err(text)
}

#[tauri::command(async)]
fn open_result(app: AppHandle, path: String, page: Option<u32>, second: Option<f64>) -> Result<(), String> {
    native::open(Path::new(&path), page, second).map_err(text)?;
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
    Ok(())
}

#[tauri::command(async)]
fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    native::reveal(Path::new(&path)).map_err(text)?;
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
    Ok(())
}

#[tauri::command(async)]
fn copy_files(paths: Vec<String>) -> Result<(), String> {
    native::copy_files(&paths.into_iter().map(PathBuf::from).collect::<Vec<_>>()).map_err(text)
}

#[tauri::command(async)]
fn open_main(app: AppHandle, view: Option<String>, query: Option<String>) {
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
    show_main(&app, view.as_deref(), query.as_deref());
}

#[tauri::command(async)]
fn hide_panel(app: AppHandle) {
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
}

#[tauri::command(async)]
fn pin_panel(app: AppHandle, pinned: bool) {
    state(&app).pinned.store(pinned, Ordering::Relaxed);
}

/// Where the window was asked to open, once.
#[tauri::command(async)]
fn launch_target(app: AppHandle) -> Option<Value> {
    state(&app).launch.lock().unwrap().take()
}

#[tauri::command(async)]
fn quit(app: AppHandle) {
    shutdown(&app);
    app.exit(0);
}

fn shutdown(app: &AppHandle) {
    let shared = state(app);
    if let Some(task) = shared.index.lock().unwrap().as_ref() {
        task.tell("stop");
    }
    for slot in [&shared.model, &shared.speed] {
        if let Some(task) = slot.lock().unwrap().as_ref() {
            task.kill();
        }
    }
    shared.serve.stop();
}

// ---------------------------------------------------------------------------------------------------------------

fn tray(app: &AppHandle) -> tauri::Result<()> {
    let search = MenuItem::with_id(app, "search", "Search…", true, None::<&str>)?;
    let window = MenuItem::with_id(app, "window", "Open Effodio", true, None::<&str>)?;
    let pause = MenuItem::with_id(app, "pause", "Pause indexing", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Effodio", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[
        &search,
        &window,
        &PredefinedMenuItem::separator(app)?,
        &pause,
        &settings,
        &PredefinedMenuItem::separator(app)?,
        &quit,
    ])?;
    *state(app).pause_item.lock().unwrap() = Some(pause);
    let mut builder = TrayIconBuilder::with_id("main").menu(&menu).show_menu_on_left_click(false).tooltip("Effodio");
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder
        .on_menu_event(|app, event| match event.id.as_ref() {
            "search" => toggle_panel(app),
            "window" => show_main(app, None, None),
            "settings" => show_main(app, Some("settings"), None),
            "pause" => {
                let mut settings = Config::load();
                settings.paused = !settings.paused;
                let _ = set_config(app.clone(), settings);
            }
            "quit" => {
                shutdown(app);
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                toggle_panel(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// `effodio --search "dog on the beach"` opens the window on that search.
fn search_argument(arguments: &[String]) -> Option<String> {
    let at = arguments.iter().position(|argument| argument == "--search")?;
    arguments.get(at + 1).cloned()
}

fn main() {
    effodio_core::config::adopt_earlier_home();
    let wanted = search_argument(&std::env::args().collect::<Vec<_>>());
    let hidden = std::env::args().any(|argument| argument == "--hidden");
    let job = Arc::new(Job::new());
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, arguments, _| {
            if arguments.iter().any(|argument| argument == "--toggle") {
                toggle_panel(app);
            } else if arguments.iter().any(|argument| argument == "--settings") {
                show_main(app, Some("settings"), None);
            } else {
                show_main(app, None, search_argument(&arguments).as_deref());
            }
        }))
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _, event| {
                    if event.state() == ShortcutState::Pressed {
                        toggle_panel(app);
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .manage(App {
            serve: Serve::new(job.clone()),
            job,
            index: Mutex::new(None),
            index_state: Mutex::new(IndexState { phase: "idle".into(), ..Default::default() }),
            model: Mutex::new(None),
            speed: Mutex::new(None),
            dirty: Mutex::new((HashSet::new(), Instant::now())),
            again: AtomicBool::new(false),
            watcher: Mutex::new(None),
            pinned: AtomicBool::new(false),
            pause_item: Mutex::new(None),
            hotkey: Mutex::new(String::new()),
            door: Mutex::new(None),
            launch: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            ask, open_site, app_version, get_config, set_config, status, index_state, index_now, index_retry, estimate, model_download,
            speed_check, pick_folders, pick_example, suggested_folders, save_example, thumbnail, open_result, reveal,
            copy_files, open_main, hide_panel, pin_panel, quit, ocr_languages, launch_target,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            std::fs::create_dir_all(config::cache_folder()).ok();
            tray(&handle)?;
            build_panel(&handle)?;
            let settings = Config::load();
            // A shortcut that's taken mustn't stop the app from starting: the tray icon opens the panel too.
            let _ = apply(&handle, &settings, None);
            watch_loop(handle.clone());
            if !settings.setup_done {
                show_main(&handle, Some("setup"), None);
            } else {
                if !hidden {
                    show_main(&handle, None, wanted.as_deref());
                }
                start_index(&handle, None);
            }
            Ok(())
        })
        .on_window_event(|window, event| match event {
            // The panel goes away when you click elsewhere, as a menu does.
            WindowEvent::Focused(false) if window.label() == "panel" => {
                if !state(window.app_handle()).pinned.load(Ordering::Relaxed) {
                    let _ = window.hide();
                }
            }
            // Closing the window leaves Effodio in the tray.
            WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
                api.prevent_close();
                let _ = window.hide();
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("Effodio couldn't start");
}
