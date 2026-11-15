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

/// (left, top, right, bottom). A window covers a monitor when it reaches past every edge.
pub fn covers(win: (i32, i32, i32, i32), mon: (i32, i32, i32, i32)) -> bool {
    win.0 <= mon.0 && win.1 <= mon.1 && win.2 >= mon.2 && win.3 >= mon.3
}

pub fn is_hidden(app: &AppHandle) -> bool {
    app.state::<AppState>().hidden.load(Ordering::Relaxed)
}

/// One evaluation: `hidden = setting && fullscreen`; on change store it and publish `fullscreen`.
pub fn tick(app: &AppHandle) {
    let state = app.state::<AppState>();
    let enabled = state.db.lock().unwrap().settings().map(|s| s.hide_fullscreen).unwrap_or(true);
    let hide = enabled && os::is_fullscreen(app);
    if state.hidden.swap(hide, Ordering::Relaxed) != hide {
        #[cfg(windows)]
        if hide {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_ignore_cursor_events(true); // clicks reach the game right away
            }
        }
        publish(app, "fullscreen", hide);
        crate::glass::refresh(app); // hidden → empty clip, visible → the island again
        if !hide {
            crate::reminders::tick(app); // reminders held back while hidden go out now
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
    fn covers_whole_monitor_only() {
        let mon = (0, 0, 1920, 1080);
        assert!(covers((0, 0, 1920, 1080), mon));
        assert!(covers((-8, -8, 1928, 1088), mon)); // overhang still counts
        assert!(!covers((0, 0, 1920, 1040), mon)); // taskbar visible → not fullscreen
        assert!(!covers((1920, 0, 3840, 1080), mon)); // other monitor
    }
}
