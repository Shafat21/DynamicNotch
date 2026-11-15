//! Dictation commands for the Settings window / island. Webview input is validated here; the cloud key
//! is write-only (never returned).

use super::settings::{apply_patch, parse_hotkey, Settings};
use super::{audio, bind, cloud, models, send, Msg};
use crate::AppState;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, Manager};

type Res<T> = Result<T, String>;

#[tauri::command]
pub fn dictation_settings(app: AppHandle) -> Settings {
    Settings::load(&app.state::<AppState>().db.lock().unwrap())
}

/// async: may enumerate microphones (slow on some drivers).
#[tauri::command]
pub async fn set_dictation_settings(app: AppHandle, patch: Value) -> Res<Settings> {
    let cur = dictation_settings(app.clone());
    let mut next = apply_patch(&cur, &patch, audio::list_mics)?;
    if next.hotkey != cur.hotkey {
        if let Err(e) = bind(&app, &parse_hotkey(&next.hotkey)?) {
            let _ = parse_hotkey(&cur.hotkey).map(|old| bind(&app, &old));
            return Err(e);
        }
    }
    next.save(&app.state::<AppState>().db.lock().unwrap())?;
    Ok(next)
}

/// Resolves when the model is installed; progress goes out as `model-progress` {name, received, total}.
#[tauri::command]
pub async fn download_model(app: AppHandle, name: String) -> Res<()> {
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = 0u64;
        models::download(&app, &name, |received, total| {
            let pct = received * 100 / total.max(1);
            if pct != last || received == total {
                last = pct;
                let _ = app.emit("model-progress", json!({"name": name, "received": received, "total": total}));
            }
        })
        .map(|_| ())
    })
    .await
    .map_err(|e| e.to_string())?
}

/// {tiny: {installed, size}, base: …, small: …}
#[tauri::command]
pub fn model_status(app: AppHandle) -> Value {
    models::MODELS.iter().map(|m| (m.name.to_string(), json!({"installed": models::installed(&app, m.name), "size": m.size}))).collect::<serde_json::Map<_, _>>().into()
}

#[tauri::command]
pub fn set_cloud_key(provider: String, key: String) -> Res<()> {
    cloud::set_key(&provider, &key)
}

#[tauri::command]
pub fn clear_cloud_key(provider: String) -> Res<()> {
    cloud::clear_key(&provider)
}

#[tauri::command]
pub fn has_cloud_key(provider: String) -> bool {
    cloud::has_key(&provider)
}

#[tauri::command]
pub async fn list_mics() -> Vec<String> {
    audio::list_mics()
}

#[tauri::command]
pub fn dictation_cancel() {
    send(Msg::Cancel);
}
