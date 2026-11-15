//! Small watchers: accent colour (`accent`, once), Caps/Num Lock (`keylock`), charger plugged in
//! (`charging`). OS-specific sources live in `system_{windows,linux}.rs`.

use crate::publish;
use serde::Serialize;
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter};

#[cfg(windows)]
#[path = "system_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "system_linux.rs"]
mod os;

#[derive(Serialize, Clone)]
struct KeyLock {
    name: &'static str,
    on: bool,
}

pub fn spawn(app: AppHandle) {
    publish(&app, "accent", os::accent_color().unwrap_or_else(|| "#0078D7".into()));

    let a = app.clone();
    thread::spawn(move || {
        let mut last = os::caps_num();
        loop {
            thread::sleep(os::KEYLOCK_POLL);
            let now = os::caps_num();
            if now.0 != last.0 {
                let _ = a.emit("keylock", KeyLock { name: "Caps Lock", on: now.0 });
            }
            if now.1 != last.1 {
                let _ = a.emit("keylock", KeyLock { name: "Num Lock", on: now.1 });
            }
            last = now;
        }
    });

    thread::spawn(move || {
        let mut was_plugged = os::plugged_in();
        loop {
            thread::sleep(Duration::from_secs(2));
            let now = os::plugged_in();
            if now && !was_plugged {
                let _ = app.emit("charging", ());
            }
            was_plugged = now;
        }
    });
}
