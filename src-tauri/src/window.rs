//! Window placement + click-through: the window is 1200x700 and transparent, but only the island
//! should take the mouse. OS-specific code lives in `window_{windows,linux}.rs`.

use crate::placement::{self, Edge};
use crate::{display, publish};
use serde::Deserialize;
use serde_json::json;
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, LogicalSize, Manager, PhysicalPosition};

/// WebView2 flags for every window (they share one browser process, so they must match `tauri.conf.json`):
/// Tauri's defaults, plus the GPU and network services inside the browser process instead of 2 extra processes
/// (~60 MB). Crashpad stays: the WebView2 runtime always starts it. Ignored on Linux (WebKitGTK).
pub const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --in-process-gpu --enable-features=NetworkServiceInProcess2";

#[cfg(windows)]
#[path = "window_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "window_linux.rs"]
mod os;
pub use os::{set_hit_rect, setup};

/// The first island, and the second one that "Both displays" adds on another monitor (Windows).
pub const MAIN: &str = "main";
pub const SECOND: &str = "island2";
pub const ISLANDS: [&str; 2] = [MAIN, SECOND];

/// Event name for one island window: "hover" for the first, "hover:island2" for the second (each page listens for its own).
pub fn evt(label: &str, base: &'static str) -> &'static str {
    match (label, base) {
        (SECOND, "hover") => "hover:island2",
        (SECOND, "display") => "display:island2",
        (SECOND, "fullscreen") => "fullscreen:island2",
        (_, b) => b,
    }
}

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

/// Put the island(s) on their monitors, sized and scaled for each work area, and publish `display`. With "Both
/// displays" on (Windows) a second island window is created on the other monitor, or closed when it isn't wanted.
/// Safe to call again on any monitor/setting change.
pub fn place(app: &AppHandle) -> tauri::Result<()> {
    let s = crate::current_settings(app).map_err(|e| tauri::Error::Io(std::io::Error::other(e)))?;
    let (mons, infos) = display::monitors(app)?;
    let Some(i) = display::choose_monitor(&infos, &s.display) else { return Ok(()) };
    place_on(app, MAIN, i, &s, &mons, &infos)?;
    let second = if cfg!(windows) && s.dual_notch { display::second_monitor(&infos, i) } else { None };
    match (second, app.get_webview_window(SECOND)) {
        (Some(j), Some(_)) => place_on(app, SECOND, j, &s, &mons, &infos)?,
        (Some(_), None) => create_second(app.clone()),
        (None, Some(w)) => {
            let _ = w.destroy();
            crate::fullscreen::island_closed(app, SECOND);
        }
        (None, None) => {}
    }
    Ok(())
}

static CREATING: AtomicBool = AtomicBool::new(false);

/// Builds the second island on a worker thread (creating a window from a command's own thread can deadlock on
/// Windows), then places it. The page is the same one the first island runs.
fn create_second(app: AppHandle) {
    if CREATING.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::spawn(move || {
        let made = (|| -> tauri::Result<()> {
            let mut cfg = app.config().app.windows.first().cloned().ok_or(tauri::Error::WindowNotFound)?;
            cfg.label = SECOND.into();
            let win = tauri::WebviewWindowBuilder::from_config(&app, &cfg)?.build()?;
            win.set_ignore_cursor_events(true)?; // clicks pass through until the cursor reaches the island
            win.show()
        })();
        CREATING.store(false, Ordering::SeqCst);
        match made {
            Ok(()) => {
                if let Err(e) = place(&app) {
                    eprintln!("display: placing the second island failed: {e}");
                }
            }
            Err(e) => eprintln!("display: could not create the second island: {e}"),
        }
    });
}

fn place_on(app: &AppHandle, label: &str, i: usize, s: &crate::db::Settings, mons: &[tauri::Monitor], infos: &[display::MonitorInfo]) -> tauri::Result<()> {
    let win = app.get_webview_window(label).ok_or(tauri::Error::WindowNotFound)?;
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
    // move first, so the logical size below uses the target monitor's scale factor
    win.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32))?;
    win.set_size(LogicalSize::new(w, h))?;
    let mon = &infos[i];
    publish(
        app,
        evt(label, "display"),
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

#[cfg(test)]
mod browser_args_tests {
    #[test]
    fn conf_uses_the_same_webview2_flags() {
        let conf: serde_json::Value = serde_json::from_str(include_str!("../tauri.conf.json")).unwrap();
        assert_eq!(conf["app"]["windows"][0]["additionalBrowserArgs"], super::BROWSER_ARGS);
    }
}
