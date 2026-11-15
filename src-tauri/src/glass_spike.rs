//! THROWAWAY spike (Task 1). Runs only with DN_GLASS_SPIKE=1. Delete this file, `mod glass_spike;`
//! and the `glass_spike::run(...)` line in lib.rs to remove.
use std::{thread, time::Duration};
use tauri::{
    window::{Effect, EffectsBuilder},
    WebviewWindow,
};
use windows::Win32::{
    Foundation::HWND,
    Graphics::Gdi::{CreateRoundRectRgn, SetWindowRgn},
};

fn set_pill(hwnd: HWND, win_w: f64, w: f64, h: f64, top: f64, scale: f64) {
    let (w, h, top) = (w * scale, h * scale, top * scale);
    let x1 = (win_w - w) / 2.0;
    unsafe {
        let rgn = CreateRoundRectRgn(x1 as i32, top as i32, (x1 + w) as i32, (top + h) as i32, h as i32, h as i32);
        SetWindowRgn(hwnd, Some(rgn), true);
    }
}

// shortcut: easeOutQuart instead of cubic-bezier(0.16,1,0.3,1); fine for a visual spike.
fn ease(t: f64) -> f64 {
    1.0 - (1.0 - t).powi(4)
}

pub fn run(win: &WebviewWindow) {
    if std::env::var("DN_GLASS_SPIKE").as_deref() != Ok("1") {
        return;
    }
    let r = win.set_effects(EffectsBuilder::new().effect(Effect::Acrylic).color(tauri::window::Color(0, 0, 0, 80)).build());
    eprintln!("[glass_spike] set_effects(Acrylic) -> {r:?}");
    let Ok(raw) = win.hwnd() else { return };
    let hwnd = HWND(raw.0 as _);
    let scale = win.scale_factor().unwrap_or(1.0);
    let win_w = win.outer_size().map(|s| s.width as f64).unwrap_or(1200.0);
    set_pill(hwnd, win_w, 360.0, 40.0, 10.0, scale);
    let addr = hwnd.0 as isize;
    thread::spawn(move || {
        let hwnd = HWND(addr as _);
        loop {
            thread::sleep(Duration::from_secs(1));
            for (from, to) in [((180.0, 40.0), (720.0, 230.0)), ((720.0, 230.0), (180.0, 40.0))] {
                let steps = 850 / 16;
                for i in 0..=steps {
                    let e = ease(i as f64 / steps as f64);
                    set_pill(hwnd, win_w, from.0 + (to.0 - from.0) * e, from.1 + (to.1 - from.1) * e, 10.0, scale);
                    thread::sleep(Duration::from_millis(16));
                }
                thread::sleep(Duration::from_secs(1));
            }
        }
    });
}
