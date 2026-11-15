//! Which monitor the island sits on and how big it is (spec §19, §20). Pure maths here; the
//! window move itself is `window::place`.

use crate::{db, window, AppState};
use std::{thread, time::Duration};
use tauri::{AppHandle, Manager, Monitor};

#[derive(Clone, Debug)]
pub struct MonitorInfo {
    pub name: String,
    pub primary: bool,
    pub width: f64,
    pub height: f64,
}

/// Base 1080p layout multiplier: `Auto` follows the work-area height, the others are fixed.
pub fn ui_scale(work_area_h: f64, setting: &str) -> f64 {
    match setting {
        "Small" => 0.85,
        "Medium" => 1.0,
        "Large" => 1.15,
        _ => (work_area_h / 1080.0).clamp(0.85, 1.25),
    }
}

/// Shrink `ui` until the 720 px dashboard plus a 40 px margin fits the work-area width.
pub fn dashboard_fit(ui: f64, work_w: f64) -> f64 {
    ui.min((work_w - 40.0) / 720.0)
}

/// Window size (logical px): the 1200x700 base, scaled, but never larger than the work area.
pub fn fit_window(work_w: f64, work_h: f64, ui: f64) -> (f64, f64) {
    ((1200.0 * ui).min(work_w), (700.0 * ui).min(work_h))
}

/// The named monitor if plugged in, else the primary, else the first.
pub fn choose_monitor(mons: &[MonitorInfo], setting: &str) -> Option<usize> {
    mons.iter()
        .position(|m| m.name == setting)
        .or_else(|| mons.iter().position(|m| m.primary))
        .or((!mons.is_empty()).then_some(0))
}

/// `Monitor::name()` can be missing; fall back to its 1-based order.
pub fn monitor_name(m: &Monitor, index: usize) -> String {
    m.name().cloned().unwrap_or_else(|| format!("Display {}", index + 1))
}

/// Monitors in `available_monitors()` order, with the primary flagged.
pub fn monitors(app: &AppHandle) -> tauri::Result<(Vec<Monitor>, Vec<MonitorInfo>)> {
    let mons = app.available_monitors()?;
    let primary = app.primary_monitor()?;
    let infos = mons
        .iter()
        .enumerate()
        .map(|(i, m)| MonitorInfo {
            name: monitor_name(m, i),
            primary: primary.as_ref().is_some_and(|p| p.position() == m.position() && p.name() == m.name()),
            width: m.size().width as f64,
            height: m.size().height as f64,
        })
        .collect();
    Ok((mons, infos))
}

/// Save the chosen screen (`"primary"` or a monitor name) and move the island there.
pub fn set_display(app: &AppHandle, name: &str) -> Result<(), String> {
    if !valid_display(name, &monitors(app).map_err(|e| e.to_string())?.1) {
        return Err(format!("unknown display {name}"));
    }
    save_and_place(app, "display", name)
}

/// `"primary"` or a monitor that is plugged in right now.
fn valid_display(choice: &str, mons: &[MonitorInfo]) -> bool {
    choice == "primary" || mons.iter().any(|m| m.name == choice)
}

/// Save the island size (`db::ISLAND_SIZES`) and re-scale.
pub fn set_island_size(app: &AppHandle, size: &str) -> Result<(), String> {
    if !db::ISLAND_SIZES.contains(&size) {
        return Err(format!("unknown island size {size}"));
    }
    save_and_place(app, "island_size", size)
}

fn save_and_place(app: &AppHandle, key: &str, value: &str) -> Result<(), String> {
    app.state::<AppState>().db.lock().unwrap().set(key, value).map_err(|e| e.to_string())?;
    window::place(app).map_err(|e| e.to_string())
}

type Signature = Vec<(String, (i32, i32), (u32, u32), f64, ((i32, i32), (u32, u32)))>;

fn signature(app: &AppHandle) -> Option<Signature> {
    let mons = app.available_monitors().ok()?;
    Some(
        mons.iter()
            .enumerate()
            .map(|(i, m)| {
                let (p, s, wa) = (m.position(), m.size(), m.work_area());
                let work = ((wa.position.x, wa.position.y), (wa.size.width, wa.size.height));
                (monitor_name(m, i), (p.x, p.y), (s.width, s.height), m.scale_factor(), work)
            })
            .collect(),
    )
}

/// Every 2 s: re-place when monitors, resolution, scaling or the taskbar change.
pub fn spawn_watcher(app: AppHandle) {
    thread::spawn(move || {
        let mut last = signature(&app);
        loop {
            thread::sleep(Duration::from_secs(2));
            let now = signature(&app);
            if now.is_some() && now != last {
                last = now;
                if let Err(e) = window::place(&app) {
                    eprintln!("display: re-place failed: {e}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_rules() {
        assert_eq!(ui_scale(1080.0, "Auto"), 1.0);
        assert_eq!(ui_scale(720.0, "Auto"), 0.85); // clamped low
        assert_eq!(ui_scale(2160.0, "Auto"), 1.25); // clamped high
        assert_eq!(ui_scale(1440.0, "Small"), 0.85);
        assert_eq!(ui_scale(1440.0, "Large"), 1.15);
        assert_eq!(ui_scale(1440.0, "bogus"), ui_scale(1440.0, "Auto"));
        assert!((dashboard_fit(1.25, 900.0) - (860.0 / 720.0)).abs() < 1e-9);
        assert_eq!(dashboard_fit(1.0, 1920.0), 1.0);
    }

    #[test]
    fn window_fits_screen() {
        assert_eq!(fit_window(1920.0, 1040.0, 1.0), (1200.0, 700.0));
        assert_eq!(fit_window(1024.0, 600.0, 1.0), (1024.0, 600.0));
        assert_eq!(fit_window(3840.0, 2100.0, 1.25), (1500.0, 875.0));
    }

    #[test]
    fn monitor_choice_falls_back() {
        let m = |n: &str, p: bool| MonitorInfo { name: n.into(), primary: p, width: 1920.0, height: 1080.0 };
        let mons = [m(r"\\.\DISPLAY1", false), m(r"\\.\DISPLAY2", true)];
        assert_eq!(choose_monitor(&mons, "primary"), Some(1));
        assert_eq!(choose_monitor(&mons, r"\\.\DISPLAY1"), Some(0));
        assert_eq!(choose_monitor(&mons, r"\\.\DISPLAY9"), Some(1)); // unplugged → primary
        assert_eq!(choose_monitor(&[m("a", false)], "primary"), Some(0)); // no primary flag → first
        assert_eq!(choose_monitor(&[], "primary"), None);
    }

    #[test]
    fn display_choice_is_validated() {
        let mons = [MonitorInfo { name: r"\\.\DISPLAY1".into(), primary: true, width: 1920.0, height: 1080.0 }];
        assert!(valid_display("primary", &mons));
        assert!(valid_display(r"\\.\DISPLAY1", &mons));
        assert!(!valid_display(r"\\.\DISPLAY9", &mons));
        assert!(!valid_display("", &mons));
    }
}
