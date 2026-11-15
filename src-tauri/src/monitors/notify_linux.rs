//! Linux notifications (D-Bus): not implemented yet.

use tauri::AppHandle;

pub fn spawn(_app: AppHandle) {
    eprintln!("notification monitor: not implemented on Linux yet");
}
