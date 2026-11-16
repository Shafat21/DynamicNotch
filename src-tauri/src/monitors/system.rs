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

/// Power for the compact Widgets card: on AC or not, battery percent; None = unknown / no battery.
#[derive(serde::Serialize, Clone, Copy, Debug, PartialEq)]
pub struct Power {
    pub plugged: Option<bool>,
    pub percent: Option<u8>,
}

/// Windows' encoding (also used by the Linux reader): AC 0/1, 255 = unknown; percent 0..=100, else unknown.
pub fn power_from_raw(ac: u8, percent: u8) -> Power {
    Power { plugged: match ac { 0 => Some(false), 1 => Some(true), _ => None }, percent: (percent <= 100).then_some(percent) }
}

pub fn power() -> Power {
    let (ac, pct) = os::power_raw();
    power_from_raw(ac, pct)
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

#[cfg(test)]
mod power_tests {
    use super::*;

    #[test]
    fn power_maps_raw_status_and_unknowns() {
        assert_eq!(power_from_raw(1, 82), Power { plugged: Some(true), percent: Some(82) });
        assert_eq!(power_from_raw(0, 40), Power { plugged: Some(false), percent: Some(40) });
        // 255 = unknown for both (Windows), and a PC with no battery reports 255 percent
        assert_eq!(power_from_raw(255, 255), Power { plugged: None, percent: None });
        assert_eq!(power_from_raw(1, 120), Power { plugged: Some(true), percent: None });
    }
}
