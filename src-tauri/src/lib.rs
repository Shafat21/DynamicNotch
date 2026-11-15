mod actions;
mod bridge;
mod commands;
mod db;
#[cfg(windows)]
mod glass_spike; // SPIKE: remove
mod launcher;
mod menu;
mod monitors;
mod window;

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

pub struct AppState {
    pub db: Mutex<db::Db>,
    /// Last payload of each "sticky" event, so a freshly loaded page can catch up (see `get_snapshot`).
    pub last: Mutex<HashMap<&'static str, Value>>,
    /// Interactive area of the island, in logical px relative to the window.
    pub hit: Mutex<window::Rect>,
}

/// Emit an event to the frontend and remember it for `get_snapshot`.
pub fn publish<T: Serialize>(app: &AppHandle, event: &'static str, payload: T) {
    let value = serde_json::to_value(payload).unwrap_or_default();
    app.state::<AppState>().last.lock().unwrap().insert(event, value.clone());
    let _ = app.emit(event, value);
}

pub fn current_settings(app: &AppHandle) -> Result<db::Settings, String> {
    let mut s = app.state::<AppState>().db.lock().unwrap().settings().map_err(|e| e.to_string())?;
    s.autostart = app.autolaunch().is_enabled().unwrap_or(false);
    Ok(s)
}

pub fn emit_settings(app: &AppHandle) {
    if let Ok(s) = current_settings(app) {
        let _ = app.emit("settings", s);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("dynamic_notch.db"))?;
            app.manage(AppState {
                db: Mutex::new(db),
                last: Mutex::new(HashMap::new()),
                hit: Mutex::new(window::Rect::default()),
            });

            let win = app.get_webview_window("main").expect("main window");
            window::setup(app.handle(), &win)?;
            #[cfg(windows)]
            glass_spike::run(&win); // SPIKE: remove

            let handle = app.handle().clone();
            app.on_menu_event(move |_, event| menu::handle(&handle, event.id().as_ref()));

            monitors::spawn_all(app.handle().clone());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::get_snapshot,
            commands::list_tasks,
            commands::add_task,
            commands::delete_task,
            commands::set_location,
            commands::media_control,
            commands::run_action,
            commands::list_apps,
            commands::pick_app,
            commands::remove_app,
            commands::launch_app,
            commands::set_hit_rect,
            commands::show_context_menu,
        ])
        .run(tauri::generate_context!())
        .expect("error while running DynamicNotch");
}
