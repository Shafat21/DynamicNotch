//! Liquid Glass (spec §6.5): real desktop blur behind the island, clipped to its shape and following
//! the CSS morph. Pure geometry here; the OS side lives in `glass_{windows,linux}.rs`.

use serde::Deserialize;
use std::time::Duration;
use tauri::AppHandle;

#[cfg(windows)]
#[path = "glass_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "glass_linux.rs"]
mod os;

#[derive(Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Pill,
    /// square top corners (joined to the screen edge)
    Notch,
}

/// Island outline in logical px, already multiplied by the display uiScale (main.ts `geometry()`).
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub w: f64,
    pub h: f64,
    pub r: f64,
    pub top: f64,
    pub duration_ms: u64,
    #[serde(default)]
    pub shape: Kind,
}

/// CSS `cubic-bezier(x1, y1, x2, y2)`: progress for time fraction `t` (0..1).
pub fn cubic_bezier(x1: f64, y1: f64, x2: f64, y2: f64, t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    let bez = |a: f64, b: f64, u: f64| 3.0 * a * u * (1.0 - u).powi(2) + 3.0 * b * u * u * (1.0 - u) + u.powi(3);
    let dx = |u: f64| 3.0 * x1 * (1.0 - u).powi(2) + 6.0 * (x2 - x1) * u * (1.0 - u) + 3.0 * (1.0 - x2) * u * u;
    let mut u = t;
    for _ in 0..8 {
        let d = dx(u);
        if d.abs() < 1e-6 {
            break;
        }
        u = (u - (bez(x1, x2, u) - t) / d).clamp(0.0, 1.0);
    }
    if (bez(x1, x2, u) - t).abs() > 1e-6 {
        // Newton stalled: bisection (x(u) is monotonic for x1, x2 in 0..1)
        let (mut lo, mut hi) = (0.0, 1.0);
        for _ in 0..40 {
            u = (lo + hi) / 2.0;
            if bez(x1, x2, u) < t { lo = u } else { hi = u }
        }
    }
    bez(y1, y2, u)
}

/// The morph easing (`--ease` in styles.css).
pub fn ease(t: f64) -> f64 {
    cubic_bezier(0.16, 1.0, 0.3, 1.0, t)
}

pub fn lerp_shape(from: Shape, to: Shape, p: f64) -> Shape {
    let l = |a: f64, b: f64| a + (b - a) * p;
    Shape { w: l(from.w, to.w), h: l(from.h, to.h), r: l(from.r, to.r), top: l(from.top, to.top), ..to }
}

/// (left, top, right, bottom, corner diameter) in physical px, horizontally centred in the window.
pub fn region_px(s: Shape, window_w_logical: f64, scale: f64) -> (i32, i32, i32, i32, i32) {
    let px = |v: f64| (v * scale).round() as i32;
    let left = (window_w_logical - s.w) / 2.0;
    (px(left), px(s.top), px(left + s.w), px(s.top + s.h), px(s.r * 2.0))
}

/// One animation step at the monitor refresh rate, clamped to 60–165 Hz (spec §19).
pub fn frame_interval(hz: u32) -> Duration {
    Duration::from_secs_f64(1.0 / f64::from(hz.clamp(60, 165)))
}

/// Blur on (bg_mode "glass") or off; `tint` = the theme's dark colour.
pub fn set_enabled(app: &AppHandle, on: bool, tint: (u8, u8, u8)) {
    os::set_enabled(app, on, tint);
}

pub fn apply(app: &AppHandle, a: &crate::appearance::Appearance) {
    let t = crate::appearance::theme_for(a).bg_from;
    let c = |i: usize| u8::from_str_radix(&t[i..i + 2], 16).unwrap_or(0);
    set_enabled(app, a.bg_mode == "glass", (c(1), c(3), c(5)));
}

/// Morph the clip to `to`; a newer target cancels the running animation.
pub fn animate_to(app: &AppHandle, to: Shape) {
    os::animate_to(app, to);
}

/// Re-apply the current shape (fullscreen hide → empty region, un-hide → back).
pub fn refresh(app: &AppHandle) {
    os::refresh(app);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn shape(w: f64, h: f64, r: f64, duration_ms: u64) -> Shape {
        Shape { w, h, r, top: 10.0, duration_ms, shape: Kind::Pill }
    }

    #[test]
    fn ease_endpoints_and_monotonic() {
        let e = |t| cubic_bezier(0.16, 1.0, 0.3, 1.0, t);
        assert!(e(0.0).abs() < 1e-6 && (e(1.0) - 1.0).abs() < 1e-6);
        let mut prev = 0.0;
        for i in 1..=100 {
            let v = e(i as f64 / 100.0);
            assert!(v + 1e-9 >= prev);
            prev = v;
        }
        assert!(e(0.25) > 0.75); // OutExpo-like: most of the motion early
        assert!((cubic_bezier(0.0, 0.0, 1.0, 1.0, 0.3) - 0.3).abs() < 1e-6); // linear
    }
    #[test]
    fn region_is_centred_and_scaled() {
        let s = Shape { top: 10.0, ..shape(360.0, 40.0, 20.0, 0) };
        assert_eq!(region_px(s, 1200.0, 1.5), (630, 15, 1170, 75, 60));
    }
    #[test]
    fn lerp_hits_both_ends() {
        let a = shape(180.0, 40.0, 20.0, 850);
        let b = Shape { shape: Kind::Notch, ..shape(720.0, 230.0, 30.0, 850) };
        assert_eq!(lerp_shape(a, b, 0.0), Shape { shape: Kind::Notch, ..a });
        assert_eq!(lerp_shape(a, b, 1.0), b);
        assert_eq!(lerp_shape(a, b, 0.5).w, 450.0);
    }
    #[test]
    fn frame_follows_refresh_rate() {
        assert_eq!(frame_interval(30), frame_interval(60));
        assert_eq!(frame_interval(240), frame_interval(165));
        assert!((frame_interval(144).as_secs_f64() - 1.0 / 144.0).abs() < 1e-9);
    }
    #[test]
    fn shape_from_frontend_json() {
        let s: Shape = serde_json::from_str(r#"{"w":1,"h":2,"r":3,"top":0,"durationMs":250,"shape":"notch"}"#).unwrap();
        assert_eq!((s.duration_ms, s.shape), (250, Kind::Notch));
    }
}
