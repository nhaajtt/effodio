//! What the interface can ask of the app. Every path it sends goes through `permitted` first.

use super::*;

/// A page of effodio.nhaajt.com, in the browser.
#[tauri::command]
pub fn open_site(rest: String) -> Result<(), String> {
    native::open_site(&rest).map_err(text)
}

#[tauri::command]
pub fn app_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// A request to the search helper: `{"op": "search", "q": "…"}` and the others it knows.
#[tauri::command(async)]
pub fn ask(app: AppHandle, request: Value) -> Result<Value, String> {
    let slow = matches!(
        request["op"].as_str(),
        Some(
            "example"
                | "duplicates"
                | "map"
                | "page"
                | "frame"
                | "tidy"
                | "albums"
                | "thread"
                | "collections"
                | "people"
                | "faces_enable"
                | "face_example"
                | "faces_of"
                | "face_name"
                | "ask_files"
                | "update_check"
                | "people_merge"
        )
    );
    // Searching by a picture, or showing a page or a frame, reads the file named: only one the page may use.
    if let Some(path) = request["path"].as_str() {
        permitted(&app, path)?;
    }
    state(&app).serve.ask(request, Duration::from_secs(if slow { 180 } else { 60 })).map_err(text)
}

#[tauri::command(async)]
pub fn get_config() -> Config {
    Config::load()
}

#[tauri::command(async)]
pub fn set_config(app: AppHandle, settings: Config) -> Result<Config, String> {
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
    if settings.roots != before.roots
        || settings.rules != before.rules
        || settings.ocr != before.ocr
        || (before.paused && !settings.paused)
        || (settings.setup_done && !before.setup_done)
    {
        start_index(&app, None);
    }
    publish(&app);
    hotkey.map(|_| settings)
}

#[tauri::command(async)]
pub fn status(app: AppHandle) -> Result<Value, String> {
    let mut value = helpers::once(&["status"]).map_err(text)?;
    value["index_state"] = serde_json::to_value(state(&app).index_state.lock().unwrap().clone()).unwrap_or_default();
    value["version"] = json!(env!("CARGO_PKG_VERSION"));
    Ok(value)
}

#[tauri::command(async)]
pub fn index_state(app: AppHandle) -> IndexState {
    state(&app).index_state.lock().unwrap().clone()
}

#[tauri::command(async)]
pub fn index_now(app: AppHandle) {
    start_index(&app, None);
}

/// Failed files get another go, then a full pass.
#[tauri::command(async)]
pub fn index_retry(app: AppHandle) -> Result<(), String> {
    let store = effodio_core::store::Store::open(&config::index_file()).map_err(text)?;
    store.retry_failed().map_err(text)?;
    start_index(&app, None);
    Ok(())
}

/// How many files a choice of folders holds and how long reading them takes, before anything is read.
#[tauri::command(async)]
pub fn estimate(settings: Config) -> Result<Value, String> {
    let folder = config::cache_folder();
    std::fs::create_dir_all(&folder).map_err(text)?;
    let file = folder.join(format!("estimate-{}.json", std::process::id()));
    settings.save_to(&file).map_err(text)?;
    let result = helpers::once(&["estimate", "--config", &file.to_string_lossy()]).map_err(text);
    let _ = std::fs::remove_file(&file);
    result
}

pub(crate) fn run_task(
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
pub fn model_download(app: AppHandle) -> Result<(), String> {
    if model_ready() {
        let _ = app.emit("model", json!({"event": "finished"}));
        return Ok(());
    }
    run_task(&app, |shared| &shared.model, &["model"], "model")
}

#[tauri::command(async)]
pub fn speed_check(app: AppHandle) -> Result<(), String> {
    run_task(&app, |shared| &shared.speed, &["speed"], "speed")
}

#[tauri::command(async)]
pub fn pick_folders(app: AppHandle) -> Vec<String> {
    let shared = state(&app);
    shared.pinned.store(true, Ordering::Relaxed);
    let picked = app.dialog().file().blocking_pick_folders().unwrap_or_default();
    shared.pinned.store(false, Ordering::Relaxed);
    picked
        .into_iter()
        .filter_map(|path| path.into_path().ok())
        .map(|path| path.to_string_lossy().into_owned())
        .collect()
}

/// A picture, a recording or a video to search by.
#[tauri::command(async)]
pub fn pick_example(app: AppHandle) -> Option<String> {
    let shared = state(&app);
    shared.pinned.store(true, Ordering::Relaxed);
    let picked = app
        .dialog()
        .file()
        .add_filter(
            "Pictures, sound and video",
            &[
                "jpg", "jpeg", "png", "webp", "gif", "bmp", "heic", "mp3", "wav", "m4a", "flac", "ogg", "mp4", "mov",
                "mkv", "webm",
            ],
        )
        .blocking_pick_file();
    shared.pinned.store(false, Ordering::Relaxed);
    let picked = picked.and_then(|path| path.into_path().ok())?;
    shared.given.add(&picked);
    Some(picked.to_string_lossy().into_owned())
}

/// The folders most people keep their files in, with whether each is chosen already.
#[tauri::command(async)]
pub fn suggested_folders() -> Vec<Value> {
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
pub fn save_example(bytes: Vec<u8>, extension: String) -> Result<String, String> {
    let extension: String =
        extension.chars().filter(char::is_ascii_alphanumeric).take(5).collect::<String>().to_lowercase();
    // Only what can be searched by: a picture, a recording or a video, never a program.
    let searchable = matches!(
        effodio_core::kinds::kind_of(Path::new(&format!("example.{extension}"))),
        Some(FileKind::Image | FileKind::Screenshot | FileKind::Audio | FileKind::Video)
    );
    if bytes.is_empty() || bytes.len() > 64 << 20 || !searchable {
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
pub fn ocr_languages() -> Vec<String> {
    effodio_core::readers::winrt::ocr_languages()
}

#[tauri::command(async)]
pub fn thumbnail(app: AppHandle, path: String, side: Option<u32>) -> Result<String, String> {
    let path = permitted(&app, &path)?;
    native::thumbnail(&path, side.unwrap_or(256).clamp(32, 1024), &config::cache_folder().join("thumbs"))
        .map(|file| file.to_string_lossy().into_owned())
        .map_err(text)
}

#[tauri::command(async)]
pub fn open_result(app: AppHandle, path: String, page: Option<u32>, second: Option<f64>) -> Result<(), String> {
    let path = permitted(&app, &path)?;
    // Only the kinds of file Effodio reads are opened: never a program, a script or a shortcut.
    if effodio_core::kinds::kind_of(&path).is_none() || path.is_dir() {
        return Err("Effodio only opens the kinds of file it searches".into());
    }
    native::open(&path, page, second).map_err(text)?;
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
    Ok(())
}

#[tauri::command(async)]
pub fn reveal(app: AppHandle, path: String) -> Result<(), String> {
    let path = permitted(&app, &path)?;
    native::reveal(&path).map_err(text)?;
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
    Ok(())
}

#[tauri::command(async)]
pub fn copy_files(app: AppHandle, paths: Vec<String>) -> Result<(), String> {
    let paths = paths.iter().map(|path| permitted(&app, path)).collect::<Result<Vec<_>, _>>()?;
    native::copy_files(&paths).map_err(text)
}

#[tauri::command(async)]
pub fn open_main(app: AppHandle, view: Option<String>, query: Option<String>) {
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
    show_main(&app, view.as_deref(), query.as_deref());
}

#[tauri::command(async)]
pub fn hide_panel(app: AppHandle) {
    if let Some(window) = panel(&app) {
        let _ = window.hide();
    }
}

#[tauri::command(async)]
pub fn pin_panel(app: AppHandle, pinned: bool) {
    state(&app).pinned.store(pinned, Ordering::Relaxed);
}

/// Where the window was asked to open, once.
#[tauri::command(async)]
pub fn launch_target(app: AppHandle) -> Option<Value> {
    state(&app).launch.lock().unwrap().take()
}

#[tauri::command(async)]
pub fn quit(app: AppHandle) {
    shutdown(&app);
    app.exit(0);
}
