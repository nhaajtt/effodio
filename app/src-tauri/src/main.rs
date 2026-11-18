//! Effodio for Windows: the tray icon, the hotkey, the search panel and the window, over Effodio's helper.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod door;
mod helpers;
mod indexing;
mod native;
mod paths;
mod windows;

use commands::*;
use effodio_core::config::{self, Config};
use effodio_core::kinds::FileKind;
use helpers::{Serve, Task};
use indexing::*;
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
use windows::*;

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
    /// Files the person handed over by the file dialog or a drop, which the page may then use.
    given: paths::Given,
}

fn state(app: &AppHandle) -> tauri::State<'_, App> {
    app.state::<App>()
}

/// `path` from the page, if it's one the page may use: in a chosen folder, in the app's own folder, or handed over by
/// the person (see `paths`).
fn permitted(app: &AppHandle, path: &str) -> Result<PathBuf, String> {
    let mut folders: Vec<PathBuf> = Config::load().roots.into_iter().map(|root| root.path).collect();
    folders.push(config::home());
    paths::check(path, &folders, &state(app).given)
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
    let menu = Menu::with_items(
        app,
        &[
            &search,
            &window,
            &PredefinedMenuItem::separator(app)?,
            &pause,
            &settings,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
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
                // Only from settings that could be read: saving the defaults back would lose the chosen folders.
                if let Ok(mut settings) = Config::read() {
                    settings.paused = !settings.paused;
                    let _ = set_config(app.clone(), settings);
                }
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
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
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
            given: paths::Given::default(),
        })
        .invoke_handler(tauri::generate_handler![
            commands::ask,
            commands::open_site,
            commands::app_version,
            commands::get_config,
            commands::set_config,
            commands::status,
            commands::index_state,
            commands::index_now,
            commands::index_retry,
            commands::estimate,
            commands::model_download,
            commands::speed_check,
            commands::pick_folders,
            commands::pick_example,
            commands::suggested_folders,
            commands::save_example,
            commands::thumbnail,
            commands::open_result,
            commands::reveal,
            commands::copy_files,
            commands::open_main,
            commands::hide_panel,
            commands::pin_panel,
            commands::quit,
            commands::ocr_languages,
            commands::launch_target,
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
            // Files dropped on a window are the person's to search by, wherever they are.
            WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) => {
                let shared = state(window.app_handle());
                for path in paths {
                    shared.given.add(path);
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
