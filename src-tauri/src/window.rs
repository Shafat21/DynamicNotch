//! Window placement + click-through: the window is 1200x700 and transparent, but only the island
//! should take the mouse. OS-specific code lives in `window_{windows,linux}.rs`.

use crate::placement::{self, Edge};
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
    // shortcut: Linux stays top-centre until placement is tried on real hardware
    let p = if cfg!(windows) { placement::parse(&s.placement) } else { placement::Placement::default() };
    let side = matches!(p.edge, Edge::Left | Edge::Right);
    // pill size for clamping: the idle pill (about 180x40) or the side tab (40x120), scaled
    let pill = if side { (40.0, 120.0) } else { (180.0, 40.0) };
    let work = (wa.position.x as f64, wa.position.y as f64, wa.size.width as f64, wa.size.height as f64);
    let (x, y, anchor) = placement::window_pos(p, work, (w * sf, h * sf), (pill.0 * ui * sf, pill.1 * ui * sf));
    // the top edge hangs from the monitor's very top (over a top taskbar), as before
    let y = if p.edge == Edge::Top { os::top_y(m) as f64 } else { y };
    // move first, so the logical size below uses the target monitor's scale factor
    win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    win.set_size(LogicalSize::new(w, h))?;
    let mon = &infos[i];
    publish(
        app,
        "display",
        json!({ "uiScale": ui, "edge": p.edge, "anchor": anchor / sf, "monitor": { "index": i, "name": mon.name, "width": mon.width, "height": mon.height, "scale": sf } }),
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
