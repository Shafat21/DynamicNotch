//! Linux: top-centre placement and click-through. The window is 1200x700 and transparent; a GTK input shape
//! limits the part that takes the pointer to the island's hit rect, so the desktop under the rest stays usable
//! (X11 input shape / Wayland input region; both via GTK). Hover comes from the webview itself (main.ts): unlike
//! Windows there is no global cursor to poll on Wayland, and under XWayland it goes stale over native windows.
//! Placement and always-on-top need X11 semantics: lib.rs runs GTK on XWayland unless GDK_BACKEND is set.

use super::Rect;
use crate::AppState;
use gtk::cairo::{RectangleInt, Region};
use gtk::prelude::WidgetExt;
use tauri::{AppHandle, Manager, Monitor, WebviewWindow};

pub fn setup(app: &AppHandle, win: &WebviewWindow) -> tauri::Result<()> {
    super::place(app)?;
    input_region(win, None); // nothing takes clicks until the page reports where the island is
    win.show()?;
    super::place(app) // X11 window managers may move a window when it is mapped
}

/// Below the top panel (GNOME's top bar, KDE's panel).
pub fn top_y(m: &Monitor) -> i32 {
    m.work_area().position.y
}

pub fn set_hit_rect(app: &AppHandle, rect: Rect) {
    *app.state::<AppState>().hit.lock().unwrap() = rect;
    let Some(win) = app.get_webview_window("main") else { return };
    // GTK is main-thread only; commands run on a worker
    let _ = app.run_on_main_thread(move || input_region(&win, Some(rect)));
}

/// The part of the window that takes the pointer: `rect` (logical px, window-relative), or nothing.
fn input_region(win: &WebviewWindow, rect: Option<Rect>) {
    let Ok(gtk_win) = win.gtk_window() else { return };
    let region = match rect.and_then(pixels) {
        Some((x, y, w, h)) => Region::create_rectangle(&RectangleInt::new(x, y, w, h)),
        None => Region::create(),
    };
    gtk_win.input_shape_combine_region(Some(&region));
}

/// `rect` as whole pixels covering it, or None when it is empty (hidden island).
fn pixels(r: Rect) -> Option<(i32, i32, i32, i32)> {
    let (x, y) = (r.x.floor(), r.y.floor());
    let (w, h) = ((r.x + r.w).ceil() - x, (r.y + r.h).ceil() - y);
    (w >= 1.0 && h >= 1.0).then_some((x as i32, y as i32, w as i32, h as i32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hit_rect_becomes_covering_pixels() {
        assert_eq!(pixels(Rect { x: 10.4, y: 0.0, w: 100.2, h: 40.0 }), Some((10, 0, 101, 40)));
        assert_eq!(pixels(Rect { x: 0.0, y: 0.0, w: 0.0, h: 0.0 }), None); // hidden: click-through everywhere
    }
}
