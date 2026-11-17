//! Wired and wireless peripherals (USB and 2.4 GHz dongles) for the Devices tab, with battery where the device tells
//! us: Logitech over HID++ (receivers and direct links). Others are listed by name, without a battery level.
//! Bluetooth devices stay in the Bluetooth list above it. Windows only for now (hidapi on Windows' own HID API).

use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[cfg_attr(not(windows), allow(dead_code))]
pub mod hidpp;
#[cfg(windows)]
mod windows;

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Peripheral {
    pub id: String,
    pub name: String,
    /// "mouse" | "keyboard" | "headset" | "gamepad" | "audio" | "other"
    pub kind: &'static str,
    /// "wireless" (dongle) | "usb" (cable)
    pub via: &'static str,
    /// Percent; None when the device doesn't report it (or is asleep).
    pub battery: Option<u8>,
    pub charging: bool,
}

#[cfg_attr(not(windows), allow(dead_code))]
/// HID usages (page, usage) of a device's collections -> what kind of device it is.
pub fn kind_from_usages(usages: &[(u16, u16)], name: &str) -> &'static str {
    let has = |u: (u16, u16)| usages.contains(&u);
    let n = name.to_lowercase();
    if ["headset", "headphone", "earbud", "barracuda", "kraken", "arctis", "blackshark"].iter().any(|w| n.contains(w)) {
        "headset"
    } else if has((1, 2)) {
        "mouse"
    } else if has((1, 6)) {
        "keyboard"
    } else if has((1, 4)) || has((1, 5)) {
        "gamepad"
    } else if usages.iter().any(|u| u.0 == 0x0B) || (usages.iter().all(|u| u.0 == 0x0C || u.0 >= 0xFF00) && usages.iter().any(|u| u.0 == 0x0C)) {
        "audio" // telephony, or only media keys (+ vendor): a USB headset / speaker's volume buttons
    } else {
        "other"
    }
}

#[cfg_attr(not(windows), allow(dead_code))]
/// Dongles say so in their name ("LIGHTSPEED Receiver", "Barracuda X 2.4", "Wireless Gaming Headset").
pub fn via_from_name(name: &str) -> &'static str {
    let n = name.to_lowercase();
    if ["receiver", "wireless", "dongle", "2.4", "lightspeed", "hyperspeed", "unifying", "bolt"].iter().any(|w| n.contains(w)) {
        "wireless"
    } else {
        "usb"
    }
}

static CACHE: Mutex<Option<(Instant, Vec<Peripheral>)>> = Mutex::new(None);

/// The connected peripherals; asking the devices takes up to a second or two, so a list younger than 20 s is reused.
pub fn list() -> Vec<Peripheral> {
    let mut cache = CACHE.lock().unwrap();
    if let Some((at, l)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(20) {
            return l.clone();
        }
    }
    #[cfg(windows)]
    let l = windows::scan();
    #[cfg(not(windows))]
    let l = Vec::new(); // shortcut: Linux would need hidraw access rules (udev), add when someone asks
    *cache = Some((Instant::now(), l.clone()));
    l
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kinds_from_usages_and_names() {
        assert_eq!(kind_from_usages(&[(1, 2), (1, 6), (0x0C, 1), (0xFF00, 2)], "LIGHTSPEED Receiver"), "mouse");
        assert_eq!(kind_from_usages(&[(1, 6), (0x0C, 1)], "K845"), "keyboard");
        assert_eq!(kind_from_usages(&[(0x0C, 1), (0xFF00, 1)], "Razer Barracuda X 2.4"), "headset");
        assert_eq!(kind_from_usages(&[(0x0C, 1)], "ONIKUMA"), "audio");
        assert_eq!(kind_from_usages(&[(0xFF00, 1)], "Some gadget"), "other");
        assert_eq!(via_from_name("Razer Barracuda X 2.4"), "wireless");
        assert_eq!(via_from_name("ONIKUMA"), "usb");
    }
}
