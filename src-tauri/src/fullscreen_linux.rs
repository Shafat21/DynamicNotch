//! Linux: the compositor (Mutter / KDE) stacks fullscreen windows above us, so there is nothing to detect.

use tauri::AppHandle;

pub fn fullscreen_state(_app: &AppHandle, _per_monitor: bool) -> super::Fs {
    super::Fs::No
}

pub fn window_monitor(_app: &AppHandle, _label: &str) -> Option<(i32, i32, i32, i32)> {
    None
}

/// Not detected on Linux (no fullscreen detection there either).
pub fn foreground_exe() -> Option<String> {
    None
}

pub fn running_apps() -> Vec<String> {
    Vec::new()
}
