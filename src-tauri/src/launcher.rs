//! User-picked apps: launch by DB record and extract icons. The public API is platform-neutral;
//! OS code lives in `launcher_{windows,linux}.rs`.

use crate::db::App;
use std::path::Path;

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() {
                out.push(B64[(n >> (18 - 6 * i) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

pub fn launch(app: &App) -> Result<(), String> {
    match app.kind.as_str() {
        "action" => crate::actions::run(&app.target),
        "path" => {
            if !Path::new(&app.target).exists() {
                return Err(format!("{} not found: {}", app.name, app.target));
            }
            os::shell_open(&app.target)
        }
        k => Err(format!("unknown app kind: {k}")),
    }
}

/// `data:image/png;base64,...` of the file's icon, or None if it can't be extracted.
pub fn icon_data_uri(path: &Path) -> Option<String> {
    os::icon_png_uri(path)
}

#[cfg(windows)]
#[path = "launcher_windows.rs"]
pub(crate) mod os;
#[cfg(target_os = "linux")]
#[path = "launcher_linux.rs"]
pub(crate) mod os;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_rfc4648() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
    #[test]
    fn missing_file_is_an_error_not_a_panic() {
        let app = App { id: 1, name: "x".into(), kind: "path".into(), target: r"C:\nope\R&D tools\gone.exe".into(), icon: None };
        assert!(launch(&app).unwrap_err().contains("not found"));
    }
    #[test]
    fn unknown_action_and_kind_rejected() {
        let mk = |kind: &str, target: &str| App { id: 1, name: "x".into(), kind: kind.into(), target: target.into(), icon: None };
        assert!(launch(&mk("action", "calc & del *")).is_err());
        assert!(launch(&mk("script", "x")).is_err());
    }
}
