//! Bluetooth (spec §23.5): radio switch and the paired devices with their connection state. The frontend
//! polls `state()` every 10 s while the tab or card is shown. OS calls run on one worker thread (WinRT needs
//! an initialized apartment on the calling thread; on Windows the worker joins the MTA).

use crate::worker::{self, Job};
use serde::Serialize;
use std::sync::{mpsc, OnceLock};

#[cfg(windows)]
#[path = "bluetooth_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "bluetooth_linux.rs"]
mod os;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct BtDevice {
    pub id: String,
    pub name: String,
    /// "audio" | "input" | "phone" | "other"
    pub kind: &'static str,
    pub connected: bool,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BtState {
    /// None = no Bluetooth adapter
    pub radio: Option<bool>,
    pub devices: Vec<BtDevice>,
    pub can_connect: bool,
}

/// Classic Bluetooth class-of-device: major class is bits 8..=12.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn kind_from_class(cod: u32) -> &'static str {
    match (cod >> 8) & 0x1f {
        0x04 => "audio", // audio/video
        0x05 => "input", // peripheral: keyboard, mouse, gamepad
        0x02 => "phone",
        _ => "other",
    }
}

/// BLE GAP appearance: category is the value >> 6.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn kind_from_appearance(a: u16) -> &'static str {
    match a >> 6 {
        1 => "phone",
        15 => "input",            // HID: keyboard, mouse, joystick, gamepad
        33 | 34 | 37 => "audio",  // audio sink, audio source, wearable audio
        _ => "other",
    }
}

/// BlueZ `Device1.Icon` (freedesktop icon names).
#[cfg_attr(windows, allow(dead_code))]
pub fn kind_from_icon(icon: &str) -> &'static str {
    if icon.starts_with("audio-") {
        "audio"
    } else if icon.starts_with("input-") {
        "input"
    } else if icon.starts_with("phone") {
        "phone"
    } else {
        "other"
    }
}

/// A dual-mode device is listed as classic and as LE: fold an LE entry into a classic one only when exactly
/// one classic entry has that exact name (the name is all they share); otherwise keep both rows.
#[cfg_attr(target_os = "linux", allow(dead_code))]
pub fn merge_le(mut list: Vec<BtDevice>, le: Vec<BtDevice>) -> Vec<BtDevice> {
    let classic = list.len();
    for d in le {
        let mut same = (0..classic).filter(|&i| list[i].name == d.name);
        match (same.next(), same.next()) {
            (Some(i), None) => {
                list[i].connected |= d.connected;
                if list[i].kind == "other" {
                    list[i].kind = d.kind;
                }
            }
            _ => list.push(d),
        }
    }
    list
}

/// The webview may only address a device from the current list.
pub fn check_id(id: &str, devices: &[BtDevice]) -> Result<(), String> {
    if devices.iter().any(|d| d.id == id) {
        Ok(())
    } else {
        Err("Unknown Bluetooth device".into())
    }
}

static JOBS: OnceLock<mpsc::Sender<Job>> = OnceLock::new();

fn run<T: Send + 'static>(f: impl FnOnce() -> Result<T, String> + Send + 'static) -> Result<T, String> {
    let jobs = JOBS.get_or_init(|| worker::spawn(os::init_thread));
    worker::call(Some(jobs), f)
}

pub fn state() -> Result<BtState, String> {
    run(|| {
        let radio = os::radio()?;
        let devices = if radio.is_some() { os::devices().unwrap_or_default() } else { Vec::new() };
        Ok(BtState { radio, devices, can_connect: os::can_connect() })
    })
}

/// Devices connected in `after` that weren't in `before` (by id): what the "connected" toast announces.
pub fn newly_connected(before: &[BtDevice], after: &[BtDevice]) -> Vec<BtDevice> {
    after.iter().filter(|d| d.connected && !before.iter().any(|b| b.id == d.id && b.connected)).cloned().collect()
}

/// Watches for paired devices connecting and emits `bt-connected` {id, name, kind, connected} for each.
/// shortcut: polls every 8 s (enumeration is not free); a WinRT DeviceWatcher would be instant — upgrade if the
/// toast feels late.
pub fn watch(app: tauri::AppHandle) {
    use tauri::Emitter;
    std::thread::spawn(move || {
        let mut last: Option<Vec<BtDevice>> = None; // the first look only sets the baseline: no toast at startup
        loop {
            if let Ok(st) = state() {
                if let Some(prev) = &last {
                    for d in newly_connected(prev, &st.devices) {
                        let _ = app.emit("bt-connected", &d);
                    }
                }
                last = Some(st.devices);
            }
            std::thread::sleep(std::time::Duration::from_secs(8));
        }
    });
}

pub fn set_radio(on: bool) -> Result<(), String> {
    run(move || os::set_radio(on))
}

pub fn connect(id: String) -> Result<(), String> {
    run(move || {
        check_id(&id, &os::devices()?)?;
        os::connect(&id)
    })
}

pub fn disconnect(id: String) -> Result<(), String> {
    run(move || {
        check_id(&id, &os::devices()?)?;
        os::disconnect(&id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(id: &str, connected: bool) -> BtDevice {
        BtDevice { id: id.into(), name: format!("Dev {id}"), kind: "audio", connected }
    }

    #[test]
    fn newly_connected_lists_only_flips() {
        let before = [dev("a", false), dev("b", true), dev("c", false)];
        let after = [dev("a", true), dev("b", true), dev("c", false), dev("d", true)];
        let ids: Vec<String> = newly_connected(&before, &after).into_iter().map(|d| d.id).collect();
        assert_eq!(ids, ["a", "d"]); // a turned on, d is new and connected; b was already on
        assert!(newly_connected(&after, &after).is_empty());
    }

    #[test]
    fn class_of_device_maps_to_kind() {
        assert_eq!(kind_from_class(0x240404), "audio"); // headset
        assert_eq!(kind_from_class(0x200418), "audio"); // headphones
        assert_eq!(kind_from_class(0x002540), "input"); // keyboard
        assert_eq!(kind_from_class(0x002580), "input"); // mouse
        assert_eq!(kind_from_class(0x002508), "input"); // gamepad
        assert_eq!(kind_from_class(0x5a020c), "phone");
        assert_eq!(kind_from_class(0x00010c), "other"); // computer
        assert_eq!(kind_from_class(0), "other");
    }

    #[test]
    fn appearance_maps_to_kind() {
        assert_eq!(kind_from_appearance(0x03C1), "input"); // keyboard
        assert_eq!(kind_from_appearance(0x03C2), "input"); // mouse
        assert_eq!(kind_from_appearance(0x03C4), "input"); // gamepad
        assert_eq!(kind_from_appearance(0x0040), "phone");
        assert_eq!(kind_from_appearance(0x0841), "audio");
        assert_eq!(kind_from_appearance(0x0941), "audio"); // earbud
        assert_eq!(kind_from_appearance(0), "other");
    }

    #[test]
    fn bluez_icon_maps_to_kind() {
        assert_eq!(kind_from_icon("audio-headset"), "audio");
        assert_eq!(kind_from_icon("audio-card"), "audio");
        assert_eq!(kind_from_icon("input-keyboard"), "input");
        assert_eq!(kind_from_icon("input-gaming"), "input");
        assert_eq!(kind_from_icon("phone"), "phone");
        assert_eq!(kind_from_icon(""), "other");
    }

    #[test]
    fn le_entries_merge_only_on_a_unique_name() {
        let d = |id: &str, name: &str, connected| BtDevice { id: id.into(), name: name.into(), kind: "other", connected };
        let one = merge_le(vec![d("c1", "Pro 2", false)], vec![d("l1", "Pro 2", true)]);
        assert_eq!((one.len(), one[0].connected), (1, true));
        let two = merge_le(vec![d("c1", "Pro 2", true), d("c2", "Pro 2", false)], vec![d("l1", "Pro 2", false)]);
        assert_eq!(two.len(), 3);
        assert!(two[0].connected && !two[1].connected); // states not mixed
        assert_eq!(merge_le(vec![d("c1", "A", false)], vec![d("l1", "B", false)]).len(), 2);
    }

    #[test]
    fn device_must_be_in_the_current_list() {
        let d = |id: &str| BtDevice { id: id.into(), name: "x".into(), kind: "other", connected: false };
        let list = [d("a"), d("b")];
        assert!(check_id("b", &list).is_ok());
        assert!(check_id("c", &list).is_err());
        assert!(check_id("", &list).is_err());
        assert!(check_id("a", &[]).is_err());
    }
}
