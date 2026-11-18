//! The two windows (the quick search panel and the main window), the global shortcut, and applying saved settings.

use super::*;

pub(crate) fn panel(app: &AppHandle) -> Option<WebviewWindow> {
    app.get_webview_window("panel")
}

pub(crate) fn build_panel(app: &AppHandle) -> tauri::Result<WebviewWindow> {
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

pub(crate) fn build_main(app: &AppHandle) -> tauri::Result<WebviewWindow> {
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

pub(crate) fn toggle_panel(app: &AppHandle) {
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

pub(crate) fn show_main(app: &AppHandle, view: Option<&str>, query: Option<&str>) {
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

pub(crate) fn register_hotkey(app: &AppHandle, wanted: &str) -> Result<(), String> {
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
pub(crate) fn apply(app: &AppHandle, settings: &Config, before: Option<&Config>) -> Result<(), String> {
    let scope = app.asset_protocol_scope();
    let _ = scope.allow_directory(config::cache_folder(), true);
    for root in &settings.roots {
        let _ = scope.allow_directory(&root.path, true);
    }
    if before.is_none_or(|old| old.autostart != settings.autostart) {
        let auto = app.autolaunch();
        let _ = if settings.autostart { auto.enable() } else { auto.disable() };
    }
    if before.is_none_or(|old| old.roots != settings.roots) {
        watch(app);
    }
    if before.is_none_or(|old| old.door != settings.door) {
        let shared = state(app);
        let mut slot = shared.door.lock().unwrap();
        *slot = None; // the old one stops listening when dropped
        if settings.door.enabled {
            *slot = door::Door::open(app.clone(), &settings.door).ok();
        }
    }
    if before.is_none_or(|old| old.hotkey != settings.hotkey) {
        register_hotkey(app, &settings.hotkey)?;
    }
    Ok(())
}
