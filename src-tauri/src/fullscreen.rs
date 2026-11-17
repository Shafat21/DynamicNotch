//! Hide the island while a fullscreen app or game is in front (spec §18). The watcher is
//! platform-neutral; detection lives in `fullscreen_{windows,linux}.rs`.

use crate::{publish, AppState};
use std::sync::atomic::Ordering;
use std::{thread, time::Duration};
use tauri::{AppHandle, Manager};

#[cfg(windows)]
#[path = "fullscreen_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "fullscreen_linux.rs"]
mod os;
#[cfg(windows)]
pub use os::exe_of_pid;

/// (left, top, right, bottom). A window covers a monitor when it reaches past every edge.
pub fn covers(win: (i32, i32, i32, i32), mon: (i32, i32, i32, i32)) -> bool {
    win.0 <= mon.0 && win.1 <= mon.1 && win.2 >= mon.2 && win.3 >= mon.3
}

/// "C:\Apps\Code.EXE" -> "code.exe": how apps are listed and compared.
pub fn exe_name(s: &str) -> String {
    s.trim().rsplit(['/', '\\']).next().unwrap_or("").trim().to_lowercase()
}

/// The foreground app is on the "keep showing over these apps" list.
pub fn is_exempt(list: &[String], foreground: Option<&str>) -> bool {
    foreground.is_some_and(|f| list.iter().any(|e| *e == exe_name(f)))
}

const EXEMPT_KEY: &str = "fullscreen_exempt";

/// The "keep showing over these apps" list (file names, lower-case), as saved.
pub fn exempt_apps(app: &AppHandle) -> Vec<String> {
    let json = app.state::<AppState>().db.lock().unwrap().get(EXEMPT_KEY).ok().flatten().unwrap_or_default();
    serde_json::from_str(&json).unwrap_or_default()
}

/// Add (`on`) or drop an app from that list; returns the new list. Applies at once.
pub fn set_exempt(app: &AppHandle, name: &str, on: bool) -> Result<Vec<String>, String> {
    let name = exe_name(name);
    if name.is_empty() {
        return Err("Enter an app's file name, like chrome.exe".into());
    }
    let mut list = exempt_apps(app);
    list.retain(|e| *e != name);
    if on {
        list.push(name);
        list.sort();
    }
    app.state::<AppState>().db.lock().unwrap().set(EXEMPT_KEY, &serde_json::to_string(&list).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    tick(app);
    Ok(list)
}

/// Apps with a visible window, for the picker in Settings (file names, lower-case, sorted).
pub fn running_apps() -> Vec<String> {
    os::running_apps()
}

/// Where a fullscreen app is: nowhere, on a given monitor (its rect), or somewhere unknown (counts as everywhere).
#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Fs {
    No,
    All,
    Monitor((i32, i32, i32, i32)),
}

/// Does a fullscreen app at `fs` hide an island that sits on `monitor`?
pub fn hides(fs: Fs, monitor: Option<(i32, i32, i32, i32)>) -> bool {
    match fs {
        Fs::No => false,
        Fs::All => true,
        Fs::Monitor(r) => monitor.is_none_or(|m| m == r), // an island whose monitor is unknown hides, as before
    }
}

/// The first island is hidden (reminders and prompts check this one).
pub fn is_hidden(app: &AppHandle) -> bool {
    is_hidden_for(app, crate::window::MAIN)
}

pub fn is_hidden_for(app: &AppHandle, label: &str) -> bool {
    let s = app.state::<AppState>();
    if label == crate::window::SECOND { s.hidden2.load(Ordering::Relaxed) } else { s.hidden.load(Ordering::Relaxed) }
}

/// The second island was closed: it no longer counts as hidden.
pub fn island_closed(app: &AppHandle, label: &str) {
    if label == crate::window::SECOND {
        app.state::<AppState>().hidden2.store(false, Ordering::Relaxed);
    }
}

/// One evaluation per island: `hidden = setting && a fullscreen app on its monitor`; on change store it and publish
/// `fullscreen` (the second island's own event is `fullscreen:island2`).
pub fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    let enabled = state.db.lock().unwrap().settings().map(|s| s.hide_fullscreen).unwrap_or(true);
    let dual = app.get_webview_window(crate::window::SECOND).is_some();
    let fs = if enabled { os::fullscreen_state(app, dual) } else { Fs::No };
    let exempt = fs != Fs::No && is_exempt(&exempt_apps(app), os::foreground_exe().as_deref());
    for label in crate::window::ISLANDS {
        if app.get_webview_window(label).is_none() {
            continue;
        }
        let hide = !exempt && hides(fs, os::window_monitor(app, label));
        let flag = if label == crate::window::SECOND { &state.hidden2 } else { &state.hidden };
        if flag.swap(hide, Ordering::Relaxed) != hide {
            #[cfg(windows)]
            if hide {
                if let Some(w) = app.get_webview_window(label) {
                    let _ = w.set_ignore_cursor_events(true); // clicks reach the game right away
                }
            }
            publish(app, crate::window::evt(label, "fullscreen"), hide);
            if !hide && label == crate::window::MAIN {
                crate::reminders::tick(app); // reminders held back while hidden go out now
            }
        }
    }
}

pub fn spawn_watcher(app: AppHandle) {
    thread::spawn(move || loop {
        tick(&app);
        thread::sleep(Duration::from_millis(500));
    });
}

/// Save the "Hide in fullscreen apps" setting (Settings window → `set_general`) and apply it at once.
pub fn set_hide_fullscreen(app: &AppHandle, on: bool) -> Result<(), String> {
    app.state::<AppState>().db.lock().unwrap().set("hide_fullscreen", if on { "true" } else { "false" }).map_err(|e| e.to_string())?;
    tick(app);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exempt_apps_match_by_file_name_ignoring_case_and_folder() {
        let list = vec!["code.exe".to_string(), "vlc.exe".to_string()];
        assert_eq!(exe_name(" C:\\Apps\\Code.EXE "), "code.exe");
        assert!(is_exempt(&list, Some("C:\\Program Files\\VLC\\vlc.exe")));
        assert!(!is_exempt(&list, Some("game.exe")));
        assert!(!is_exempt(&list, None)); // unknown foreground app: not exempt
        assert!(!is_exempt(&[], Some("code.exe")));
    }

    #[test]
    fn a_fullscreen_app_hides_only_the_island_on_its_monitor() {
        let (a, b) = ((0, 0, 1920, 1080), (1920, 0, 3840, 1080));
        assert!(hides(Fs::Monitor(a), Some(a)));
        assert!(!hides(Fs::Monitor(a), Some(b))); // the other screen's island stays
        assert!(hides(Fs::All, Some(b))); // location unknown: everything hides
        assert!(!hides(Fs::No, Some(a)));
        assert!(hides(Fs::Monitor(a), None)); // island with no known monitor: hide, as before
    }

    #[test]
    fn covers_whole_monitor_only() {
        let mon = (0, 0, 1920, 1080);
        assert!(covers((0, 0, 1920, 1080), mon));
        assert!(covers((-8, -8, 1928, 1088), mon)); // overhang still counts
        assert!(!covers((0, 0, 1920, 1040), mon)); // taskbar visible → not fullscreen
        assert!(!covers((1920, 0, 3840, 1080), mon)); // other monitor
    }
}
