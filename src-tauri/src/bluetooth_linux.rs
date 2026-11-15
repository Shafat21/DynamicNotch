//! Linux (best effort, compile-unverified): `bluetoothctl`, which talks to BlueZ (`org.bluez`) over D-Bus
//! (spares a zbus dependency). Device ids are MAC addresses.

use super::{kind_from_icon, BtDevice};
use crate::sound::sh;

pub fn init_thread() {}

pub fn can_connect() -> bool {
    true
}

/// "Key: value" line of `bluetoothctl show|info` output.
fn field<'a>(out: &'a str, key: &str) -> Option<&'a str> {
    out.lines().find_map(|l| l.trim().strip_prefix(key)?.strip_prefix(':')).map(str::trim)
}

pub fn radio() -> Result<Option<bool>, String> {
    // no adapter: `show` fails or prints "No default controller available"
    Ok(sh("bluetoothctl", &["show"]).ok().and_then(|o| field(&o, "Powered").map(|p| p == "yes")))
}

pub fn set_radio(on: bool) -> Result<(), String> {
    sh("bluetoothctl", &["power", if on { "on" } else { "off" }]).map(|_| ())
}

pub fn devices() -> Result<Vec<BtDevice>, String> {
    // older BlueZ has no "devices Paired" filter
    let list = sh("bluetoothctl", &["devices", "Paired"]).or_else(|_| sh("bluetoothctl", &["paired-devices"]))?;
    let mut out = Vec::new();
    for mac in list.lines().filter_map(|l| l.strip_prefix("Device ")?.split_whitespace().next()) {
        let info = sh("bluetoothctl", &["info", mac])?;
        out.push(BtDevice {
            id: mac.into(),
            name: field(&info, "Alias").or_else(|| field(&info, "Name")).unwrap_or(mac).into(),
            kind: kind_from_icon(field(&info, "Icon").unwrap_or("")),
            connected: field(&info, "Connected") == Some("yes"),
        });
    }
    Ok(out)
}

pub fn connect(id: &str) -> Result<(), String> {
    sh("bluetoothctl", &["connect", id]).map(|_| ())
}

pub fn disconnect(id: &str) -> Result<(), String> {
    sh("bluetoothctl", &["disconnect", id]).map(|_| ())
}
