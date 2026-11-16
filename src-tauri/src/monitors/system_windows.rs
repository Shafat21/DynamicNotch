//! Win32 sources for the shared watchers in `system.rs`.

use std::time::Duration;
use windows::UI::ViewManagement::{UIColorType, UISettings};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VIRTUAL_KEY, VK_CAPITAL, VK_NUMLOCK};

pub const KEYLOCK_POLL: Duration = Duration::from_millis(20);

fn lock_on(vk: VIRTUAL_KEY) -> bool {
    // Low bit of GetKeyState = toggle state.
    unsafe { GetKeyState(vk.0 as i32) & 1 != 0 }
}

/// (Caps Lock, Num Lock)
pub fn caps_num() -> (bool, bool) {
    (lock_on(VK_CAPITAL), lock_on(VK_NUMLOCK))
}

pub fn plugged_in() -> bool {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe { GetSystemPowerStatus(&mut s).is_ok() && s.ACLineStatus == 1 }
}

/// (ACLineStatus, BatteryLifePercent); (255, 255) when Windows can't say.
pub fn power_raw() -> (u8, u8) {
    let mut s = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut s) }.is_ok() { (s.ACLineStatus, s.BatteryLifePercent) } else { (255, 255) }
}

pub fn accent_color() -> Option<String> {
    let c = UISettings::new().ok()?.GetColorValue(UIColorType::Accent).ok()?;
    Some(format!("#{:02x}{:02x}{:02x}", c.R, c.G, c.B))
}
