//! Linux media (MPRIS): not implemented yet.

use tauri::AppHandle;

pub fn spawn(_app: AppHandle) {
    eprintln!("media monitor: not implemented on Linux yet");
}

pub fn control(_action: &str) -> Result<(), String> {
    Err("not implemented on Linux yet".into())
}

pub fn seek(_seconds: f64) -> Result<(), String> {
    Err("not implemented on Linux yet".into())
}

pub fn open_player() -> Result<(), String> {
    Err("not implemented on Linux yet".into())
}
