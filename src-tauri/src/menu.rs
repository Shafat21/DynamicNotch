//! Right-click menu (native popup). Item ids: "settings", "anim:<style>", "claude", "quit".
//! Display, size, fullscreen, autostart and location moved to the Settings window (spec §22.4).

use crate::{bridge, commands, db, emit_settings, AppState};
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager, WebviewWindow, Wry};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

pub fn build(app: &AppHandle) -> tauri::Result<Menu<Wry>> {
    let s = crate::current_settings(app).map_err(|e| tauri::Error::Io(std::io::Error::other(e)))?;
    let claude = if bridge::is_connected_shown(app) { "Disconnect Claude Code" } else { "Connect Claude Code" };
    let check = |id: String, label: &str, on: bool| CheckMenuItem::with_id(app, id, label, true, on, None::<&str>);

    let anim = Submenu::with_id(app, "anim", "Animation Style", true)?;
    for st in db::ANIMATION_STYLES {
        anim.append(&check(format!("anim:{st}"), st, s.animation_style == st)?)?;
    }

    Menu::with_items(
        app,
        &[
            &MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?,
            &PredefinedMenuItem::separator(app)?,
            &anim,
            &PredefinedMenuItem::separator(app)?,
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
    } else {
        match id {
            "settings" => {
                let app = app.clone();
                // off the menu thread, like the Claude dialog below
                std::thread::spawn(move || {
                    if let Err(e) = commands::show_settings(&app) {
                        eprintln!("open settings failed: {e}");
                    }
                });
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
