//! Everything the frontend can call via `invoke(...)`. Inputs from the webview are validated here.

use crate::monitors::calendar;
use crate::{actions, db, emit_settings, launcher, menu, monitors, window, AppState};
use serde_json::Value;
use std::collections::HashMap;
use tauri::{AppHandle, Manager, State, WebviewWindow};
use tauri_plugin_dialog::DialogExt;

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Res<db::Settings> {
    crate::current_settings(&app)
}

/// Latest perf/media/lyric/weather/accent payloads, for when the page (re)loads after monitors started.
#[tauri::command]
pub fn get_snapshot(state: State<AppState>) -> HashMap<&'static str, Value> {
    state.last.lock().unwrap().clone()
}

#[tauri::command]
pub fn list_tasks(state: State<AppState>) -> Res<Vec<db::Task>> {
    state.db.lock().unwrap().tasks().map_err(err)
}

#[tauri::command]
pub fn add_task(state: State<AppState>, name: String, category: String, time: String) -> Res<Vec<db::Task>> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 100 || time.chars().count() > 30 {
        return Err("Task name must be 1-100 characters, time at most 30".into());
    }
    let time = if time.trim().is_empty() { "N/A" } else { time.trim() };
    let db = state.db.lock().unwrap();
    db.add_task(name, &category, time).map_err(err)?;
    db.tasks().map_err(err)
}

#[tauri::command]
pub fn delete_task(state: State<AppState>, id: i64) -> Res<Vec<db::Task>> {
    let db = state.db.lock().unwrap();
    db.delete_task(id).map_err(err)?;
    db.tasks().map_err(err)
}

/// Geocode a city name, store it, refresh the weather. Returns the resolved "City, Country".
#[tauri::command]
pub async fn set_location(app: AppHandle, city: String) -> Res<String> {
    let city = city.trim().to_string();
    if city.is_empty() || city.chars().count() > 100 {
        return Err("Enter a city name".into());
    }
    let (name, lat, lon) = tauri::async_runtime::spawn_blocking(move || monitors::weather::geocode(&city))
        .await
        .map_err(err)??;
    {
        use tauri::Manager;
        app.state::<AppState>().db.lock().unwrap().set_location(&name, lat, lon).map_err(err)?;
    }
    monitors::weather::refresh();
    emit_settings(&app);
    Ok(name)
}

/// Store the calendar link after one successful fetch ("" removes it); returns the new `calendar` payload.
/// A failed fetch leaves the stored link unchanged. The link never goes back to the webview.
#[tauri::command]
pub async fn set_calendar_url(app: AppHandle, url: String) -> Res<Value> {
    let url = calendar::normalize_url(&url)?;
    let started = calendar::begin(); // supersedes older saves and the monitor's in-flight fetch
    let Some(url) = url else {
        return calendar::commit(&app, started, false, Some(Vec::new()), None, Some(""));
    };
    let fetch_url = url.clone();
    let (events, note) = tauri::async_runtime::spawn_blocking(move || calendar::load(&fetch_url)).await.map_err(err)??;
    calendar::commit(&app, started, true, Some(events), note, Some(&url))
}

/// Open the join link of event `id` from the last published `calendar` payload (no URL from the webview).
#[tauri::command]
pub fn open_join(state: State<AppState>, id: String) -> Res<()> {
    let url = {
        let last = state.last.lock().unwrap();
        let p = last.get("calendar").ok_or("No calendar loaded")?;
        let mut events = p["today"].as_array().into_iter().flatten().chain([&p["tomorrowFirst"]]);
        events.find(|e| e["id"] == id.as_str()).and_then(|e| e["join"].as_str()).ok_or("No join link for this event")?.to_string()
    };
    launcher::os::shell_open(&url)
}

#[tauri::command]
pub async fn media_control(action: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || monitors::media::control(&action)).await.map_err(err)?
}

/// Seek the current track; bounds come from the last published `media-progress` duration.
#[tauri::command]
pub async fn media_seek(app: AppHandle, seconds: f64) -> Res<()> {
    let state = app.state::<AppState>();
    let duration = state.last.lock().unwrap().get("media-progress").and_then(|p| p["duration"].as_f64()).unwrap_or(0.0);
    let seconds = monitors::media::validate_seek(seconds, duration)?;
    tauri::async_runtime::spawn_blocking(move || monitors::media::seek(seconds)).await.map_err(err)??;
    crate::publish(&app, "media-progress", serde_json::json!({ "position": seconds, "duration": duration }));
    Ok(())
}

#[tauri::command]
pub async fn media_open_player() -> Res<()> {
    tauri::async_runtime::spawn_blocking(monitors::media::open_player).await.map_err(err)?
}

#[tauri::command]
pub fn run_action(id: String) -> Res<()> {
    actions::run(&id)
}

#[tauri::command]
pub fn set_hit_rect(app: AppHandle, rect: window::Rect) {
    window::set_hit_rect(&app, rect);
}

#[tauri::command]
pub fn show_context_menu(app: AppHandle, window: WebviewWindow) -> Res<()> {
    menu::popup(&app, &window).map_err(err)
}

#[tauri::command]
pub fn list_apps(state: State<AppState>) -> Res<Vec<db::App>> {
    state.db.lock().unwrap().apps().map_err(err)
}

/// Native file dialog, opened from Rust so the webview needs no dialog permission. Cancel returns the list unchanged.
#[tauri::command]
pub async fn pick_app(app: AppHandle) -> Res<Vec<db::App>> {
    let handle = app.clone();
    let picked = tauri::async_runtime::spawn_blocking(move || {
        handle.dialog().file().add_filter("Apps", &["exe", "lnk"]).blocking_pick_file()
    })
    .await
    .map_err(err)?;
    let state = app.state::<AppState>();
    if let Some(path) = picked.and_then(|p| p.into_path().ok()) {
        let name = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let icon = tauri::async_runtime::spawn_blocking({
            let path = path.clone();
            move || launcher::icon_data_uri(&path)
        })
        .await
        .ok()
        .flatten();
        state.db.lock().unwrap().add_app(&name, "path", &path.to_string_lossy(), icon.as_deref()).map_err(err)?;
    }
    let apps = state.db.lock().unwrap().apps().map_err(err);
    apps
}

#[tauri::command]
pub fn remove_app(state: State<AppState>, id: i64) -> Res<Vec<db::App>> {
    let db = state.db.lock().unwrap();
    db.remove_app(id).map_err(err)?;
    db.apps().map_err(err)
}

#[tauri::command]
pub fn launch_app(state: State<AppState>, id: i64) -> Res<()> {
    let app = state.db.lock().unwrap().app(id).map_err(err)?.ok_or("unknown app")?;
    launcher::launch(&app)
}
