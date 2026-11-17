//! Windows: top-centre placement, and a 25 ms cursor poll that toggles click-through and emits `hover`.

use super::{evt, Rect, ISLANDS};
use crate::AppState;
use std::{collections::HashMap, thread, time::Duration};
use tauri::{AppHandle, Emitter, Manager, Monitor, WebviewWindow};

/// Top-centre of the chosen monitor; clicks pass through until the cursor reaches the island.
pub fn setup(app: &AppHandle, win: &WebviewWindow) -> tauri::Result<()> {
    super::place(app)?;
    win.set_ignore_cursor_events(true)?;
    win.show()?;
    spawn(app.clone());
    Ok(())
}

/// The island hangs from the very top of the monitor (over the taskbar if it is on top).
pub fn top_y(m: &Monitor) -> i32 {
    m.position().y
}

/// `label`: which island window the rect belongs to (each page reports its own).
pub fn set_hit_rect(app: &AppHandle, label: &str, rect: Rect) {
    app.state::<AppState>().hit.lock().unwrap().insert(label.to_string(), rect);
}

#[derive(Debug, PartialEq)]
enum Action {
    None,
    ForceIgnore,
    Set(bool),
}

/// While hidden: force click-through once per hidden period. Otherwise: toggle on change only.
fn hover_action(hidden: bool, forced: bool, prev: Option<bool>, now: bool) -> Action {
    if hidden {
        if forced { Action::None } else { Action::ForceIgnore }
    } else if prev != Some(now) {
        Action::Set(now)
    } else {
        Action::None
    }
}

/// Per island window: whether the cursor was over it last tick, and whether click-through was forced while hidden.
#[derive(Default)]
struct Track {
    inside: Option<bool>,
    forced: bool,
}

fn spawn(app: AppHandle) {
    thread::spawn(move || {
        let mut tracks: HashMap<&str, Track> = HashMap::new();
        loop {
            thread::sleep(Duration::from_millis(25));
            for label in ISLANDS {
                let Some(win) = app.get_webview_window(label) else {
                    tracks.remove(label);
                    continue;
                };
                let t = tracks.entry(label).or_default();
                let (Ok(cursor), Ok(pos), Ok(scale)) = (app.cursor_position(), win.outer_position(), win.scale_factor()) else {
                    continue;
                };
                let rect = app.state::<AppState>().hit.lock().unwrap().get(label).copied().unwrap_or_default();
                let now = rect.contains((cursor.x - pos.x as f64) / scale, (cursor.y - pos.y as f64) / scale);
                let hidden = crate::fullscreen::is_hidden_for(&app, label);
                match hover_action(hidden, t.forced, t.inside, now) {
                    Action::ForceIgnore => {
                        let _ = win.set_ignore_cursor_events(true);
                        t.forced = true;
                        t.inside = None; // re-evaluate once visible again
                    }
                    Action::Set(v) => {
                        // the watcher may have hidden us since the read above: never undo its click-through
                        if crate::fullscreen::is_hidden_for(&app, label) {
                            continue;
                        }
                        t.inside = Some(v);
                        let _ = win.set_ignore_cursor_events(!v);
                        let _ = app.emit(evt(label, "hover"), v);
                    }
                    Action::None => {}
                }
                if !hidden {
                    t.forced = false;
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hover_action_respects_hidden() {
        assert_eq!(hover_action(true, false, Some(true), true), Action::ForceIgnore); // inside the rect when hidden
        assert_eq!(hover_action(true, true, None, true), Action::None); // once per hidden period
        assert_eq!(hover_action(false, true, None, true), Action::Set(true)); // visible again
        assert_eq!(hover_action(false, false, Some(true), true), Action::None);
        assert_eq!(hover_action(false, false, Some(true), false), Action::Set(false));
    }
}
