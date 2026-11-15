//! Linux: the compositor (Mutter / KDE) stacks fullscreen windows above us, so there is nothing to detect.

use tauri::AppHandle;

pub fn is_fullscreen(_app: &AppHandle) -> bool {
    false
}
