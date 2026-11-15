//! Windows: Acrylic behind the transparent webview, clipped with `SetWindowRgn` to the island, the
//! region stepped on one background thread at the monitor refresh rate.

use super::{ease, frame_interval, lerp_shape, region_px, Kind, Shape};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::window::{Color, Effect, EffectsBuilder};
use tauri::{AppHandle, Manager, WebviewWindow};
use windows::core::PCWSTR;
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    CombineRgn, CreateRectRgn, CreateRoundRectRgn, DeleteObject, EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromWindow, SetWindowRgn, DEVMODEW,
    ENUM_CURRENT_SETTINGS, MONITORINFO, MONITORINFOEXW, MONITOR_DEFAULTTONEAREST, RGN_OR,
};

enum Msg {
    Enable(bool, (u8, u8, u8)),
    Target(Shape),
    Refresh,
}

static TX: OnceLock<Mutex<Sender<Msg>>> = OnceLock::new();

fn send(app: &AppHandle, m: Msg) {
    let tx = TX.get_or_init(|| {
        let (tx, rx) = channel();
        let app = app.clone();
        std::thread::spawn(move || run(app, rx));
        Mutex::new(tx)
    });
    let _ = tx.lock().unwrap().send(m);
}

pub fn set_enabled(app: &AppHandle, on: bool, tint: (u8, u8, u8)) {
    send(app, Msg::Enable(on, tint));
}

pub fn animate_to(app: &AppHandle, to: Shape) {
    send(app, Msg::Target(to));
}

pub fn refresh(app: &AppHandle) {
    send(app, Msg::Refresh);
}

fn hwnd(win: &WebviewWindow) -> Option<HWND> {
    win.hwnd().ok().map(|h| HWND(h.0 as _))
}

fn refresh_hz(hwnd: HWND) -> u32 {
    let mut mi = MONITORINFOEXW { monitorInfo: MONITORINFO { cbSize: size_of::<MONITORINFOEXW>() as u32, ..Default::default() }, ..Default::default() };
    let mut dm = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
    unsafe {
        let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        if !GetMonitorInfoW(mon, &mut mi.monitorInfo).as_bool()
            || !EnumDisplaySettingsW(PCWSTR(mi.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut dm).as_bool()
        {
            return 60;
        }
    }
    dm.dmDisplayFrequency
}

/// left, top, right, bottom, corner diameter (physical px), shape
type Region = (i32, i32, i32, i32, i32, Kind);

/// `None` = no clip (solid mode); `Some(None)` = empty (hidden); `Some(Some(..))` = the island.
fn set_region(hwnd: HWND, region: Option<Option<Region>>) {
    unsafe {
        let rgn = match region {
            None => None,
            Some(None) => Some(CreateRectRgn(0, 0, 0, 0)),
            Some(Some((l, t, r, b, d, kind))) => {
                let rgn = CreateRoundRectRgn(l, t, r + 1, b + 1, d, d);
                if kind == Kind::Notch {
                    let top = CreateRectRgn(l, t, r + 1, t + (b - t) / 2 + 1);
                    CombineRgn(Some(rgn), Some(rgn), Some(top), RGN_OR);
                    let _ = DeleteObject(top.into());
                }
                Some(rgn)
            }
        };
        SetWindowRgn(hwnd, rgn, true); // Windows owns (and frees) the region from here
    }
}

fn blur(win: &WebviewWindow, on: bool, (r, g, b): (u8, u8, u8)) {
    let res = if on {
        win.set_effects(EffectsBuilder::new().effect(Effect::Acrylic).color(Color(r, g, b, 40)).build())
    } else {
        win.set_effects(None)
    };
    if let Err(e) = res {
        eprintln!("glass: blur {on} failed: {e}");
    }
}

fn run(app: AppHandle, rx: std::sync::mpsc::Receiver<Msg>) {
    let mut enabled = false;
    let mut last_enable = None;
    let mut cur: Option<Shape> = None; // latest target (kept while disabled)
    let mut shown: Option<Shape> = None; // what the region shows now: a new morph starts here
    let mut anim: Option<(Shape, Shape, Instant)> = None;
    // window logical width, scale, frame interval: re-read per target (the window moves/resizes rarely)
    let mut metrics = (1200.0, 1.0, frame_interval(60));
    loop {
        let msg = match anim {
            Some(_) => rx.recv_timeout(metrics.2),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        let Some(win) = app.get_webview_window("main") else { continue };
        let Some(hwnd) = hwnd(&win) else { continue };
        let mut apply_now = false;
        match msg {
            Err(RecvTimeoutError::Disconnected) => return,
            Err(RecvTimeoutError::Timeout) => {}
            Ok(Msg::Enable(on, tint)) if last_enable == Some((on, tint)) => {} // appearance changed something else
            Ok(Msg::Enable(on, tint)) => {
                last_enable = Some((on, tint));
                blur(&win, on, tint);
                enabled = on;
                anim = None;
                if on {
                    apply_now = true;
                } else {
                    shown = None;
                    set_region(hwnd, None);
                }
            }
            Ok(Msg::Refresh) => {
                anim = None;
                apply_now = true;
            }
            Ok(Msg::Target(to)) => {
                let scale = win.scale_factor().unwrap_or(1.0);
                let w = win.inner_size().map(|s| s.width as f64 / scale).unwrap_or(1200.0);
                metrics = (w, scale, frame_interval(refresh_hz(hwnd)));
                match shown {
                    Some(from) if enabled && to.duration_ms > 0 && from != to => anim = Some((from, to, Instant::now())),
                    _ => {
                        anim = None;
                        apply_now = true;
                    }
                }
                cur = Some(to);
            }
        }
        if !enabled {
            anim = None;
            continue;
        }
        let shape = match anim {
            Some((from, to, start)) => {
                let t = start.elapsed().as_secs_f64() / Duration::from_millis(to.duration_ms).as_secs_f64();
                if t >= 1.0 {
                    anim = None;
                }
                Some(lerp_shape(from, to, ease(t.min(1.0))))
            }
            None if apply_now => cur,
            None => continue,
        };
        let hidden = crate::fullscreen::is_hidden(&app);
        let region = shape.filter(|_| !hidden).map(|s| {
            let (l, t, r, b, d) = region_px(s, metrics.0, metrics.1);
            (l, t, r, b, d, s.shape)
        });
        set_region(hwnd, Some(region));
        shown = shape;
    }
}
