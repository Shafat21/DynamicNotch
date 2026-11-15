//! Linux window placement / click-through / hover: not implemented yet.

use super::Rect;
use tauri::{AppHandle, Monitor, WebviewWindow};

pub fn setup(app: &AppHandle, win: &WebviewWindow) -> tauri::Result<()> {
    eprintln!("window setup: click-through not implemented on Linux yet");
    super::place(app)?;
    win.show()
}

/// X11: below top panels. Task 12: layer-shell (Wayland) uses `set_monitor` for the chosen display instead.
pub fn top_y(m: &Monitor) -> i32 {
    m.work_area().position.y
}

pub fn set_hit_rect(_app: &AppHandle, _rect: Rect) {}
