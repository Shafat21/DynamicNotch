mod actions;
mod appearance;
mod bluetooth;
mod brightness;
mod bridge;
mod clipboard;
mod commands;
mod db;
mod dictation;
mod display;
mod fullscreen;
pub mod hook;
mod launcher;
mod menu;
mod monitors;
mod notes;
mod notetaker;
mod reminders;
mod shelf;
mod sound;
pub mod statusline;
mod window;
mod worker;

use serde::Serialize;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::atomic::AtomicBool;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

pub struct AppState {
    pub db: Mutex<db::Db>,
    /// Last payload of each "sticky" event, so a freshly loaded page can catch up (see `get_snapshot`).
    pub last: Mutex<HashMap<&'static str, Value>>,
    /// Interactive area of the island, in logical px relative to the window.
    pub hit: Mutex<window::Rect>,
    /// Coding agent sessions seen by the hook bridge (memory only).
    pub sessions: Mutex<bridge::sessions::Sessions>,
    /// Permission / question prompts waiting for an answer in the island.
    pub prompts: bridge::Prompts,
    /// Latest account-wide plan usage from the statusLine forwarder (sticky event `usage`).
    pub usage: Mutex<bridge::usage::Usage>,
    /// True while a fullscreen app hides the island (spec §18).
    pub hidden: AtomicBool,
    /// Clipboard history (memory; mirrored to the DB only with "Remember across restarts").
    pub clips: Mutex<clipboard::ClipStore>,
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
        // first, so a second launch exits before setup runs
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let _ = commands::show_settings(app);
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .on_window_event(|win, event| {
            if let (tauri::WindowEvent::DragDrop(d), "main") = (event, win.label()) {
                shelf::on_drag_drop(win.app_handle(), d);
            }
            if let (tauri::WindowEvent::ScaleFactorChanged { .. }, "main") = (event, win.label()) {
                // off the event thread: the OS applies its own DPI resize after this handler returns
                let app = win.app_handle().clone();
                std::thread::spawn(move || {
                    if let Err(e) = window::place(&app) {
                        eprintln!("display: re-place failed: {e}");
                    }
                });
            }
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&dir)?;
            let db = db::Db::open(&dir.join("dynamic_notch.db"))?;
            let clips = if clipboard::remember(&db) { clipboard::ClipStore::from_saved(db.clips().unwrap_or_default()) } else { clipboard::ClipStore::default() };
            app.manage(AppState {
                db: Mutex::new(db),
                clips: Mutex::new(clips),
                last: Mutex::new(HashMap::new()),
                hit: Mutex::new(window::Rect::default()),
                sessions: Mutex::default(),
                prompts: bridge::Prompts::default(),
                usage: Mutex::default(),
                hidden: AtomicBool::new(false),
            });

            let (a, l) = appearance::load(&app.state::<AppState>().db.lock().unwrap());
            publish(app.handle(), "appearance", appearance::payload(&a, &l));

            let win = app.get_webview_window("main").expect("main window");
            window::setup(app.handle(), &win)?;
            display::spawn_watcher(app.handle().clone());
            fullscreen::spawn_watcher(app.handle().clone());
            reminders::spawn_scheduler(app.handle().clone());
            clipboard::spawn(app.handle().clone());
            sound::spawn(app.handle().clone());

            let handle = app.handle().clone();
            app.on_menu_event(move |_, event| menu::handle(&handle, event.id().as_ref()));

            monitors::spawn_all(app.handle().clone());
            bridge::spawn(app.handle().clone());
            dictation::spawn(app.handle().clone());
            notetaker::sweep(app.handle()); // leftover call audio from a crash or kill
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::open_settings,
            commands::set_general,
            commands::list_displays,
            commands::open_link,
            commands::get_appearance,
            commands::set_appearance,
            commands::apply_theme,
            commands::reset_appearance,
            commands::set_layout,
            commands::get_snapshot,
            commands::list_tasks,
            commands::add_task,
            commands::delete_task,
            commands::list_notes,
            commands::add_note,
            commands::update_note,
            commands::pin_note,
            commands::delete_note,
            commands::list_reminders,
            commands::quick_due,
            commands::add_reminder,
            commands::snooze_reminder,
            commands::done_reminder,
            commands::delete_reminder,
            commands::list_clips,
            commands::copy_clip,
            commands::pin_clip,
            commands::remove_clip,
            commands::clear_clips,
            commands::set_clip_paused,
            commands::list_shelf,
            commands::open_shelf,
            commands::reveal_shelf,
            commands::copy_shelf_path,
            commands::remove_shelf,
            commands::clear_shelf,
            commands::drag_out,
            commands::set_location,
            commands::set_calendar_url,
            commands::set_calendar_desktop,
            commands::open_join,
            commands::media_control,
            commands::media_seek,
            commands::media_open_player,
            commands::run_action,
            commands::bt_state,
            commands::bt_set_radio,
            commands::bt_connect,
            commands::bt_disconnect,
            commands::sound_state,
            commands::set_volume,
            commands::set_mute,
            commands::audio_devices,
            commands::set_audio_device,
            commands::brightness_displays,
            commands::set_brightness,
            commands::list_apps,
            commands::pick_app,
            commands::remove_app,
            commands::launch_app,
            commands::set_hit_rect,
            commands::show_context_menu,
            commands::answer_prompt,
            commands::set_agent,
            dictation::commands::dictation_settings,
            dictation::commands::set_dictation_settings,
            dictation::commands::download_model,
            dictation::commands::model_status,
            dictation::commands::set_cloud_key,
            dictation::commands::clear_cloud_key,
            dictation::commands::has_cloud_key,
            dictation::commands::list_mics,
            dictation::commands::dictation_cancel,
            dictation::commands::dictation_words,
            dictation::commands::set_dictation_words,
            dictation::commands::dictation_history,
            dictation::commands::delete_dictation,
            dictation::commands::dictation_stats,
            dictation::commands::dictation_copy,
            dictation::commands::voice_note_start,
            dictation::commands::voice_note_stop,
            notetaker::notetaker_start,
            notetaker::notetaker_stop,
            notetaker::notetaker_cancel,
            notetaker::notetaker_settings,
            notetaker::set_notetaker_claude,
        ])
        .build(tauri::generate_context!())
        .expect("error while running DynamicNotch")
        .run(|app, event| {
            // call audio never outlives the app (a crash or kill is swept at the next start)
            if matches!(event, tauri::RunEvent::Exit) {
                notetaker::shutdown(app);
            }
        });
}
