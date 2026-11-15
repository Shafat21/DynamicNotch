//! Window placement + click-through: the window is 1200x700 and transparent, but only the island
//! should take the mouse. OS-specific code lives in `window_{windows,linux}.rs`.

use crate::{display, publish};
use serde::Deserialize;
use serde_json::json;
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition};

#[cfg(windows)]
#[path = "window_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "window_linux.rs"]
mod os;
pub use os::{set_hit_rect, setup};

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

/// Put the window top-centre on the chosen monitor, sized and scaled for its work area, and
/// publish `display`. Safe to call again on any monitor/setting change.
pub fn place(app: &AppHandle) -> tauri::Result<()> {
    let s = crate::current_settings(app).map_err(|e| tauri::Error::Io(std::io::Error::other(e)))?;
    let win = app.get_webview_window("main").ok_or(tauri::Error::WindowNotFound)?;
    let (mons, infos) = display::monitors(app)?;
    let Some(i) = display::choose_monitor(&infos, &s.display) else { return Ok(()) };
    let m = &mons[i];
    let sf = m.scale_factor();
    let wa = m.work_area();
    let (work_w, work_h) = (wa.size.width as f64 / sf, wa.size.height as f64 / sf);
    let ui = display::dashboard_fit(display::ui_scale(work_h, &s.island_size), work_w);
    let (w, h) = display::fit_window(work_w, work_h, ui);
    // move first, so the logical size below uses the target monitor's scale factor
    let x = m.position().x + (m.size().width as i32 - (w * sf).round() as i32) / 2;
    win.set_position(PhysicalPosition::new(x, os::top_y(m)))?;
    win.set_size(LogicalSize::new(w, h))?;
    let mon = &infos[i];
    publish(
        app,
        "display",
        json!({ "uiScale": ui, "monitor": { "index": i, "name": mon.name, "width": mon.width, "height": mon.height, "scale": sf } }),
    );
    Ok(())
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
