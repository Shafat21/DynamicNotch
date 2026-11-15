//! Right-click menu (native popup). Item ids: "anim:<style>", "style:<style>", "autostart", "location", "quit".

use crate::{db, emit_settings, AppState};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Wry};
use tauri_plugin_autostart::ManagerExt;

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = crate::current_settings(app).map_err(|e| tauri::Error::Io(std::io::Error::other(e)))?;
    let check = |id: String, label: &str, on: bool| CheckMenuItem::with_id(app, id, label, true, on, None::<&str>);

    let anim = Submenu::with_id(app, "anim", "Animation Style", true)?;
    for st in db::ANIMATION_STYLES {
        anim.append(&check(format!("anim:{st}"), st, s.animation_style == st)?)?;
    }
    let style = Submenu::with_id(app, "style", "Island Style", true)?;
    for st in db::ISLAND_STYLES {
        style.append(&check(format!("style:{st}"), st, s.island_style == st)?)?;
    }

    Menu::with_items(
        app,
        &[
            &anim,
            &style,
            &PredefinedMenuItem::separator(app)?,
            &check("autostart".into(), "Start with Windows", s.autostart)?,
            &MenuItem::with_id(app, "location", "Change Location", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?,
        ],
    )
}

pub fn popup(app: &AppHandle, win: &WebviewWindow) -> tauri::Result<()> {
    win.popup_menu(&build(app)?)
}

pub fn handle(app: &AppHandle, id: &str) {
    let state = app.state::<AppState>();
    let result = if let Some(st) = id.strip_prefix("anim:").filter(|s| db::ANIMATION_STYLES.contains(s)) {
        state.db.lock().unwrap().set("animation_style", st).map_err(|e| e.to_string())
    } else if let Some(st) = id.strip_prefix("style:").filter(|s| db::ISLAND_STYLES.contains(s)) {
        state.db.lock().unwrap().set("island_style", st).map_err(|e| e.to_string())
    } else {
        match id {
            "autostart" => {
                let al = app.autolaunch();
                let r = if al.is_enabled().unwrap_or(false) { al.disable() } else { al.enable() };
                r.map_err(|e| e.to_string())
            }
            "location" => {
                let _ = app.emit("edit-location", ());
                Ok(())
            }
            "quit" => {
                app.exit(0);
                Ok(())
            }
            _ => Ok(()),
        }
    };
    if let Err(e) = result {
        eprintln!("menu action {id} failed: {e}");
    }
    emit_settings(app);
}
