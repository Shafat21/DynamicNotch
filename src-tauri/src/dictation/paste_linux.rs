//! Linux: key injection isn't allowed on GNOME Wayland, so dictation copies and the island says "Press Ctrl+V".

/// Ok(false) = copied only.
pub fn paste(text: &str) -> Result<bool, String> {
    super::copy(text)?;
    Ok(false)
}

/// The program in front; not known on Linux yet (history shows no app).
pub fn foreground_app() -> Option<String> {
    None
}
