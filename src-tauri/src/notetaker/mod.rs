//! Notetaker: "Record a call" captures the microphone and the computer's sound into two WAV files under the
//! app data dir (max 3 h; warns, then stops, when the disk runs low). Stop → offline Parakeet transcript
//! labelled You / Them by track → summary, key points and to-dos from Claude Code (`claude -p`; transcript
//! only when it's unavailable) → a new call note. The audio files are deleted when processing ends and on
//! cancel. Sticky event `notetaker` {state: "idle"|"recording"|"processing"|"done"|"error", ms?, system?,
//! warn?, step?, progress?, noteId?, error?}.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License.

pub mod build;
pub mod capture;

use crate::dictation::{self, models};
use crate::AppState;
use capture::{Source, Track};
use chrono::{DateTime, Local};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const MAX: Duration = Duration::from_secs(3 * 3600);
/// Both tracks together take ~115 MB per hour.
const WARN_FREE: u64 = 1 << 30;
const STOP_FREE: u64 = 200 << 20;

struct Recording {
    id: u64,
    mic: Track,
    system: Option<Track>,
    dir: PathBuf,
    t0: Instant,
    started: DateTime<Local>,
}

static REC: Mutex<Option<Recording>> = Mutex::new(None);
static IDS: AtomicU64 = AtomicU64::new(0);
static BUSY: AtomicBool = AtomicBool::new(false);

fn publish(app: &AppHandle, v: serde_json::Value) {
    crate::publish(app, "notetaker", v);
}

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map(|d| d.join("notetaker")).map_err(|e| e.to_string())
}

pub fn start(app: &AppHandle) -> Result<(), String> {
    if !models::installed(app) {
        return Err(dictation::NEED_MODEL.into());
    }
    let mut slot = REC.lock().unwrap();
    if slot.is_some() || BUSY.load(Ordering::SeqCst) {
        return Err("A call is already being recorded or transcribed".into());
    }
    let root = root(app)?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if capture::free_bytes(&root).is_some_and(|f| f < 2 * STOP_FREE) {
        return Err("Not enough free disk space to record a call".into());
    }
    let started = Local::now();
    let dir = root.join(started.format("%Y%m%d-%H%M%S").to_string());
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mic_name = dictation::settings::Settings::load(&app.state::<AppState>().db.lock().unwrap()).mic;
    let mic = match Track::start(Source::Mic(mic_name), dir.join("mic.wav")) {
        Ok(t) => t,
        Err(e) => {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
    };
    // a call without the computer's sound still has your side
    let system = Track::start(Source::System, dir.join("system.wav")).map_err(|e| eprintln!("notetaker: computer sound not recorded: {e}")).ok();
    let id = IDS.fetch_add(1, Ordering::SeqCst) + 1;
    let has_system = system.is_some();
    *slot = Some(Recording { id, mic, system, dir, t0: Instant::now(), started });
    drop(slot);
    publish(app, json!({"state": "recording", "ms": 0, "system": has_system}));
    let app = app.clone();
    std::thread::spawn(move || tick(app, id, has_system));
    Ok(())
}

/// Timer, disk and length checks while recording `id`.
fn tick(app: AppHandle, id: u64, system: bool) {
    let mut warn: Option<&str> = None;
    let mut checked = Instant::now() - Duration::from_secs(60);
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let (ms, dir, level) = match REC.lock().unwrap().as_ref() {
            Some(r) if r.id == id => (r.t0.elapsed().as_millis() as u64, r.dir.clone(), r.mic.level().max(r.system.as_ref().map_or(0.0, Track::level))),
            _ => return,
        };
        let mut stop_now = ms >= MAX.as_millis() as u64;
        if checked.elapsed() >= Duration::from_secs(30) {
            checked = Instant::now();
            let free = capture::free_bytes(&dir).unwrap_or(u64::MAX);
            warn = (free < WARN_FREE).then_some("Low disk space");
            stop_now |= free < STOP_FREE;
        }
        publish(&app, json!({"state": "recording", "ms": ms, "system": system, "warn": warn, "level": level}));
        if stop_now {
            stop(&app);
            return;
        }
    }
}

/// Stop recording and process the call in the background.
pub fn stop(app: &AppHandle) {
    let Some(rec) = REC.lock().unwrap().take() else { return };
    BUSY.store(true, Ordering::SeqCst);
    publish(app, json!({"state": "processing", "step": "transcribing", "progress": 0.0}));
    let app = app.clone();
    std::thread::spawn(move || {
        let dir = rec.dir.clone();
        let result = process(&app, rec);
        let _ = std::fs::remove_dir_all(&dir); // the audio never outlives processing
        BUSY.store(false, Ordering::SeqCst);
        match result {
            Ok(id) => publish(&app, json!({"state": "done", "noteId": id})),
            Err(e) => publish(&app, json!({"state": "error", "error": e})),
        }
    });
}

/// Drop the recording and its audio.
pub fn cancel(app: &AppHandle) {
    let Some(rec) = REC.lock().unwrap().take() else { return };
    let dir = rec.dir.clone();
    drop(rec); // closes the devices and files
    let _ = std::fs::remove_dir_all(&dir);
    publish(app, json!({"state": "idle"}));
}

fn process(app: &AppHandle, rec: Recording) -> Result<i64, String> {
    let Recording { mic, system, dir, started, .. } = rec;
    mic.finish()?;
    if let Some(s) = system {
        if let Err(e) = s.finish() {
            eprintln!("notetaker: computer sound track: {e}");
        }
    }
    let save = &mut |body: &str| app.state::<AppState>().db.lock().unwrap().add_call_note(body).map_err(|e| e.to_string());
    process_dir(&models::dir(app)?, &dir, started, true, &mut |p| {
        publish(app, json!({"state": "processing", "step": "transcribing", "progress": p}));
    }, &mut || publish(app, json!({"state": "processing", "step": "summarizing", "progress": 1.0})), save)
}

/// The recordings in `dir` (mic.wav, optional system.wav) → a saved note. `dir` is deleted whatever happens.
pub fn process_dir(
    model: &Path,
    dir: &Path,
    started: DateTime<Local>,
    claude: bool,
    progress: &mut dyn FnMut(f32),
    summarizing: &mut dyn FnMut(),
    save: &mut dyn FnMut(&str) -> Result<i64, String>,
) -> Result<i64, String> {
    let r = make_note(model, dir, started, claude, progress, summarizing).and_then(|body| save(&body));
    let _ = std::fs::remove_dir_all(dir);
    r
}

fn make_note(model: &Path, dir: &Path, started: DateTime<Local>, claude: bool, progress: &mut dyn FnMut(f32), summarizing: &mut dyn FnMut()) -> Result<String, String> {
    let mic = capture::read_wav(&dir.join("mic.wav"))?;
    let sys = dir.join("system.wav");
    let sys = if sys.exists() { capture::read_wav(&sys).unwrap_or_default() } else { Vec::new() };
    let lines = build::transcribe_call(model, &mic, &sys, progress)?;
    if lines.is_empty() {
        return Err("No speech detected in the call".into());
    }
    let summary = if claude {
        summarizing();
        build::summarize(&lines)
    } else {
        Err("Claude Code not asked".into())
    };
    if let Err(e) = &summary {
        eprintln!("notetaker: no summary: {e}");
    }
    Ok(build::compose(started, &lines, summary.as_ref().map_err(|e| e.as_str()), crate::notes::MAX_BODY))
}

#[tauri::command]
pub async fn notetaker_start(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || start(&app)).await.map_err(|e| e.to_string())?
}

#[tauri::command]
pub fn notetaker_stop(app: AppHandle) {
    stop(&app);
}

#[tauri::command]
pub fn notetaker_cancel(app: AppHandle) {
    cancel(&app);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole Stop path on two real WAVs (16-bit PCM, e.g. Windows SAPI): offline Parakeet, the note saved
    /// in a DB, the audio folder deleted. Claude Code is asked too when DN_CALL_CLAUDE=1.
    /// `DN_CALL_MIC=… DN_CALL_SYS=… cargo test --lib real_call_end_to_end -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn real_call_end_to_end() {
        let dir = std::env::temp_dir().join(format!("dn-call-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::copy(std::env::var("DN_CALL_MIC").unwrap(), dir.join("mic.wav")).unwrap();
        std::fs::copy(std::env::var("DN_CALL_SYS").unwrap(), dir.join("system.wav")).unwrap();
        let model = PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.dynamicnotch.app").join("models").join(models::ID);
        let db = crate::db::Db::open_in_memory().unwrap();
        let claude = std::env::var("DN_CALL_CLAUDE").as_deref() == Ok("1");
        let t0 = Instant::now();
        let mut steps = Vec::new();
        let mut asked = false;
        let id = process_dir(&model, &dir, Local::now(), claude, &mut |p| steps.push((p * 100.0).round()), &mut || asked = true, &mut |b| db.add_call_note(b).map_err(|e| e.to_string())).unwrap();
        println!("progress % {steps:?}; claude asked: {asked}; took {:?}", t0.elapsed());
        assert!(!dir.exists(), "audio folder left behind");
        let note = db.notes("").unwrap().into_iter().find(|n| n.id == id).unwrap();
        assert!(note.call);
        println!("=== NOTE ===
{}
=== END ===", note.body);
        assert!(note.body.contains("You: ") && note.body.contains("Them: ") && note.body.contains("
Transcript
"));
    }
}
