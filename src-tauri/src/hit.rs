//! Click-through: the window is 1200x700 and transparent, but only the island should take the mouse.
//! Poll the cursor every 25 ms; inside the island rect -> accept input and emit `hover: true`.

use crate::AppState;
use serde::Deserialize;
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager};

#[derive(Deserialize, Clone, Copy, Default, Debug)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.x + self.w && py >= self.y && py <= self.y + self.h
    }
}

pub fn spawn(app: AppHandle) {
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

#[cfg(test)]
mod tests {
    use super::Rect;

    #[test]
    fn contains_edges() {
        let r = Rect { x: 10.0, y: 0.0, w: 100.0, h: 40.0 };
        assert!(r.contains(10.0, 0.0) && r.contains(110.0, 40.0) && r.contains(50.0, 20.0));
        assert!(!r.contains(9.9, 20.0) && !r.contains(50.0, 40.1));
    }
}
