//! Linux: no-op until Task 16 (spec §13.4) adds the compositor blur.

use super::Shape;
use tauri::AppHandle;

pub fn set_enabled(_app: &AppHandle, _on: bool, _tint: (u8, u8, u8)) {}

pub fn animate_to(_app: &AppHandle, _to: Shape) {}

pub fn refresh(_app: &AppHandle) {}
