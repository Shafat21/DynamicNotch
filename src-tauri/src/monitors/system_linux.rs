//! Linux sources for the shared watchers in `system.rs`: not implemented yet.

use std::time::Duration;

pub const KEYLOCK_POLL: Duration = Duration::from_millis(100);

pub fn caps_num() -> (bool, bool) {
    (false, false)
}

pub fn plugged_in() -> bool {
    false
}

/// (AC online 0/1, battery percent) from /sys/class/power_supply, in Windows' encoding; 255 = unknown.
pub fn power_raw() -> (u8, u8) {
    let read = |p: std::path::PathBuf| std::fs::read_to_string(p).ok().map(|s| s.trim().to_string());
    let (mut ac, mut pct) = (255u8, 255u8);
    for e in std::fs::read_dir("/sys/class/power_supply").into_iter().flatten().flatten() {
        match read(e.path().join("type")).as_deref() {
            Some("Mains") => ac = if read(e.path().join("online")).as_deref() == Some("1") { 1 } else { 0 },
            Some("Battery") => pct = read(e.path().join("capacity")).and_then(|c| c.parse().ok()).unwrap_or(255),
            _ => {}
        }
    }
    (ac, pct)
}

pub fn accent_color() -> Option<String> {
    None
}
