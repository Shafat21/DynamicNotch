//! Linux window placement / click-through / hover: not implemented yet.

use super::Rect;
use tauri::{AppHandle, WebviewWindow};

pub fn setup(_app: &AppHandle, win: &WebviewWindow) -> tauri::Result<()> {
    eprintln!("window setup: not implemented on Linux yet");
    win.show()
}

pub fn set_hit_rect(_app: &AppHandle, _rect: Rect) {}
