//! Notification monitor. Event: `notify`. OS-specific listeners live in `notify_{windows,linux}.rs`.

use serde::Serialize;

#[cfg(windows)]
#[path = "notify_windows.rs"]
mod os;
#[cfg(target_os = "linux")]
#[path = "notify_linux.rs"]
mod os;
pub use os::spawn;

#[derive(Serialize, Clone)]
struct Notification {
    app: String,
    title: String,
    text: String,
}
