//! Indexing runs: starting the helper on the chosen folders, reporting its progress to the windows, and the folder
//! watcher that reads changed files as they change.

use super::*;

pub(crate) fn publish(app: &AppHandle) {
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

pub(crate) fn on_index_event(app: &AppHandle, event: &Value) {
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
pub(crate) fn start_index(app: &AppHandle, paths: Option<Vec<PathBuf>>) {
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
pub(crate) fn watch(app: &AppHandle) {
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

pub(crate) fn watch_loop(app: AppHandle) {
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
