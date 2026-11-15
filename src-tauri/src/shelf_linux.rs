//! Linux (best effort, unverified): reveal = open the parent folder.

use std::path::Path;
use std::process::Command;

pub fn reveal(path: &str) -> Result<(), String> {
    let dir = Path::new(path).parent().ok_or("no parent folder")?;
    Command::new("xdg-open").arg(dir).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Not supported: the UI hides drag-out after this error and keeps Copy path / Show in folder.
pub fn drag_out(_win: &tauri::WebviewWindow, _path: &str) -> Result<(), String> {
    Err("drag-out is not supported on Linux yet".into())
}
