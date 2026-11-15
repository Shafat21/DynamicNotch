//! Shelf (spec §23.6): files / folders dropped on the island, kept as paths (never copied).

use serde::Serialize;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

#[cfg(windows)]
#[path = "shelf_windows.rs"]
pub(crate) mod os;
#[cfg(target_os = "linux")]
#[path = "shelf_linux.rs"]
pub(crate) mod os;

pub const CAP: usize = 100;

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ShelfItem {
    pub id: i64,
    pub path: String,
    pub name: String,
    pub is_dir: bool,
    pub exists: bool,
    pub icon: Option<String>,
}

/// Icons already extracted: per path for folders and files with their own icon, else per extension.
static ICONS: Mutex<Option<HashMap<String, Option<String>>>> = Mutex::new(None);

fn icon_key(path: &str, is_dir: bool) -> String {
    let ext = Path::new(path).extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if is_dir || ext.is_empty() || matches!(ext.as_str(), "exe" | "lnk" | "ico") {
        format!("path:{}", path.to_lowercase())
    } else {
        format!("ext:{ext}")
    }
}

fn cached_icon(path: &Path, is_dir: bool) -> Option<String> {
    let key = icon_key(&path.to_string_lossy(), is_dir);
    if let Some(hit) = ICONS.lock().unwrap().get_or_insert_with(HashMap::new).get(&key) {
        return hit.clone();
    }
    let icon = crate::launcher::icon_data_uri(path); // slow shell call: never under a lock
    ICONS.lock().unwrap().get_or_insert_with(HashMap::new).insert(key, icon.clone());
    icon
}

impl ShelfItem {
    pub fn new(id: i64, path: String) -> Self {
        let p = Path::new(&path);
        let exists = p.exists();
        let is_dir = exists && p.is_dir();
        let name = p.file_name().map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
        let icon = if exists { cached_icon(p, is_dir) } else { None };
        Self { id, name, is_dir, exists, icon, path }
    }
}

/// Same file for the shelf's duplicate check: Windows paths ignore case.
pub fn same_path(a: &str, b: &str) -> bool {
    if cfg!(windows) { a.eq_ignore_ascii_case(b) } else { a == b }
}

/// The shelf with icons. Takes the DB lock only for the rows; file checks and icon extraction run after it is released.
pub fn list(app: &tauri::AppHandle) -> Result<Vec<ShelfItem>, String> {
    use tauri::Manager;
    let rows = app.state::<crate::AppState>().db.lock().unwrap().shelf_rows().map_err(|e| e.to_string())?;
    Ok(rows.into_iter().map(|(id, path)| ShelfItem::new(id, path)).collect())
}

/// Only existing, absolute, local paths (no URLs, no network shares) go on the shelf.
pub fn validate(path: &str) -> Result<(), String> {
    let p = Path::new(path);
    if path.contains("://") || path.starts_with("\\\\") || path.starts_with("//") {
        return Err("only local files and folders can be shelved".into());
    }
    if !p.is_absolute() || !p.exists() {
        return Err(format!("not found: {path}"));
    }
    Ok(())
}

/// Drag over the island: Enter opens the Shelf tab, Drop shelves the paths. The DB and icon work runs on its own thread.
pub fn on_drag_drop(app: &tauri::AppHandle, event: &tauri::DragDropEvent) {
    use tauri::{DragDropEvent as D, Emitter, Manager};
    let phase = match event {
        D::Enter { .. } => "enter",
        D::Leave => "leave",
        D::Drop { paths, .. } => {
            let (app, paths) = (app.clone(), paths.clone());
            std::thread::spawn(move || {
                {
                    let state = app.state::<crate::AppState>();
                    let db = state.db.lock().unwrap();
                    for p in &paths {
                        if let Err(e) = p.to_str().ok_or("non-UTF-8 path".to_string()).and_then(|s| db.add_shelf(s).map_err(|e| e.to_string())) {
                            eprintln!("shelf: skipped {}: {e}", p.display());
                        }
                    }
                }
                let _ = app.emit("shelf", list(&app).unwrap_or_default());
                let _ = app.emit("shelf-drag", "drop");
            });
            return;
        }
        _ => return,
    };
    let _ = app.emit("shelf-drag", phase);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;

    fn temp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("dn-shelf-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn same_path_is_case_insensitive_on_windows_only() {
        assert!(same_path("C:/a/B.txt", "C:/a/B.txt") && !same_path("C:/a/b.txt", "C:/a/c.txt"));
        assert_eq!(same_path("C:/a/B.txt", "c:/A/b.TXT"), cfg!(windows));
    }

    #[test]
    fn icon_keys_share_by_extension_except_for_dirs_and_executables() {
        assert_eq!(icon_key("C:/x/A.PDF", false), icon_key("D:/y/b.pdf", false));
        assert_ne!(icon_key("C:/x/a.exe", false), icon_key("C:/x/b.exe", false)); // own icons
        assert_ne!(icon_key("C:/x", true), icon_key("C:/y", true));
    }

    #[test]
    fn rejects_missing_url_and_relative() {
        let d = temp("validate");
        let f = d.join("a b.txt");
        std::fs::write(&f, "x").unwrap();
        assert!(validate(f.to_str().unwrap()).is_ok() && validate(d.to_str().unwrap()).is_ok());
        for bad in [d.join("nope.txt").to_str().unwrap(), "https://example.com/a.txt", "file:///C:/Windows", r"\server\share\a.txt", "relative.txt", ""] {
            assert!(validate(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn add_dedupes_caps_and_flags_missing() {
        let d = temp("add");
        let db = Db::open_in_memory().unwrap();
        let a = d.join("a.txt");
        std::fs::write(&a, "x").unwrap();
        let id = db.add_shelf(a.to_str().unwrap()).unwrap();
        assert_eq!(db.add_shelf(a.to_str().unwrap()).unwrap(), id); // duplicate ignored
        assert!(db.add_shelf("https://example.com").is_err() && db.add_shelf(d.join("zz").to_str().unwrap()).is_err());
        let items = db.shelf().unwrap();
        assert_eq!(items.len(), 1);
        assert!(items[0].exists && !items[0].is_dir && items[0].name == "a.txt");
        let dir = db.add_shelf(d.to_str().unwrap()).unwrap();
        let it = db.shelf().unwrap();
        assert_eq!(it[0].id, dir); // newest first
        assert!(it[0].is_dir && it[0].icon.is_some()); // folders get a shell icon too
        std::fs::remove_file(&a).unwrap();
        let gone = db.shelf().unwrap().into_iter().find(|i| i.id == id).unwrap();
        assert!(!gone.exists && gone.icon.is_none()); // kept, flagged missing
        assert_eq!(db.shelf_path(id).unwrap().as_deref(), a.to_str());
        db.remove_shelf(id).unwrap();
        db.clear_shelf().unwrap();
        assert!(db.shelf().unwrap().is_empty());
        for i in 0..CAP + 5 {
            let f = d.join(format!("f{i}"));
            std::fs::write(&f, "").unwrap();
            db.add_shelf(f.to_str().unwrap()).unwrap();
        }
        let l = db.shelf().unwrap();
        assert_eq!(l.len(), CAP);
        assert_eq!(l[0].name, format!("f{}", CAP + 4)); // oldest dropped
    }
}
