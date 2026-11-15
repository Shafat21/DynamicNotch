//! Linux (best effort, X11/XWayland): poll the clipboard text every second.

use arboard::Clipboard;
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;

/// Kept alive after a write: on X11 the content disappears when its owner is dropped.
static WRITER: Mutex<Option<Clipboard>> = Mutex::new(None);

pub fn spawn(_app: AppHandle, on_text: impl Fn(String) + Send + 'static) {
    std::thread::spawn(move || {
        let mut last: Option<String> = None;
        loop {
            std::thread::sleep(Duration::from_secs(1));
            let Ok(mut cb) = Clipboard::new() else { continue };
            if let Ok(t) = cb.get_text() {
                if last.as_deref() != Some(t.as_str()) {
                    last = Some(t.clone());
                    on_text(t);
                }
            }
        }
    });
}

pub fn write(text: &str) -> Result<(), String> {
    let mut w = WRITER.lock().unwrap();
    let cb = match w.as_mut() {
        Some(c) => c,
        None => w.insert(Clipboard::new().map_err(|e| e.to_string())?),
    };
    cb.set_text(text.to_string()).map_err(|e| e.to_string())
}
