//! Dictation commands for the Settings window / island. Webview input is validated here; the cloud key
//! is write-only (never returned).

use super::settings::{apply_patch, parse_hotkey, Settings};
use super::text::{self, Entry, Stats, Words};
use super::{audio, bind, cleanup, cloud, local, models, send, Msg};
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
    if next.speech != cur.speech && next.engine != "cloud" {
        if let Some(d) = models::chosen(&app, &next.speech).and_then(|m| models::dir(&app, m).ok()) {
            local::preload(d);
        }
    }
    Ok(next)
}

/// Resolves when the offline model is installed; progress goes out as `model-progress` {name, received, total}.
/// `model`: "multilingual" (Omnilingual) or anything else / none = Parakeet.
/// A successful download switches a stored "cloud" engine to "auto", so the offline model is used.
#[tauri::command]
pub async fn download_model(app: AppHandle, model: Option<String>) -> Res<()> {
    let m = if model.as_deref() == Some("multilingual") { &models::MULTILINGUAL } else { &models::PARAKEET };
    let a = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut last = u64::MAX;
        models::download(&a, m, |received, total| {
            let pct = received * 100 / total.max(1);
            if pct != last || received == total {
                last = pct;
                let _ = a.emit("model-progress", json!({"name": m.id, "received": received, "total": total}));
            }
        })
    })
    .await
    .map_err(|e| e.to_string())??;
    let db = app.state::<AppState>();
    let db = db.db.lock().unwrap();
    let mut s = Settings::load(&db);
    if s.engine == "cloud" {
        s.engine = "auto".into();
        s.save(&db)?;
    }
    if let Some(m) = models::chosen(&app, &s.speech) {
        local::preload(models::dir(&app, m)?);
    }
    Ok(())
}

/// {installed, size (bytes of the whole download), id} for Parakeet, the same under `multilingual` for
/// Omnilingual, and claude (Claude Code found, for the clean-up).
#[tauri::command]
pub async fn model_status(app: AppHandle) -> Value {
    let one = |m: &models::Model| json!({"installed": models::installed(&app, m), "size": models::total_size(m), "id": m.id});
    let mut v = one(&models::PARAKEET);
    v["multilingual"] = one(&models::MULTILINGUAL);
    v["claude"] = json!(cleanup::exe().is_some());
    v
}

#[tauri::command]
pub fn dictation_words(app: AppHandle) -> Words {
    super::load_words(&app)
}

#[tauri::command]
pub fn set_dictation_words(app: AppHandle, words: Words) -> Res<Words> {
    let w = words.validated()?;
    let json = serde_json::to_string(&w).map_err(|e| e.to_string())?;
    app.state::<AppState>().db.lock().unwrap().set("dictation_words", &json).map_err(|e| e.to_string())?;
    Ok(w)
}

/// Newest first, at most 200; `query` filters on text or app.
#[tauri::command]
pub fn dictation_history(app: AppHandle, query: String) -> Res<Vec<Entry>> {
    let q: String = query.chars().take(200).collect();
    app.state::<AppState>().db.lock().unwrap().dictations(q.trim(), 200).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_dictation(app: AppHandle, id: i64) -> Res<()> {
    app.state::<AppState>().db.lock().unwrap().delete_dictation(id).map_err(|e| e.to_string())
}

/// Words dictated, words/min, days in a row, top apps — from the local history.
#[tauri::command]
pub fn dictation_stats(app: AppHandle) -> Res<Stats> {
    use chrono::{Datelike, Local, TimeZone};
    let rows = app.state::<AppState>().db.lock().unwrap().dictation_rows().map_err(|e| e.to_string())?;
    let day = |ms: i64| Local.timestamp_millis_opt(ms).single().map_or(0, |t| t.date_naive().num_days_from_ce() as i64);
    let rows: Vec<_> = rows.into_iter().map(|(w, ms, at, a)| (w, ms, day(at), a)).collect();
    Ok(text::stats(&rows, day(chrono::Utc::now().timestamp_millis())))
}

/// Copy a history entry (lands in clipboard history like any copy).
#[tauri::command]
pub fn dictation_copy(text: String) -> Res<()> {
    super::copy(&text)
}

/// Keyring calls may block (e.g. a Secret Service unlock prompt): run them off the main thread.
async fn blocking<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Res<T> {
    tauri::async_runtime::spawn_blocking(f).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_cloud_key(provider: String, key: String) -> Res<()> {
    blocking(move || cloud::set_key(&provider, &key)).await?
}

#[tauri::command]
pub async fn clear_cloud_key(provider: String) -> Res<()> {
    blocking(move || cloud::clear_key(&provider)).await?
}

#[tauri::command]
pub async fn has_cloud_key(provider: String) -> Res<bool> {
    blocking(move || cloud::has_key(&provider)).await
}

#[tauri::command]
pub async fn list_mics() -> Vec<String> {
    audio::list_mics()
}

#[tauri::command]
pub fn dictation_cancel() {
    send(Msg::Cancel);
}

/// Voice note from the island's mic button: the text comes back in the `dictation` "done" event (page: true).
#[tauri::command]
pub fn voice_note_start() {
    send(Msg::PageStart);
}

#[tauri::command]
pub fn voice_note_stop() {
    send(Msg::PageStop);
}
