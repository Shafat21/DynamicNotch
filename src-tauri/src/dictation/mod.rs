//! Voice dictation (spec §23.7): hold the hotkey, speak, release → transcribe (local whisper.cpp or a
//! cloud API) → paste at the cursor. Audio stays in memory. Events: `dictation`
//! {state: "recording"|"transcribing"|"done"|"error"|"idle", level?, ms?, escCancels?, text?, pasted?, error?}.
//! Works while a fullscreen app hides the island too; the UI decides what to show.

pub mod audio;
pub mod cloud;
pub mod commands;
pub mod local;
pub mod models;
pub mod settings;

#[cfg(windows)]
#[path = "hotkey_windows.rs"]
mod hotkey_os;
#[cfg(target_os = "linux")]
#[path = "hotkey_linux.rs"]
mod hotkey_os;
#[cfg(windows)]
#[path = "paste_windows.rs"]
mod paste_os;
#[cfg(target_os = "linux")]
#[path = "paste_linux.rs"]
mod paste_os;

use crate::AppState;
use serde_json::json;
use settings::{Hotkey, Settings};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub const MIN_HOLD_MS: u64 = 300;
pub const MAX_MS: u64 = 120_000;
const TICK: Duration = Duration::from_millis(66); // ~15 Hz level updates

#[derive(Debug, PartialEq)]
pub enum Msg {
    Press,
    Release,
    Cancel,
    /// Transcription job finished.
    Done(u64, Result<String, String>),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Phase {
    Idle,
    Recording { start: u64 },
    Transcribing { job: u64 },
}

#[derive(Debug, PartialEq)]
pub enum Step {
    None,
    Start,
    Discard,
    Transcribe(u64),
}

/// The press/hold/release logic, on a millisecond clock. Pure.
pub struct Machine {
    pub phase: Phase,
    jobs: u64,
}

impl Default for Machine {
    fn default() -> Self {
        Self { phase: Phase::Idle, jobs: 0 }
    }
}

impl Machine {
    pub fn press(&mut self, now: u64) -> Step {
        if self.phase != Phase::Idle {
            return Step::None;
        }
        self.phase = Phase::Recording { start: now };
        Step::Start
    }

    fn finish(&mut self) -> Step {
        self.jobs += 1;
        self.phase = Phase::Transcribing { job: self.jobs };
        Step::Transcribe(self.jobs)
    }

    pub fn release(&mut self, now: u64) -> Step {
        match self.phase {
            Phase::Recording { start } if now.saturating_sub(start) < MIN_HOLD_MS => {
                self.phase = Phase::Idle;
                Step::Discard
            }
            Phase::Recording { .. } => self.finish(),
            _ => Step::None,
        }
    }

    pub fn tick(&mut self, now: u64) -> Step {
        match self.phase {
            Phase::Recording { start } if now.saturating_sub(start) >= MAX_MS => self.finish(),
            _ => Step::None,
        }
    }

    /// Esc: drop the recording, or forget the running transcription (its result is ignored).
    pub fn cancel(&mut self) -> Step {
        let was = self.phase;
        self.phase = Phase::Idle;
        if was == Phase::Idle {
            Step::None
        } else {
            Step::Discard
        }
    }

    /// True when `job` is the transcription we're waiting for (not cancelled / stale).
    pub fn done(&mut self, job: u64) -> bool {
        if self.phase == (Phase::Transcribing { job }) {
            self.phase = Phase::Idle;
            return true;
        }
        false
    }
}

static TX: OnceLock<Sender<Msg>> = OnceLock::new();

pub fn send(msg: Msg) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(msg);
    }
}

fn emit(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit("dictation", payload);
}

/// Clipboard write that does land in clipboard history (the final text, unlike paste's temporary write).
pub fn copy(text: &str) -> Result<(), String> {
    crate::clipboard::write(text)?;
    crate::clipboard::take_suppressed(text);
    Ok(())
}

/// (Re)bind the hotkey. The global-shortcut plugin is used only by dictation, hence `unregister_all`.
pub fn bind(app: &AppHandle, hk: &Hotkey) -> Result<(), String> {
    let gs = app.global_shortcut();
    gs.unregister_all().map_err(|e| e.to_string())?;
    if hk.key.is_none() {
        return hotkey_os::set(Some(hk.mods));
    }
    hotkey_os::set(None)?;
    gs.on_shortcut(hk.shortcut().as_str(), |_, _, e| send(if e.state() == ShortcutState::Pressed { Msg::Press } else { Msg::Release }))
        .map_err(|e| format!("Couldn't register {}: {e}", hk.canonical()))
}

fn load(app: &AppHandle) -> Settings {
    Settings::load(&app.state::<AppState>().db.lock().unwrap())
}

/// Fail fast on press (before recording) when the chosen engine can't run.
fn preflight(app: &AppHandle, s: &Settings) -> Result<(), String> {
    if s.engine == "cloud" {
        cloud::get_key(&s.cloud_provider).map(|_| ())
    } else if !local::AVAILABLE {
        Err(local::UNAVAILABLE.into())
    } else if !models::installed(app, &s.model) {
        Err(format!("The {} model isn't downloaded yet — open Settings → Dictation", s.model))
    } else {
        Ok(())
    }
}

fn transcribe(app: &AppHandle, s: &Settings, samples: &[f32]) -> Result<String, String> {
    let lang = (s.language != "auto").then_some(s.language.as_str());
    if s.engine == "cloud" {
        cloud::transcribe(&s.cloud_provider, &s.cloud_model, &cloud::get_key(&s.cloud_provider)?, samples, lang)
    } else {
        local::transcribe(&models::path(app, &s.model)?, samples, lang)
    }
}

fn deliver(s: &Settings, text: &str) -> Result<bool, String> {
    if s.output == "copy" {
        copy(text).map(|_| false)
    } else {
        paste_os::paste(text)
    }
}

fn run(app: AppHandle, rx: std::sync::mpsc::Receiver<Msg>) {
    let t0 = Instant::now();
    let mut m = Machine::default();
    let mut rec: Option<(audio::Recorder, Settings)> = None;
    let mut cfg: Option<Settings> = None; // settings of the running transcription
    loop {
        let msg = rx.recv_timeout(TICK);
        let now = t0.elapsed().as_millis() as u64;
        let step = match msg {
            Ok(Msg::Press) => m.press(now),
            Ok(Msg::Release) => m.release(now),
            Ok(Msg::Cancel) => m.cancel(),
            Ok(Msg::Done(job, r)) => {
                if m.done(job) {
                    let s = cfg.take().unwrap_or_default();
                    let r = r.map(|t| t.trim().to_string()).and_then(|t| if t.is_empty() { Err("No speech detected".to_string()) } else { Ok(t) });
                    match r.and_then(|t| deliver(&s, &t).map(|pasted| (t, pasted))) {
                        Ok((text, pasted)) => emit(&app, json!({"state": "done", "text": text, "pasted": pasted})),
                        Err(e) => emit(&app, json!({"state": "error", "error": e})),
                    }
                }
                Step::None
            }
            Err(RecvTimeoutError::Timeout) => m.tick(now),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match step {
            Step::Start => {
                let s = load(&app);
                match preflight(&app, &s).and_then(|_| audio::Recorder::start(Some(s.mic.as_str()).filter(|n| !n.is_empty()))) {
                    Ok(r) => rec = Some((r, s)),
                    Err(e) => {
                        m.cancel();
                        emit(&app, json!({"state": "error", "error": e}));
                    }
                }
            }
            Step::Discard => {
                rec = None;
                cfg = None;
                emit(&app, json!({"state": "idle"}));
            }
            Step::Transcribe(job) => {
                let Some((r, s)) = rec.take() else {
                    m.done(job);
                    continue;
                };
                emit(&app, json!({"state": "transcribing"}));
                let samples = r.stop();
                cfg = Some(s.clone());
                let app = app.clone();
                std::thread::spawn(move || send(Msg::Done(job, transcribe(&app, &s, &samples))));
            }
            Step::None => {}
        }
        if let (Phase::Recording { start }, Some((r, s))) = (m.phase, &rec) {
            // Esc (any other key) cancels only through the Windows modifier-only hook
            let esc = cfg!(windows) && settings::parse_hotkey(&s.hotkey).is_ok_and(|h| h.key.is_none());
            emit(&app, json!({"state": "recording", "level": r.level(), "ms": now - start, "escCancels": esc}));
        }
    }
}

/// Start the controller thread, the keyboard hook and the saved hotkey (app setup).
pub fn spawn(app: AppHandle) {
    let (tx, rx) = channel();
    if TX.set(tx).is_err() {
        return;
    }
    if let Err(e) = app.plugin(tauri_plugin_global_shortcut::Builder::new().build()) {
        eprintln!("dictation: global-shortcut plugin failed: {e}");
    }
    hotkey_os::spawn();
    let hk = settings::parse_hotkey(&load(&app).hotkey).or_else(|_| settings::parse_hotkey(&Settings::default().hotkey)).expect("default hotkey parses");
    if let Err(e) = bind(&app, &hk) {
        eprintln!("dictation: hotkey {}: {e}", hk.canonical());
    }
    std::thread::spawn(move || run(app, rx));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_hold_is_ignored() {
        let mut m = Machine::default();
        assert_eq!(m.press(1_000), Step::Start);
        assert_eq!(m.press(1_050), Step::None); // key repeat
        assert_eq!(m.release(1_299), Step::Discard);
        assert_eq!(m.phase, Phase::Idle);
        assert_eq!(m.press(2_000), Step::Start);
        assert_eq!(m.release(2_300), Step::Transcribe(1));
        assert_eq!(m.release(2_400), Step::None);
        assert_eq!(m.press(2_500), Step::None); // busy transcribing
        assert!(!m.done(7) && m.done(1));
        assert_eq!(m.phase, Phase::Idle);
    }

    #[test]
    fn max_duration_stops_recording() {
        let mut m = Machine::default();
        m.press(0);
        assert_eq!(m.tick(MAX_MS - 1), Step::None);
        assert_eq!(m.tick(MAX_MS), Step::Transcribe(1));
        assert_eq!(m.release(MAX_MS + 500), Step::None); // the late key-up is harmless
        assert_eq!(m.tick(MAX_MS * 3), Step::None);
    }

    #[test]
    fn cancel_drops_recording_and_pending_result() {
        let mut m = Machine::default();
        assert_eq!(m.cancel(), Step::None);
        m.press(0);
        assert_eq!(m.cancel(), Step::Discard);
        assert_eq!(m.release(5_000), Step::None);
        m.press(10_000);
        assert_eq!(m.release(11_000), Step::Transcribe(1));
        assert_eq!(m.cancel(), Step::Discard);
        assert!(!m.done(1)); // result arrives after Esc: nothing is pasted
        assert_eq!(m.press(12_000), Step::Start);
    }
}
