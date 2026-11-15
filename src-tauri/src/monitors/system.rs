//! Small Win32 watchers: Windows accent colour (`accent`, once), Caps/Num Lock (`keylock`),
//! charger plugged in (`charging`).

use crate::publish;
use serde::Serialize;
use std::{thread, time::Duration};
use tauri::{AppHandle, Emitter};
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_NUMLOCK};

#[derive(Serialize, Clone)]
struct KeyLock {
    name: &'static str,
    on: bool,
}

pub fn spawn(app: AppHandle) {
    publish(&app, "accent", accent_color().unwrap_or_else(|| "#0078D7".into()));

    let a = app.clone();
    thread::spawn(move || {
        let mut last = (lock_on(VK_CAPITAL), lock_on(VK_NUMLOCK));
        loop {
            thread::sleep(Duration::from_millis(20));
            let now = (lock_on(VK_CAPITAL), lock_on(VK_NUMLOCK));
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
        let mut was_plugged = plugged_in();
        loop {
            thread::sleep(Duration::from_secs(2));
            let now = plugged_in();
            if now && !was_plugged {
                let _ = app.emit("charging", ());
            }
            was_plugged = now;
        }
    });
}

fn lock_on(vk: VIRTUAL_KEY) -> bool {
    // Low bit of GetKeyState = toggle state.
    unsafe { GetKeyState(vk.0 as i32) & 1 != 0 }
}

fn plugged_in() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).is_ok() && s.ACLineStatus == 1 }
}

fn accent_color() -> Option<String> {
    let c = UISettings::new().ok()?.GetColorValue(UIColorType::Accent).ok()?;
    Some(format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B))
}
