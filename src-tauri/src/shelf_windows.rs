//! Windows: reveal in Explorer.

use std::os::windows::process::CommandExt;
use std::process::Command;

/// `/select,"path"` must be ONE argument with the quotes inside it, or Explorer splits at spaces.
pub fn reveal_arg(path: &str) -> String {
    format!("/select,\"{path}\"")
}

pub fn reveal(path: &str) -> Result<(), String> {
    Command::new("explorer.exe").raw_arg(reveal_arg(path)).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Start a native file drag (the left button is still down). Blocks until the drop / cancel.
pub fn drag_out(win: &tauri::WebviewWindow, path: &str) -> Result<(), String> {
    drag::start_drag(win, drag::DragItem::Files(vec![path.into()]), drag::Image::Raw(Vec::new()), |_, _| {}, drag::Options::default()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    #[test]
    fn reveal_arg_keeps_spaces_in_one_quoted_arg() {
        assert_eq!(super::reveal_arg(r"C:\a b\c d.txt"), r#"/select,"C:\a b\c d.txt""#);
    }
}
