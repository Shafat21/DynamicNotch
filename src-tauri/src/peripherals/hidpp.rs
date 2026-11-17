//! Logitech HID++ 2.0 over a receiver (LIGHTSPEED / Unifying / Bolt: devices at index 1..=6) or a direct link
//! (wired / Bluetooth: index 0xFF). Just what the Devices tab needs: name, type and battery. Long reports only
//! (report 0x11, 20 bytes): Windows opens the long-report collection, and every receiver since Unifying takes them.
//! Feature numbers and layouts as documented by Logitech's public HID++ 2.0 drafts and Solaar.

/// How reports reach the device; a fake in the tests.
pub trait Io {
    fn send(&self, report: &[u8]) -> bool;
    /// One incoming report into `buf`, waiting at most `ms`; 0 = nothing arrived.
    fn recv(&self, buf: &mut [u8], ms: i32) -> usize;
}

const LONG: u8 = 0x11;
/// Our software id (low nibble of byte 3): lets us tell our replies from G HUB's and from notifications.
const SW: u8 = 0x0B;

const ROOT: u16 = 0x0000;
const NAME: u16 = 0x0005;
const BATTERY_STATUS: u16 = 0x1000;
const BATTERY_VOLTAGE: u16 = 0x1001;
const UNIFIED_BATTERY: u16 = 0x1004;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Battery {
    pub percent: u8,
    pub charging: bool,
}

pub fn request(dev: u8, feature: u8, func: u8, params: &[u8]) -> [u8; 20] {
    let mut r = [0u8; 20];
    r[..4].copy_from_slice(&[LONG, dev, feature, (func << 4) | SW]);
    let n = params.len().min(16);
    r[4..4 + n].copy_from_slice(&params[..n]);
    r
}

/// One call: the 16 parameter bytes of the matching reply. None on an error reply (feature unsupported, device
/// asleep or not paired) or when nothing matching arrives within about a second.
pub fn call(io: &dyn Io, dev: u8, feature: u8, func: u8, params: &[u8]) -> Option<[u8; 16]> {
    if !io.send(&request(dev, feature, func, params)) {
        return None;
    }
    let mut buf = [0u8; 64];
    for _ in 0..20 {
        let n = io.recv(&mut buf, 50);
        if n < 4 || buf[1] != dev {
            continue; // nothing yet, or another device / another app's traffic
        }
        // HID++ 2.0 error: [.., dev, 0xFF, feature, func|sw, code]; HID++ 1.0 error (receiver): [0x10, dev, 0x8F, ..]
        if (buf[2] == 0xFF && buf[3] == feature && buf[4] == (func << 4) | SW) || (buf[0] == 0x10 && buf[2] == 0x8F && buf[3] == feature) {
            return None;
        }
        if buf[0] == LONG && buf[2] == feature && buf[3] == (func << 4) | SW {
            let mut p = [0u8; 16];
            p.copy_from_slice(&buf[4..20]);
            return Some(p);
        }
    }
    None
}

/// Index of `feature` on this device (the root feature's GetFeature); None when the device lacks it.
pub fn feature_index(io: &dyn Io, dev: u8, feature: u16) -> Option<u8> {
    let p = call(io, dev, ROOT as u8, 0, &feature.to_be_bytes())?;
    (p[0] != 0).then_some(p[0])
}

/// The device's marketing name ("G502 X PLUS") and type (0x0005 GetDeviceType: 0 keyboard, 3 mouse, 8 headset, ...).
pub fn name_and_type(io: &dyn Io, dev: u8) -> Option<(String, u8)> {
    let f = feature_index(io, dev, NAME)?;
    let len = call(io, dev, f, 0, &[])?[0] as usize;
    let mut name = Vec::new();
    while name.len() < len {
        let chunk = call(io, dev, f, 1, &[name.len() as u8])?;
        let take = (len - name.len()).min(16);
        name.extend_from_slice(&chunk[..take]);
    }
    let kind = call(io, dev, f, 2, &[]).map_or(0xFF, |p| p[0]);
    Some((String::from_utf8_lossy(&name).trim_end_matches('\0').trim().to_string(), kind))
}

pub fn kind_name(device_type: u8) -> &'static str {
    match device_type {
        0 | 2 => "keyboard",
        3 | 4 | 5 => "mouse",
        8 => "headset",
        12 | 11 | 10 => "gamepad",
        _ => "other",
    }
}

/// Battery from whichever battery feature the device has, newest first.
pub fn battery(io: &dyn Io, dev: u8) -> Option<Battery> {
    if let Some(f) = feature_index(io, dev, UNIFIED_BATTERY) {
        return call(io, dev, f, 1, &[]).map(|p| unified(&p));
    }
    if let Some(f) = feature_index(io, dev, BATTERY_STATUS) {
        return call(io, dev, f, 0, &[]).map(|p| Battery { percent: p[0].min(100), charging: matches!(p[2], 1..=4) });
    }
    if let Some(f) = feature_index(io, dev, BATTERY_VOLTAGE) {
        return call(io, dev, f, 0, &[]).map(|p| Battery { percent: voltage_percent(u16::from_be_bytes([p[0], p[1]])), charging: p[2] & 0x80 != 0 });
    }
    None
}

/// 0x1004 GetStatus: [state of charge %, level flags, charging status, external power]. Devices that only report a
/// level (no percentage) get a rough figure from the flags: full / good / low / critical.
fn unified(p: &[u8; 16]) -> Battery {
    let percent = if p[0] > 0 {
        p[0].min(100)
    } else {
        match p[1] {
            l if l & 8 != 0 => 90,
            l if l & 4 != 0 => 60,
            l if l & 2 != 0 => 20,
            _ => 5,
        }
    };
    Battery { percent, charging: matches!(p[2], 1..=3) }
}

/// Li-ion voltage (mV) to charge, piecewise linear over the curve Solaar uses for 0x1001 devices.
pub fn voltage_percent(mv: u16) -> u8 {
    const CURVE: [(u16, u8); 13] = [
        (4186, 100), (4067, 90), (3989, 80), (3922, 70), (3859, 60), (3811, 50), (3778, 40),
        (3751, 30), (3717, 20), (3671, 10), (3646, 5), (3579, 2), (3500, 0),
    ];
    if mv >= CURVE[0].0 {
        return 100;
    }
    for w in CURVE.windows(2) {
        let ((hv, hp), (lv, lp)) = (w[0], w[1]);
        if mv >= lv {
            return lp + ((mv - lv) as u32 * (hp - lp) as u32 / (hv - lv) as u32) as u8;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::VecDeque;

    /// A device that answers from a table: (feature index, function, params) -> reply params.
    struct Fake {
        dev: u8,
        features: Vec<(u16, u8)>,
        replies: Vec<((u8, u8), Vec<u8>)>,
        queue: RefCell<VecDeque<Vec<u8>>>,
    }

    impl Io for Fake {
        fn send(&self, r: &[u8]) -> bool {
            let (dev, feat, func) = (r[1], r[2], r[3] >> 4);
            let mut q = self.queue.borrow_mut();
            q.push_back(vec![0x11, 0x07, 0x04, 0x00, 0x55]); // someone else's notification first
            if dev != self.dev {
                q.push_back(vec![0x10, dev, 0x8F, feat, r[3], 0x01, 0]); // receiver: no device in that slot
                return true;
            }
            let reply = if feat == 0 {
                let want = u16::from_be_bytes([r[4], r[5]]);
                Some(vec![self.features.iter().find(|f| f.0 == want).map_or(0, |f| f.1)])
            } else {
                let p = self.replies.iter().find(|((f, fu), _)| *f == feat && *fu == func).map(|(_, p)| p.clone());
                // GetDeviceName(offset): the name from that offset on, like a real device
                if feat == 3 && func == 1 { p.map(|p| p[(r[4] as usize).min(p.len())..].to_vec()) } else { p }
            };
            let mut out = vec![0x11, dev, feat, r[3]];
            match reply {
                Some(p) => out.extend(p),
                None => out = vec![0x11, dev, 0xFF, feat, r[3], 0x02], // HID++ 2.0 error: unsupported
            }
            out.resize(20, 0);
            q.push_back(out);
            true
        }
        fn recv(&self, buf: &mut [u8], _: i32) -> usize {
            match self.queue.borrow_mut().pop_front() {
                Some(r) => {
                    buf[..r.len()].copy_from_slice(&r);
                    r.len()
                }
                None => 0,
            }
        }
    }

    fn mouse() -> Fake {
        Fake {
            dev: 1,
            features: vec![(NAME, 3), (UNIFIED_BATTERY, 6)],
            replies: vec![
                ((3, 0), vec![19]),
                ((3, 1), b"PRO X SUPERLIGHT 2\0".to_vec()),
                ((3, 2), vec![3]),
                ((6, 1), vec![76, 4, 1, 1]),
            ],
            queue: RefCell::default(),
        }
    }

    #[test]
    fn reads_name_type_and_battery_past_other_traffic() {
        let m = mouse();
        assert_eq!(name_and_type(&m, 1), Some(("PRO X SUPERLIGHT 2".to_string(), 3)));
        assert_eq!(kind_name(3), "mouse");
        assert_eq!(battery(&m, 1), Some(Battery { percent: 76, charging: true }));
    }

    #[test]
    fn empty_slots_and_missing_features_are_none() {
        let m = mouse();
        assert_eq!(name_and_type(&m, 2), None); // no device paired in slot 2
        let no_battery = Fake { features: vec![(NAME, 3)], ..mouse() };
        assert_eq!(battery(&no_battery, 1), None);
    }

    #[test]
    fn older_battery_features() {
        let status = Fake { features: vec![(BATTERY_STATUS, 4)], replies: vec![((4, 0), vec![50, 20, 0])], ..mouse() };
        assert_eq!(battery(&status, 1), Some(Battery { percent: 50, charging: false }));
        let volts = Fake { features: vec![(BATTERY_VOLTAGE, 5)], replies: vec![((5, 0), vec![0x0F, 0x1B, 0x80])], ..mouse() }; // 3867 mV
        assert_eq!(battery(&volts, 1), Some(Battery { percent: 61, charging: true }));
    }

    #[test]
    fn voltage_curve() {
        assert_eq!(voltage_percent(4200), 100);
        assert_eq!(voltage_percent(3811), 50);
        assert_eq!(voltage_percent(3400), 0);
        assert!((55..=65).contains(&voltage_percent(3870)));
    }
}
