//! Linux sources for the shared watchers in `system.rs`: not implemented yet.

use std::time::Duration;

pub const KEYLOCK_POLL: Duration = Duration::from_millis(100);

pub fn caps_num() -> (bool, bool) {
    (false, false)
}

pub fn plugged_in() -> bool {
    false
}

pub fn accent_color() -> Option<String> {
    None
}
