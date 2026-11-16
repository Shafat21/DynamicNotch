//! Linux launcher: `.desktop` entries via `gio launch`, executables directly, everything else (files, folders,
//! https links) via `xdg-open`. App icons are not read yet (the tile shows the app's first letter).

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub fn shell_open(target: &str) -> Result<(), String> {
    let path = Path::new(target);
    let is_url = target.contains("://");
    let mut cmd = if !is_url && path.extension().is_some_and(|e| e == "desktop") {
        let mut c = Command::new("gio");
        c.args(["launch", target]);
        c
    } else if !is_url && path.is_file() && path.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0) {
        Command::new(path)
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(target);
        c
    };
    // detached: the island never waits on what it opened, and the child doesn't hold our terminal
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
    cmd.spawn().map(drop).map_err(|e| format!("Couldn't open it: {e}"))
}

pub fn icon_png_uri(_path: &Path) -> Option<String> {
    None
}
