//! Windows: top-centre placement, and a 25 ms cursor poll that toggles click-through and emits `hover`.

use super::Rect;
use crate::AppState;
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, Monitor, WebviewWindow};

/// Top-centre of the chosen monitor; clicks pass through until the cursor reaches the island.
pub fn setup(app: &AppHandle, win: &WebviewWindow) -> tauri::Result<()> {
    super::place(app)?;
    win.set_ignore_cursor_events(true)?;
    win.show()?;
    spawn(app.clone());
    Ok(())
}

/// The island hangs from the very top of the monitor (over the taskbar if it is on top).
pub fn top_y(m: &Monitor) -> i32 {
    m.position().y
}

pub fn set_hit_rect(app: &AppHandle, rect: Rect) {
    *app.state::<AppState>().hit.lock().unwrap() = rect;
}

fn spawn(app: AppHandle) {
    thread::spawn(move || {
        let mut inside: Option<bool> = None;
        loop {
            thread::sleep(Duration::from_millis(25));
            let Some(win) = app.get_webview_window("main") else { continue };
            let (Ok(cursor), Ok(pos), Ok(scale)) = (app.cursor_position(), win.outer_position(), win.scale_factor())
            else {
                continue;
            };
            let rect = *app.state::<AppState>().hit.lock().unwrap();
            let now = rect.contains((cursor.x - pos.x as f64) / scale, (cursor.y - pos.y as f64) / scale);
            if inside != Some(now) {
                inside = Some(now);
                let _ = win.set_ignore_cursor_events(!now);
                let _ = app.emit("hover", now);
            }
        }
    });
}
