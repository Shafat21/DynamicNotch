//! Everything the frontend can call via `invoke(...)`. Inputs from the webview are validated here.

use crate::monitors::calendar;
use crate::{actions, bluetooth, clipboard, appearance, bridge, brightness, db, display, emit_settings, fullscreen, launcher, menu, monitors, notes, reminders, shelf, sound, window, AppState};
use crate::brightness::BrightDisplay;
use crate::sound::{AudioDevice, SoundState};
use crate::clipboard::Clip;
use crate::notes::Note;
use crate::shelf::ShelfItem;
use crate::reminders::{Reminder, Repeat};
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
    ReminderSound(bool),
    ClipRemember(bool),
}

pub fn parse_general(key: &str, value: &Value) -> Res<General> {
    match (key, value) {
        ("display", Value::String(s)) => Ok(General::Display(s.clone())),
        ("island_size", Value::String(s)) => Ok(General::IslandSize(s.clone())),
        ("hide_fullscreen", Value::Bool(b)) => Ok(General::HideFullscreen(*b)),
        ("autostart", Value::Bool(b)) => Ok(General::Autostart(*b)),
        ("reminder_sound", Value::Bool(b)) => Ok(General::ReminderSound(*b)),
        ("clip_remember", Value::Bool(b)) => Ok(General::ClipRemember(*b)),
        ("display" | "island_size" | "hide_fullscreen" | "autostart" | "reminder_sound" | "clip_remember", _) => Err(format!("invalid value for {key}")),
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
        General::ReminderSound(on) => app.state::<AppState>().db.lock().unwrap().set("reminder_sound", &on.to_string()).map_err(err),
        General::ClipRemember(on) => {
            let state = app.state::<AppState>();
            let db = state.db.lock().unwrap();
            db.set("clip_remember", &on.to_string()).map_err(err)?;
            clipboard::persist(&db, &state.clips.lock().unwrap(), on); // off wipes what was stored
            Ok(())
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
        // `name` (\\.\DISPLAY1) is the stored id; people see "Display 1" + resolution
        .map(|(i, m)| json!({ "name": m.name, "label": format!("Display {}", i + 1), "width": m.width, "height": m.height, "primary": m.primary }))
        .collect())
}

/// About-page links by id (the webview never sends a URL).
#[tauri::command]
pub fn open_link(id: String) -> Res<()> {
    let url = match id.as_str() {
        "repo" => "https://github.com/Shafat21/DynamicNotch",
        "issues" => "https://github.com/Shafat21/DynamicNotch/issues",
        "releases" => "https://github.com/Shafat21/DynamicNotch/releases",
        "site" => "https://shafat21.online",
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

/// Notes: each command returns the (filtered) list so the UI re-renders from one reply.
#[tauri::command]
pub fn list_notes(state: State<AppState>, query: String) -> Res<Vec<Note>> {
    state.db.lock().unwrap().notes(&query).map_err(err)
}

#[tauri::command]
pub fn add_note(state: State<AppState>, body: String) -> Res<Vec<Note>> {
    let db = state.db.lock().unwrap();
    db.add_note(notes::clean_body(&body)?).map_err(err)?;
    db.notes("").map_err(err)
}

#[tauri::command]
pub fn update_note(state: State<AppState>, id: i64, body: String) -> Res<Vec<Note>> {
    let db = state.db.lock().unwrap();
    db.update_note(id, notes::clean_body(&body)?).map_err(err)?;
    db.notes("").map_err(err)
}

#[tauri::command]
pub fn pin_note(state: State<AppState>, id: i64, pinned: bool) -> Res<Vec<Note>> {
    let db = state.db.lock().unwrap();
    db.pin_note(id, pinned).map_err(err)?;
    db.notes("").map_err(err)
}

#[tauri::command]
pub fn delete_note(state: State<AppState>, id: i64) -> Res<Vec<Note>> {
    let db = state.db.lock().unwrap();
    db.delete_note(id).map_err(err)?;
    db.notes("").map_err(err)
}

/// Reminders: each command returns the upcoming + done-today list.
fn reminder_list(db: &db::Db) -> Res<Vec<Reminder>> {
    db.reminders(reminders::today_start()).map_err(err)
}

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

#[tauri::command]
pub fn list_reminders(state: State<AppState>) -> Res<Vec<Reminder>> {
    reminder_list(&state.db.lock().unwrap())
}

/// Due time of a New-reminder chip (`10m|30m|1h|evening|tomorrow`), in local time.
#[tauri::command]
pub fn quick_due(chip: String) -> Res<i64> {
    reminders::quick_due(&chip, chrono::Local::now()).ok_or_else(|| format!("unknown chip {chip}"))
}

#[tauri::command]
pub fn add_reminder(state: State<AppState>, text: String, due_at: i64, repeat: Repeat) -> Res<Vec<Reminder>> {
    let text = reminders::clean_text(&text)?;
    if due_at < now_ms() - 60_000 {
        return Err("Pick a time in the future".into());
    }
    let db = state.db.lock().unwrap();
    db.add_reminder(text, due_at, repeat).map_err(err)?;
    reminder_list(&db)
}

#[tauri::command]
pub fn snooze_reminder(state: State<AppState>, id: i64) -> Res<Vec<Reminder>> {
    let db = state.db.lock().unwrap();
    db.snooze_reminder(id, reminders::snoozed(now_ms())).map_err(err)?;
    reminder_list(&db)
}

/// A repeat rolls to its next occurrence after now (or after this one, when pressed early); a one-off is done.
#[tauri::command]
pub fn done_reminder(state: State<AppState>, id: i64) -> Res<Vec<Reminder>> {
    let db = state.db.lock().unwrap();
    let r = db.reminder(id).map_err(err)?.ok_or("unknown reminder")?;
    let now = now_ms();
    db.finish_reminder(id, reminders::next_due(r.due_at, r.repeat, now.max(r.due_at), &chrono::Local), now).map_err(err)?;
    reminder_list(&db)
}

#[tauri::command]
pub fn delete_reminder(state: State<AppState>, id: i64) -> Res<Vec<Reminder>> {
    let db = state.db.lock().unwrap();
    db.delete_reminder(id).map_err(err)?;
    reminder_list(&db)
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
    let desktop = calendar::desktop_on(&app);
    let Some(url) = url else {
        // link removed: only the desktop calendar (if on) is left
        let (events, note) = if desktop { blocking(|| calendar::load(None, true)).await.unwrap_or_default() } else { Default::default() };
        return calendar::commit(&app, started, desktop, Some(events), note, Some(""));
    };
    if desktop {
        // a link that doesn't load must not be saved just because the desktop calendar does
        let probe = url.clone();
        blocking(move || calendar::load(Some(&probe), false)).await?;
    }
    let fetch_url = url.clone();
    let (events, note) = blocking(move || calendar::load(Some(&fetch_url), desktop)).await?;
    calendar::commit(&app, started, true, Some(events), note, Some(&url))
}

/// Turn the desktop calendar (the OS calendar app's accounts) on or off; returns the new `calendar` payload.
/// Turning it on that fails (no access) is not saved.
#[tauri::command]
pub async fn set_calendar_desktop(app: AppHandle, on: bool) -> Res<Value> {
    let started = calendar::begin();
    let url = calendar::stored_url(&app);
    let (events, note) = if on || url.is_some() {
        let u = url.clone();
        blocking(move || calendar::load(u.as_deref(), on)).await?
    } else {
        Default::default()
    };
    app.state::<AppState>().db.lock().unwrap().set(calendar::DESKTOP_KEY, if on { "1" } else { "" }).map_err(err)?;
    calendar::commit(&app, started, on || url.is_some(), Some(events), note, None)
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

/// Sound & brightness: OS calls are slow or COM-bound, so they run off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> Res<T> + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(err)?
}

#[tauri::command]
pub async fn bt_state() -> Res<bluetooth::BtState> {
    blocking(bluetooth::state).await
}

#[tauri::command]
pub async fn bt_set_radio(on: bool) -> Res<()> {
    blocking(move || bluetooth::set_radio(on)).await
}

/// `id` must be in the current device list.
#[tauri::command]
pub async fn bt_connect(id: String) -> Res<()> {
    blocking(move || bluetooth::connect(id)).await
}

#[tauri::command]
pub async fn bt_disconnect(id: String) -> Res<()> {
    blocking(move || bluetooth::disconnect(id)).await
}

#[tauri::command]
pub async fn sound_state() -> Res<SoundState> {
    blocking(sound::state).await
}

/// `value` 0..=1 (clamped).
#[tauri::command]
pub async fn set_volume(value: f64) -> Res<()> {
    blocking(move || sound::set_volume(value)).await
}

#[tauri::command]
pub async fn set_mute(muted: bool) -> Res<()> {
    blocking(move || sound::set_mute(muted)).await
}

#[tauri::command]
pub async fn audio_devices() -> Res<Vec<AudioDevice>> {
    blocking(sound::devices).await
}

/// `id` must be in the current device list.
#[tauri::command]
pub async fn set_audio_device(id: String) -> Res<()> {
    blocking(move || sound::set_default(id)).await
}

#[tauri::command]
pub async fn brightness_displays() -> Res<Vec<BrightDisplay>> {
    blocking(|| Ok(brightness::displays())).await
}

/// `id` from the last `brightness_displays` list; `percent` 0..=100 (clamped).
#[tauri::command]
pub async fn set_brightness(id: String, percent: u8) -> Res<()> {
    blocking(move || brightness::set(&id, percent)).await
}

#[tauri::command]
pub fn run_action(id: String) -> Res<()> {
    actions::run(&id)
}

#[tauri::command]
pub fn set_hit_rect(app: AppHandle, rect: window::Rect) {
    window::set_hit_rect(&app, rect);
}

const NO_DRAG: &str = "Moving the island isn't available on Linux yet";

/// AC / battery for the compact Widgets card.
#[tauri::command]
pub fn power_status() -> monitors::system::Power {
    monitors::system::power()
}

/// Hold-and-drag, start: from now on keep window point (`x`, `y`) (CSS px, the pill's centre) under the pointer.
#[tauri::command]
pub fn island_drag_start(app: AppHandle, state: State<AppState>, x: f64, y: f64) -> Res<()> {
    if !cfg!(windows) {
        return Err(NO_DRAG.into());
    }
    let win = app.get_webview_window("main").ok_or("no island window")?;
    let sf = win.scale_factor().map_err(err)?;
    *state.drag.lock().unwrap() = Some((x * sf, y * sf));
    Ok(())
}

/// Hold-and-drag, move: the window follows the pointer.
#[tauri::command]
pub fn island_drag_move(app: AppHandle, state: State<AppState>) -> Res<()> {
    let Some((ox, oy)) = *state.drag.lock().unwrap() else { return Ok(()) };
    let win = app.get_webview_window("main").ok_or("no island window")?;
    let c = app.cursor_position().map_err(err)?;
    win.set_position(tauri::PhysicalPosition::new((c.x - ox).round() as i32, (c.y - oy).round() as i32)).map_err(err)
}

/// Hold-and-drag, end: snap to the nearest edge of the display under the pointer, save it, place the window.
/// Returns the placement (`"left:0.4"`). Always ends the drag, also when something fails.
#[tauri::command]
pub fn island_drag_end(app: AppHandle, state: State<AppState>) -> Res<String> {
    if state.drag.lock().unwrap().take().is_none() || !cfg!(windows) {
        window::place(&app).map_err(err)?;
        return Err(NO_DRAG.into());
    }
    let c = app.cursor_position().map_err(err)?;
    let (mons, infos) = display::monitors(&app).map_err(err)?;
    let here = app.monitor_from_point(c.x, c.y).map_err(err)?;
    let i = here.and_then(|h| mons.iter().position(|m| m.position() == h.position())).or_else(|| infos.iter().position(|m| m.primary)).unwrap_or(0);
    let m = mons.get(i).ok_or("no display")?;
    let wa = m.work_area();
    let p = crate::placement::snap(c.x, c.y, (wa.position.x as f64, wa.position.y as f64, wa.size.width as f64, wa.size.height as f64));
    let text = crate::placement::format(p);
    {
        let db = state.db.lock().unwrap();
        db.set("placement", &text).map_err(err)?;
        let cur = db.settings().map_err(err)?.display;
        let keep_primary = cur == "primary" && infos[i].primary; // still "the primary", even if it changes later
        if !keep_primary && cur != infos[i].name {
            db.set("display", &infos[i].name).map_err(err)?;
        }
    }
    window::place(&app).map_err(err)?;
    emit_settings(&app);
    Ok(text)
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

/// Connect (native confirm, then merge our hooks into the agent's config, with backup) or disconnect agent `id`.
#[tauri::command]
pub async fn set_agent(app: AppHandle, id: String, on: bool) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || bridge::set_agent(&app, &id, on)).await.map_err(err)?
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
        assert_eq!(parse_general("reminder_sound", &json!(false)), Ok(General::ReminderSound(false)));
        assert!(parse_general("reminder_sound", &json!("off")).is_err());
        assert!(parse_general("autostart", &json!("yes")).is_err()); // wrong type
        assert!(parse_general("display", &json!(1)).is_err());
        assert!(parse_general("bridge_token", &json!("x")).is_err()); // not allow-listed
        assert!(parse_general("", &json!(null)).is_err());
    }
}

/// Clipboard history: lists are pinned first, then newest.
#[tauri::command]
pub fn list_clips(state: State<AppState>) -> Vec<Clip> {
    state.clips.lock().unwrap().list()
}

/// Writes the entry back to the clipboard; the write itself is kept out of history.
#[tauri::command]
pub fn copy_clip(state: State<AppState>, id: i64) -> Res<()> {
    let text = state.clips.lock().unwrap().text_of(id).ok_or("unknown clip")?;
    clipboard::write(&text)
}

fn edit_clips(app: &AppHandle, f: impl FnOnce(&mut clipboard::ClipStore)) -> Vec<Clip> {
    let state = app.state::<AppState>();
    let db = state.db.lock().unwrap();
    let mut store = state.clips.lock().unwrap();
    f(&mut store);
    clipboard::changed(app, &db, &store);
    store.list()
}

#[tauri::command]
pub fn pin_clip(app: AppHandle, id: i64, pinned: bool) -> Vec<Clip> {
    edit_clips(&app, |s| s.pin(id, pinned))
}

#[tauri::command]
pub fn remove_clip(app: AppHandle, id: i64) -> Vec<Clip> {
    edit_clips(&app, |s| s.remove(id))
}

/// Keeps pinned entries.
#[tauri::command]
pub fn clear_clips(app: AppHandle) -> Vec<Clip> {
    edit_clips(&app, |s| s.clear())
}

#[tauri::command]
pub fn set_clip_paused(app: AppHandle, paused: bool) -> Res<()> {
    app.state::<AppState>().db.lock().unwrap().set("clip_paused", &paused.to_string()).map_err(err)?;
    emit_settings(&app);
    Ok(())
}

/// Shelf (spec §23.6): the webview sends ids only; paths come from the DB.
fn shelf_path(app: &AppHandle, id: i64) -> Res<String> {
    app.state::<AppState>().db.lock().unwrap().shelf_path(id).map_err(err)?.ok_or_else(|| "unknown shelf item".to_string())
}

fn edit_shelf(app: &AppHandle, f: impl FnOnce(&db::Db) -> rusqlite::Result<()>) -> Res<Vec<ShelfItem>> {
    f(&app.state::<AppState>().db.lock().unwrap()).map_err(err)?;
    let list = shelf::list(app)?; // after the DB lock is released
    let _ = app.emit("shelf", &list);
    Ok(list)
}

/// Async + blocking pool: listing touches the file system and extracts icons.
#[tauri::command]
pub async fn list_shelf(app: AppHandle) -> Res<Vec<ShelfItem>> {
    tauri::async_runtime::spawn_blocking(move || shelf::list(&app)).await.map_err(err)?
}

#[tauri::command]
pub fn open_shelf(app: AppHandle, id: i64) -> Res<()> {
    let p = shelf_path(&app, id)?;
    if !std::path::Path::new(&p).exists() {
        return Err(format!("not found: {p}"));
    }
    launcher::os::shell_open(&p)
}

#[tauri::command]
pub fn reveal_shelf(app: AppHandle, id: i64) -> Res<()> {
    let p = shelf_path(&app, id)?;
    if !std::path::Path::new(&p).exists() {
        return Err(format!("not found: {p}"));
    }
    shelf::os::reveal(&p)
}

/// The path text goes to the clipboard without entering history.
#[tauri::command]
pub fn copy_shelf_path(app: AppHandle, id: i64) -> Res<()> {
    clipboard::write(&shelf_path(&app, id)?)
}

#[tauri::command]
pub fn remove_shelf(app: AppHandle, id: i64) -> Res<Vec<ShelfItem>> {
    edit_shelf(&app, |db| db.remove_shelf(id))
}

#[tauri::command]
pub fn clear_shelf(app: AppHandle) -> Res<Vec<ShelfItem>> {
    edit_shelf(&app, |db| db.clear_shelf())
}

/// Native drag of the stored file (called on pointer-drag from a shelf tile). Sync command = main thread, as OLE needs.
#[tauri::command]
pub fn drag_out(app: AppHandle, window: WebviewWindow, id: i64) -> Res<()> {
    let p = shelf_path(&app, id)?;
    if !std::path::Path::new(&p).exists() {
        return Err(format!("not found: {p}"));
    }
    shelf::os::drag_out(&window, &p)
}
