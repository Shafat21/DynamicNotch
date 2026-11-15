//! Linux launcher: not implemented yet.

use std::path::Path;

pub fn shell_open(_path: &str) -> Result<(), String> {
    Err("not implemented on Linux yet".into())
}

pub fn icon_png_uri(_path: &Path) -> Option<String> {
    None
}
