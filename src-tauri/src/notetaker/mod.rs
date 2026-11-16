//! Notetaker: "Record a call" captures the microphone and the computer's sound into two temporary WAV files
//! under the app data dir (max 3 h; warns, then stops, when the disk runs low). Stop → offline Parakeet
//! transcript labelled You / Them by track → optionally (setting `notetaker_claude`, off by default) a summary,
//! key points and to-dos from Claude Code (`claude -p`) → a new call note. The audio is deleted when processing
//! ends (even on a panic), on cancel, on app exit, and any leftover is swept at startup. Sticky event
//! `notetaker` {state: "idle"|"recording"|"processing"|"done"|"error", ms?, system?, warn?, stopped?, step?,
//! progress?, noteId?, error?}.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License
//! (full text: THIRD_PARTY_NOTICES.md).

pub mod build;
pub mod capture;

use crate::dictation::{self, models};
use crate::AppState;
use capture::{Source, Track};
use chrono::{DateTime, Local};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

const MAX: Duration = Duration::from_secs(3 * 3600);
/// Each track takes ~115 MB per hour (16 kHz, 16-bit), both ~230 MB/h: a 3 h call is ~690 MB.
const WARN_FREE: u64 = 1 << 30;
const STOP_FREE: u64 = 200 << 20;
/// Enough for ~45 min; the checks every 30 s stop the call before the disk fills.
const START_FREE: u64 = 2 * STOP_FREE;
const CLAUDE_KEY: &str = "notetaker_claude";

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
/// Opening the devices (REC isn't held meanwhile, so Stop/Cancel never wait on a device).
static STARTING: AtomicBool = AtomicBool::new(false);
/// A finished call is being processed.
static BUSY: AtomicBool = AtomicBool::new(false);

fn rec() -> std::sync::MutexGuard<'static, Option<Recording>> {
    REC.lock().unwrap_or_else(|e| e.into_inner())
}

fn publish(app: &AppHandle, v: Value) {
    crate::publish(app, "notetaker", v);
}

fn root(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map(|d| d.join("notetaker")).map_err(|e| e.to_string())
}

/// "Summarise with Claude" (sends the transcript to Anthropic via Claude Code): off unless turned on.
pub fn claude_enabled(db: &crate::db::Db) -> bool {
    db.get(CLAUDE_KEY).ok().flatten().as_deref() == Some("1")
}

/// Clears a flag when dropped, whatever path (error, panic) leaves the scope.
struct Clear(&'static AtomicBool);
impl Drop for Clear {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

/// The offline model dictation would use (Settings → Dictation → offline model), or the other one.
fn model_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let speech = dictation::settings::Settings::load(&app.state::<AppState>().db.lock().unwrap()).speech;
    models::dir(app, models::chosen(app, &speech).ok_or(dictation::NEED_MODEL)?)
}

pub fn start(app: &AppHandle) -> Result<(), String> {
    model_dir(app)?;
    if rec().is_some() || BUSY.load(Ordering::SeqCst) || STARTING.swap(true, Ordering::SeqCst) {
        return Err("A call is already being recorded or transcribed".into());
    }
    let _starting = Clear(&STARTING);
    let root = root(app)?;
    std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
    if capture::free_bytes(&root).is_some_and(|f| f < START_FREE) {
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
    *rec() = Some(Recording { id, mic, system, dir, t0: Instant::now(), started });
    publish(app, json!({"state": "recording", "ms": 0, "system": has_system}));
    let app = app.clone();
    std::thread::spawn(move || tick(app, id, has_system));
    Ok(())
}

/// Timer, disk, length and capture checks while recording `id`.
fn tick(app: AppHandle, id: u64, system: bool) {
    let mut warn: Option<&str> = None;
    let mut checked = Instant::now() - Duration::from_secs(60);
    loop {
        std::thread::sleep(Duration::from_millis(500));
        let (ms, dir, level, failed) = match rec().as_ref() {
            Some(r) if r.id == id => (
                r.t0.elapsed().as_millis() as u64,
                r.dir.clone(),
                r.mic.level().max(r.system.as_ref().map_or(0.0, Track::level)),
                r.mic.failed().or_else(|| r.system.as_ref().and_then(Track::failed)),
            ),
            _ => return,
        };
        let mut stop_why = (ms >= MAX.as_millis() as u64).then(|| "Recording stopped: 3 hours is the limit".to_string());
        if checked.elapsed() >= Duration::from_secs(30) {
            checked = Instant::now();
            let free = capture::free_bytes(&dir).unwrap_or(u64::MAX);
            warn = (free < WARN_FREE).then_some("Low disk space");
            if free < STOP_FREE {
                stop_why = Some("Recording stopped: the disk is almost full".into());
            }
        }
        if let Some(e) = failed {
            stop_why = Some(format!("Recording stopped: {e}")); // what was recorded so far is still transcribed
        }
        publish(&app, json!({"state": "recording", "ms": ms, "system": system, "warn": warn, "level": level}));
        if stop_why.is_some() {
            stop(&app, stop_why);
            return;
        }
    }
}

/// Deletes the call's folder and clears BUSY when processing ends, however it ends (panics included).
struct Cleanup(PathBuf);
impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
        BUSY.store(false, Ordering::SeqCst);
    }
}

/// Stop recording and process the call in the background. `stopped`: why it stopped by itself.
pub fn stop(app: &AppHandle, stopped: Option<String>) {
    let Some(r) = rec().take() else { return };
    BUSY.store(true, Ordering::SeqCst);
    publish(app, json!({"state": "processing", "step": "transcribing", "progress": 0.0, "stopped": stopped}));
    let app = app.clone();
    std::thread::spawn(move || {
        let cleanup = Cleanup(r.dir.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| process(&app, r, stopped.as_deref())))
            .unwrap_or_else(|_| Err("Processing the call failed unexpectedly".into()));
        drop(cleanup); // audio gone and not busy before the UI hears the result
        match result {
            Ok(id) => publish(&app, json!({"state": "done", "noteId": id})),
            Err(e) => publish(&app, json!({"state": "error", "error": e})),
        }
    });
}

/// Drop the recording and its audio.
pub fn cancel(app: &AppHandle) {
    let Some(r) = rec().take() else { return };
    let dir = r.dir.clone();
    drop(r); // closes the devices and files
    let _ = std::fs::remove_dir_all(&dir);
    publish(app, json!({"state": "idle"}));
}

/// Startup: no call can be recording yet, so anything in the folder is left over from a crash or a kill.
pub fn sweep(app: &AppHandle) {
    if let Ok(root) = root(app) {
        let _ = std::fs::remove_dir_all(root);
    }
}

/// App exit: drop a running recording and any audio (a call being processed is lost; its audio must go).
pub fn shutdown(app: &AppHandle) {
    cancel(app);
    sweep(app);
}

fn process(app: &AppHandle, r: Recording, stopped: Option<&str>) -> Result<i64, String> {
    let Recording { mic, system, dir, started, .. } = r;
    // a track that failed still has what it wrote: log and carry on with what's readable
    if let Err(e) = mic.finish() {
        eprintln!("notetaker: microphone track: {e}");
    }
    if let Some(s) = system {
        if let Err(e) = s.finish() {
            eprintln!("notetaker: computer sound track: {e}");
        }
    }
    let claude = claude_enabled(&app.state::<AppState>().db.lock().unwrap());
    let stopped = stopped.map(String::from);
    let save = &mut |body: &str| app.state::<AppState>().db.lock().unwrap().add_call_note(body).map_err(|e| e.to_string());
    process_dir(&model_dir(app)?, &dir, started, claude, &mut |p| {
        publish(app, json!({"state": "processing", "step": "transcribing", "progress": p, "stopped": stopped}));
    }, &mut || publish(app, json!({"state": "processing", "step": "summarizing", "progress": 1.0, "stopped": stopped})), save)
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
    let _cleanup = Cleanup(dir.to_path_buf());
    make_note(model, dir, started, claude, progress, summarizing).and_then(|body| save(&body))
}

fn make_note(model: &Path, dir: &Path, started: DateTime<Local>, claude: bool, progress: &mut dyn FnMut(f32), summarizing: &mut dyn FnMut()) -> Result<String, String> {
    let read = |name: &str| {
        let p = dir.join(name);
        if !p.exists() {
            return Vec::new();
        }
        capture::read_track(&p).unwrap_or_else(|e| {
            eprintln!("notetaker: {e}");
            Vec::new()
        })
    };
    let (mic, sys) = (read("mic.wav"), read("system.wav"));
    let lines = build::transcribe_call(model, &mic, &sys, progress)?;
    if lines.is_empty() {
        return Err("No speech detected in the call".into());
    }
    let summary = if claude {
        summarizing();
        build::summarize(&lines)
    } else {
        Err("Summarise with Claude is off".into())
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

/// Off the main thread: stopping joins the audio threads.
#[tauri::command]
pub async fn notetaker_stop(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || stop(&app, None)).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn notetaker_cancel(app: AppHandle) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || cancel(&app)).await.map_err(|e| e.to_string())
}

/// {claude: the "Summarise with Claude" toggle, claudeFound: Claude Code is installed}
#[tauri::command]
pub fn notetaker_settings(app: AppHandle) -> Value {
    json!({"claude": claude_enabled(&app.state::<AppState>().db.lock().unwrap()), "claudeFound": dictation::cleanup::exe().is_some()})
}

#[tauri::command]
pub fn set_notetaker_claude(app: AppHandle, on: bool) -> Result<(), String> {
    app.state::<AppState>().db.lock().unwrap().set(CLAUDE_KEY, if on { "1" } else { "0" }).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claude_summaries_are_off_until_turned_on() {
        let db = crate::db::Db::open_in_memory().unwrap();
        assert!(!claude_enabled(&db));
        db.set(CLAUDE_KEY, "1").unwrap();
        assert!(claude_enabled(&db));
    }

    #[test]
    fn processing_always_deletes_the_folder_and_clears_busy() {
        let dir = std::env::temp_dir().join(format!("dn-call-fail-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("mic.wav"), b"not a wav").unwrap();
        BUSY.store(true, Ordering::SeqCst);
        // unreadable audio → no speech → error, and still cleaned up
        let r = process_dir(Path::new("Z:/no/model"), &dir, Local::now(), false, &mut |_| {}, &mut || {}, &mut |_| Ok(1));
        assert!(r.is_err() && !dir.exists() && !BUSY.load(Ordering::SeqCst));
        // a panic while saving: the guard still runs
        std::fs::create_dir_all(&dir).unwrap();
        BUSY.store(true, Ordering::SeqCst);
        let caught = std::panic::catch_unwind(|| {
            let _c = Cleanup(dir.clone());
            panic!("boom");
        });
        assert!(caught.is_err() && !dir.exists() && !BUSY.load(Ordering::SeqCst));
    }

    /// The whole Stop path on two real WAVs (16-bit PCM, e.g. Windows SAPI) with the default settings (Claude
    /// off): offline Parakeet, the note saved in a DB, the audio folder deleted. DN_CALL_CLAUDE=1 turns Claude on.
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
        if std::env::var("DN_CALL_CLAUDE").as_deref() == Ok("1") {
            db.set(CLAUDE_KEY, "1").unwrap();
        }
        let claude = claude_enabled(&db);
        let t0 = Instant::now();
        let mut steps = Vec::new();
        let mut asked = false;
        let id = process_dir(&model, &dir, Local::now(), claude, &mut |p| steps.push((p * 100.0).round()), &mut || asked = true, &mut |b| db.add_call_note(b).map_err(|e| e.to_string())).unwrap();
        println!("progress % {steps:?}; claude setting: {claude}, asked: {asked}; took {:?}", t0.elapsed());
        assert!(!dir.exists(), "audio folder left behind");
        let note = db.notes("").unwrap().into_iter().find(|n| n.id == id).unwrap();
        assert!(note.call);
        println!("=== NOTE ===\n{}\n=== END ===", note.body);
        assert!(note.body.contains("You: ") && note.body.contains("Them: ") && note.body.contains("\nTranscript\n"));
    }
}
