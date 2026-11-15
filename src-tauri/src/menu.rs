//! Right-click menu (native popup). Item ids: "anim:<style>", "style:<style>", "display:<name|primary>",
//! "size:<size>", "autostart", "location", "claude", "quit".

use crate::{bridge, db, display, emit_settings, AppState};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Emitter, Manager, WebviewWindow, Wry};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = crate::current_settings(app).map_err(|e| tauri::Error::Io(std::io::Error::other(e)))?;
    let claude = if bridge::is_connected_shown(app) { "Disconnect Claude Code" } else { "Connect Claude Code" };
    let check = |id: String, label: &str, on: bool| CheckMenuItem::with_id(app, id, label, true, on, None::<&str>);

    let anim = Submenu::with_id(app, "anim", "Animation Style", true)?;
    for st in db::ANIMATION_STYLES {
        anim.append(&check(format!("anim:{st}"), st, s.animation_style == st)?)?;
    }
    let style = Submenu::with_id(app, "style", "Island Style", true)?;
    for st in db::ISLAND_STYLES {
        style.append(&check(format!("style:{st}"), st, s.island_style == st)?)?;
    }
    let screens = Submenu::with_id(app, "display", "Display", true)?;
    screens.append(&check("display:primary".into(), "Primary display", s.display == "primary")?)?;
    for (i, m) in display::monitors(app)?.1.iter().enumerate() {
        let label = format!("{}: {} ({}×{})", i + 1, m.name, m.width, m.height);
        screens.append(&check(format!("display:{}", m.name), &label, s.display == m.name)?)?;
    }
    let size = Submenu::with_id(app, "size", "Island Size", true)?;
    for sz in db::ISLAND_SIZES {
        size.append(&check(format!("size:{sz}"), sz, s.island_size == sz)?)?;
    }

    Menu::with_items(
        app,
        &[
            &anim,
            &style,
            &screens,
            &size,
            &PredefinedMenuItem::separator(app)?,
            &check("autostart".into(), "Start with Windows", s.autostart)?,
            &MenuItem::with_id(app, "location", "Change Location", true, None::<&str>)?,
            &MenuItem::with_id(app, "claude", claude, true, None::<&str>)?,
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
    } else if let Some(name) = id.strip_prefix("display:") {
        display::set_display(app, name)
    } else if let Some(sz) = id.strip_prefix("size:") {
        display::set_island_size(app, sz)
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
            "claude" => {
                let (app, on) = (app.clone(), !bridge::is_connected_shown(app));
                // the confirm dialog blocks, so not on the main (menu) thread
                std::thread::spawn(move || {
                    if let Err(e) = bridge::set_connected(&app, on) {
                        app.dialog().message(e).title("Claude Code").kind(MessageDialogKind::Error).blocking_show();
                    }
                });
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
