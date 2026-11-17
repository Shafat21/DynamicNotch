//! Windows: every USB HID device, grouped per physical device (vendor + product id); Logitech HID++ devices are
//! asked for their name, type and battery (a receiver for each paired device in slots 1..=6).

use super::hidpp::{self, Io};
use super::{kind_from_usages, via_from_name, Peripheral};
use hidapi::{BusType, DeviceInfo, HidApi, HidDevice};
use std::collections::BTreeMap;

const LOGITECH: u16 = 0x046D;

/// HID++ on Windows is two collections: long reports (requests and replies) and short ones, where a receiver
/// answers "no device in that slot / asleep". Reading both turns an empty slot from a 1 s wait into a quick no.
struct Hid {
    long: HidDevice,
    short: Option<HidDevice>,
}

impl Io for Hid {
    fn send(&self, report: &[u8]) -> bool {
        self.long.write(report).is_ok()
    }
    fn recv(&self, buf: &mut [u8], ms: i32) -> usize {
        if let Some(n) = self.short.as_ref().and_then(|s| s.read_timeout(buf, 0).ok()).filter(|&n| n > 0) {
            return n;
        }
        self.long.read_timeout(buf, ms).unwrap_or(0)
    }
}

/// Well-known vendors, for devices that report no product name.
fn vendor(vid: u16) -> &'static str {
    match vid {
        0x046D => "Logitech",
        0x1532 => "Razer",
        0x1038 => "SteelSeries",
        0x0951 | 0x03F0 => "HyperX",
        0x1B1C => "Corsair",
        0x258A => "Havit",
        0x3554 => "VXE",
        0x373B => "Attack Shark",
        0x0C76 => "ONIKUMA",
        _ => "USB",
    }
}

/// Not peripherals: a laptop's built-in keyboard / lighting controller (ITE) and G HUB's virtual keyboard.
fn skip(vid: u16, pid: u16) -> bool {
    vid == 0 || vid == 0x048D || (vid == LOGITECH && pid == 0xC232)
}

pub fn scan() -> Vec<Peripheral> {
    let Ok(api) = HidApi::new() else { return Vec::new() };
    // one entry per physical device: its collections (usage page, usage) and the HID++ long-report path if any
    let mut groups: BTreeMap<(u16, u16), Vec<&DeviceInfo>> = BTreeMap::new();
    for d in api.device_list().filter(|d| matches!(d.bus_type(), BusType::Usb) && !skip(d.vendor_id(), d.product_id())) {
        groups.entry((d.vendor_id(), d.product_id())).or_default().push(d);
    }
    let mut out = Vec::new();
    for ((vid, pid), infos) in groups {
        let name = infos.iter().find_map(|d| d.product_string().filter(|s| !s.trim().is_empty())).map(str::trim).map(String::from);
        let name = name.unwrap_or_else(|| format!("{} device", vendor(vid)));
        let id = format!("{vid:04x}:{pid:04x}");
        let path = |usage| infos.iter().find(|d| d.usage_page() == 0xFF00 && d.usage() == usage).map(|d| d.path());
        if vid == LOGITECH {
            if let Some(long) = path(0x0002).and_then(|p| api.open_path(p).ok()) {
                let dev = Hid { long, short: path(0x0001).and_then(|p| api.open_path(p).ok()) };
                let receiver = name.to_lowercase().contains("receiver");
                let found = logitech(&dev, &id, receiver);
                if !found.is_empty() {
                    out.extend(found);
                    continue;
                }
            }
        }
        let usages: Vec<(u16, u16)> = infos.iter().map(|d| (d.usage_page(), d.usage())).collect();
        out.push(Peripheral { id, kind: kind_from_usages(&usages, &name), via: via_from_name(&name), name, battery: None, charging: false });
    }
    out
}

/// The devices behind a Logitech receiver (slots 1..=6), or the device itself on a cable (index 0xFF).
fn logitech(dev: &Hid, id: &str, receiver: bool) -> Vec<Peripheral> {
    let slots: Vec<u8> = if receiver { (1..=6).collect() } else { vec![0xFF] };
    slots
        .into_iter()
        .filter_map(|i| {
            let (name, ty) = hidpp::name_and_type(dev, i)?;
            let b = hidpp::battery(dev, i);
            Some(Peripheral {
                id: format!("{id}:{i}"),
                name: if name.starts_with("Logitech") || name.starts_with("G") || name.starts_with("MX") { name } else { format!("Logitech {name}") },
                kind: hidpp::kind_name(ty),
                via: if receiver { "wireless" } else { "usb" },
                battery: b.map(|b| b.percent),
                charging: b.is_some_and(|b| b.charging),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    /// Live check against the hardware plugged into this PC: `cargo test --lib live_peripherals -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_peripherals() {
        for p in super::scan() {
            println!("{p:?}");
        }
    }
}
