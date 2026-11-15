//! Everything the frontend can call via `invoke(...)`. Inputs from the webview are validated here.

use crate::monitors::calendar;
use crate::{actions, appearance, bridge, db, display, emit_settings, fullscreen, launcher, menu, monitors, window, AppState};
use serde_json::{json, Value};
use std::collections::HashMap;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::DialogExt;

type Res<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Res<db::Settings> {
    crate::current_settings(&app)
}

/// Focus the Settings window, or create it (single instance).
pub fn show_settings(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("settings") {
        let _ = w.unminimize();
        return w.set_focus();
    }
    tauri::WebviewWindowBuilder::new(app, "settings", tauri::WebviewUrl::App("settings.html".into()))
        .title("Dynamic Notch Island Settings")
        .inner_size(900.0, 640.0)
        .min_inner_size(760.0, 520.0)
        .decorations(true)
        .resizable(true)
        .build()?;
    Ok(())
}

/// async: building a webview inside a sync (main-thread) command deadlocks on Windows (wry#583).
#[tauri::command]
pub async fn open_settings(app: AppHandle) -> Res<()> {
    show_settings(&app).map_err(err)
}

/// The General settings the Settings window may change (allow-list; values checked by the setters).
#[derive(Debug, PartialEq)]
pub enum General {
    Display(String),
    IslandSize(String),
    HideFullscreen(bool),
    Autostart(bool),
}

pub fn parse_general(key: &str, value: &Value) -> Res<General> {
    match (key, value) {
        ("display", Value::String(s)) => Ok(General::Display(s.clone())),
        ("island_size", Value::String(s)) => Ok(General::IslandSize(s.clone())),
        ("hide_fullscreen", Value::Bool(b)) => Ok(General::HideFullscreen(*b)),
        ("autostart", Value::Bool(b)) => Ok(General::Autostart(*b)),
        ("display" | "island_size" | "hide_fullscreen" | "autostart", _) => Err(format!("invalid value for {key}")),
        _ => Err(format!("unknown setting {key}")),
    }
}

/// Apply one General setting through the same functions the menu used; returns the new settings.
#[tauri::command]
pub fn set_general(app: AppHandle, key: String, value: Value) -> Res<db::Settings> {
    match parse_general(&key, &value)? {
        General::Display(name) => display::set_display(&app, &name),
        General::IslandSize(size) => display::set_island_size(&app, &size),
        General::HideFullscreen(on) => fullscreen::set_hide_fullscreen(&app, on),
        General::Autostart(on) => {
            let al = app.autolaunch();
            if on { al.enable() } else { al.disable() }.map_err(err)
        }
    }?;
    emit_settings(&app);
    crate::current_settings(&app)
}

/// Plugged-in monitors for the Display dropdown: `name` is what `set_general("display")` takes.
#[tauri::command]
pub fn list_displays(app: AppHandle) -> Res<Vec<Value>> {
    let infos = display::monitors(&app).map_err(err)?.1;
    Ok(infos
        .iter()
        .enumerate()
        .map(|(i, m)| json!({ "name": m.name, "label": format!("{}: {} ({}×{})", i + 1, m.name, m.width, m.height) }))
        .collect())
}

/// About-page links by id (the webview never sends a URL).
#[tauri::command]
pub fn open_link(id: String) -> Res<()> {
    let url = match id.as_str() {
        "repo" => "https://github.com/Shafat21/DynamicNotch",
        "issues" => "https://github.com/Shafat21/DynamicNotch/issues",
        _ => return Err(format!("unknown link {id}")),
    };
    launcher::os::shell_open(url)
}

/// Run `f` on the stored appearance + layout, persist the result and publish it (sticky `appearance`).
fn update_appearance(
    app: &AppHandle,
    f: impl FnOnce(appearance::Appearance, appearance::Layout) -> (appearance::Appearance, appearance::Layout),
) -> Res<Value> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let (a, l) = appearance::load(&db);
    let (a, l) = f(a, l);
    let (a, l) = (appearance::sanitize(a), appearance::normalize_layout(l));
    appearance::save(&db, &a, &l)?;
    drop(db);
    let payload = appearance::payload(&a, &l);
    crate::publish(app, "appearance", payload.clone());
    Ok(payload)
}

#[tauri::command]
pub fn get_appearance(state: State<AppState>) -> Value {
    let (a, l) = appearance::load(&state.db.lock().unwrap());
    appearance::payload(&a, &l)
}

#[tauri::command]
pub fn set_appearance(app: AppHandle, patch: Value) -> Res<Value> {
    update_appearance(&app, |a, l| (appearance::merge_patch(&a, &patch), l))
}

#[tauri::command]
pub fn apply_theme(app: AppHandle, name: String) -> Res<Value> {
    if !appearance::THEMES.iter().any(|t| t.name == name) {
        return Err(format!("unknown theme: {name}"));
    }
    update_appearance(&app, |a, l| (appearance::Appearance { theme: name, accent: String::new(), ..a }, l))
}

#[tauri::command]
pub fn reset_appearance(app: AppHandle) -> Res<Value> {
    update_appearance(&app, |_, l| (appearance::Appearance::default(), l))
}

#[tauri::command]
pub fn set_layout(app: AppHandle, layout: appearance::Layout) -> Res<Value> {
    update_appearance(&app, |a, _| (a, layout))
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
    let apps = state.db.lock().unwrap().apps().map_err(err)?;
    let _ = app.emit("apps", &apps); // island and Settings window stay in sync
    Ok(apps)
}

#[tauri::command]
pub fn remove_app(app: AppHandle, id: i64) -> Res<Vec<db::App>> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    db.remove_app(id).map_err(err)?;
    let apps = db.apps().map_err(err)?;
    let _ = app.emit("apps", &apps);
    Ok(apps)
}

#[tauri::command]
pub fn launch_app(state: State<AppState>, id: i64) -> Res<()> {
    let app = state.db.lock().unwrap().app(id).map_err(err)?.ok_or("unknown app")?;
    launcher::launch(&app)
}

/// Answer for a pending prompt; validated against the prompt's kind and questions before the hook sees it.
#[tauri::command]
pub fn answer_prompt(state: State<AppState>, id: u64, answer: bridge::prompt::Answer) -> Res<()> {
    state.prompts.answer(id, answer)
}

/// Native confirm, then merge our hooks into ~/.claude/settings.json (with backup).
#[tauri::command]
pub async fn connect_claude(app: AppHandle) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || bridge::set_connected(&app, true)).await.map_err(err)?
}

#[tauri::command]
pub async fn disconnect_claude(app: AppHandle) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || bridge::set_connected(&app, false)).await.map_err(err)?
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn general_keys_are_allow_listed_and_typed() {
        assert_eq!(parse_general("display", &json!("primary")), Ok(General::Display("primary".into())));
        assert_eq!(parse_general("island_size", &json!("Large")), Ok(General::IslandSize("Large".into())));
        assert_eq!(parse_general("hide_fullscreen", &json!(false)), Ok(General::HideFullscreen(false)));
        assert_eq!(parse_general("autostart", &json!(true)), Ok(General::Autostart(true)));
        assert!(parse_general("autostart", &json!("yes")).is_err()); // wrong type
        assert!(parse_general("display", &json!(1)).is_err());
        assert!(parse_general("bridge_token", &json!("x")).is_err()); // not allow-listed
        assert!(parse_general("", &json!(null)).is_err());
    }
}
