//! Linux: only combos with a regular key, via tauri-plugin-global-shortcut (X11/XWayland, best effort).
//! There is no portable low-level keyboard hook for modifier-only combos.

use super::settings::Mods;

pub fn spawn() {}

pub fn set(mods: Option<Mods>) -> Result<(), String> {
    match mods {
        Some(_) => Err("Modifier-only hotkeys work on Windows only; choose a combo with a key, e.g. Ctrl+Shift+Space".into()),
        None => Ok(()),
    }
}
