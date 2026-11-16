//! Voice dictation (spec §23.7): hold the hotkey, speak, release → transcribe (offline Parakeet via
//! sherpa-onnx, or a cloud API) → dictionary / replacements / snippets / optional Claude clean-up → paste
//! at the cursor → history. Two quick taps lock hands-free recording until the next press; Esc cancels.
//! Voice notes (the island's mic button) run the same way but hand the text back to the page (`page`).
//! Audio stays in memory. Events: `dictation` {state: "recording"|"transcribing"|"done"|"error"|"idle",
//! level?, ms?, locked?, escCancels?, page?, text?, pasted?, inserted?, error?}; `dictation-history` after each saved one.
//! Works while a fullscreen app hides the island too; the UI decides what to show.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License
//! (full text: THIRD_PARTY_NOTICES.md).

pub mod audio;
pub mod cleanup;
pub mod cloud;
pub mod commands;
pub mod local;
pub mod models;
pub mod settings;
pub mod text;

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

/// A shorter press is a tap (maybe the first of a double tap), not a hold.
pub const MIN_HOLD_MS: u64 = 300;
/// The second tap of a double tap comes within this after the first release.
pub const DOUBLE_TAP_MS: u64 = 400;
pub const MAX_MS: u64 = 120_000;
const TICK: Duration = Duration::from_millis(66); // ~15 Hz level updates

#[derive(Debug, PartialEq)]
pub enum Msg {
    Press,
    Release,
    /// Esc from the keyboard hook: drops a recording, never a running transcription.
    Escape,
    /// The island's cancel: drops a recording or forgets the running transcription.
    Cancel,
    /// Voice note from the island's mic button: start hands-free / finish it.
    PageStart,
    PageStop,
    /// Transcription job finished (final text, after the text pipeline).
    Done(u64, Result<String, String>),
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Phase {
    Idle,
    /// Recording; `locked` = hands-free (double tap), finished by the next press.
    Recording { start: u64, locked: bool },
    /// After a short first tap: still recording, waiting for the second tap until `until`.
    TapWait { start: u64, until: u64 },
    Transcribing { job: u64 },
}

#[derive(Debug, PartialEq)]
pub enum Step {
    None,
    Start,
    /// Hands-free from now on.
    Lock,
    /// The wait for a second tap ran out unnoticed: drop that recording and start a new one.
    Restart,
    Discard,
    Transcribe(u64),
}

/// The press / hold / double-tap logic (after Coucou's KeyLogic), on a millisecond clock. Pure.
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
        match self.phase {
            Phase::Idle => {
                self.phase = Phase::Recording { start: now, locked: false };
                Step::Start
            }
            Phase::Recording { locked: true, .. } => self.finish(),
            Phase::TapWait { start, until } if now <= until => {
                self.phase = Phase::Recording { start, locked: true };
                Step::Lock
            }
            Phase::TapWait { .. } => {
                self.phase = Phase::Recording { start: now, locked: false };
                Step::Restart
            }
            Phase::Recording { .. } | Phase::Transcribing { .. } => Step::None, // key repeat / busy
        }
    }

    /// A voice note: recording hands-free from the start, finished by the next press (or `PageStop`).
    pub fn start_locked(&mut self, now: u64) -> Step {
        if self.phase != Phase::Idle {
            return Step::None;
        }
        self.phase = Phase::Recording { start: now, locked: true };
        Step::Start
    }

    fn finish(&mut self) -> Step {
        self.jobs += 1;
        self.phase = Phase::Transcribing { job: self.jobs };
        Step::Transcribe(self.jobs)
    }

    pub fn release(&mut self, now: u64) -> Step {
        match self.phase {
            Phase::Recording { start, locked: false } if now.saturating_sub(start) < MIN_HOLD_MS => {
                self.phase = Phase::TapWait { start, until: now + DOUBLE_TAP_MS };
                Step::None
            }
            Phase::Recording { locked: false, .. } => self.finish(),
            _ => Step::None,
        }
    }

    pub fn tick(&mut self, now: u64) -> Step {
        match self.phase {
            Phase::TapWait { until, .. } if now > until => {
                self.phase = Phase::Idle;
                Step::Discard
            }
            Phase::Recording { start, .. } if now.saturating_sub(start) >= MAX_MS => self.finish(),
            _ => Step::None,
        }
    }

    /// Esc: drops a recording (held, hands-free or between taps); a running transcription goes on.
    pub fn escape(&mut self) -> Step {
        match self.phase {
            Phase::Recording { .. } | Phase::TapWait { .. } => {
                self.phase = Phase::Idle;
                Step::Discard
            }
            _ => Step::None,
        }
    }

    /// The island's cancel: drop the recording, or forget the running transcription (its result is ignored).
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

/// A voice note starts only from Idle: anything else (a hotkey dictation recording, between taps or
/// transcribing) is reported so the page's mic buttons reset.
pub fn page_start(m: &mut Machine, now: u64) -> Result<Step, &'static str> {
    match m.start_locked(now) {
        Step::None => Err("Dictation is busy, try again in a moment"),
        step => Ok(step),
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

/// Dictionary, replacements and snippets (JSON in the `settings` table).
pub fn load_words(app: &AppHandle) -> text::Words {
    let v = app.state::<AppState>().db.lock().unwrap().get("dictation_words").ok().flatten();
    v.and_then(|v| serde_json::from_str(&v).ok()).unwrap_or_default()
}

pub const NEED_MODEL: &str = "Download the offline model — Settings → Features → Dictation";

/// The engine that will run ("cloud" or an offline model id), checked on press (before recording) so it
/// fails fast. "auto": offline once a model is installed (the chosen one, else the other), else cloud if a
/// key is saved.
fn preflight(app: &AppHandle, s: &Settings) -> Result<&'static str, String> {
    let local = models::chosen(app, &s.speech);
    match (s.engine.as_str(), local) {
        ("cloud", _) => cloud::get_key(&s.cloud_provider).map(|_| "cloud"),
        (_, Some(m)) => Ok(m.id),
        ("auto", None) if cloud::has_key(&s.cloud_provider) => Ok("cloud"),
        _ => Err(NEED_MODEL.into()),
    }
}

fn transcribe(app: &AppHandle, engine: &str, s: &Settings, samples: &[f32]) -> Result<String, String> {
    if engine == "cloud" {
        // no language = whisper-1 / Groq Whisper detect it
        let lang = (s.language != "auto").then_some(s.language.as_str());
        cloud::transcribe(&s.cloud_provider, &s.cloud_model, &cloud::get_key(&s.cloud_provider)?, samples, lang)
    } else {
        let m = if engine == models::WHISPER.id { &models::WHISPER } else { &models::PARAKEET };
        local::transcribe(&models::dir(app, m)?, &s.language, samples)
    }
}

/// Raw transcript → the text to paste: snippets set aside, replacements, dictionary, tidy-up, optional
/// Claude clean-up (used only if it keeps the snippet marks), snippets put back.
pub fn process(raw: &str, w: &text::Words, style: &str, app_name: Option<&str>) -> String {
    let p = text::prepare(raw, w);
    let mut written = text::tidy(&p.text);
    if style != "verbatim" {
        let cleaned = cleanup::clean(&written, style, app_name, &w.words);
        if text::keeps_marks(&cleaned, &p.snippets) {
            written = cleaned;
        }
    }
    text::finish(&written, &p.snippets)
}

fn deliver(s: &Settings, text: &str) -> Result<bool, String> {
    if s.output == "copy" {
        copy(text).map(|_| false)
    } else {
        paste_os::paste(text)
    }
}

/// One recording in progress: the recorder, its settings, the resolved engine, the app in front.
struct Rec {
    r: audio::Recorder,
    s: Settings,
    engine: &'static str,
    front: Option<String>,
    /// A voice note: the text goes back to the page, not to the app in front.
    page: bool,
}

/// What the Done handler needs from the running transcription.
struct Job {
    s: Settings,
    front: Option<String>,
    ms: u64,
    page: bool,
}

fn record_history(app: &AppHandle, text: &str, front: Option<&str>, ms: u64) {
    let words = text::word_count(text) as i64;
    let at = chrono::Utc::now().timestamp_millis();
    if let Err(e) = app.state::<AppState>().db.lock().unwrap().add_dictation(text, front, at, words, ms as i64) {
        eprintln!("dictation: history not saved: {e}");
    }
    let _ = app.emit("dictation-history", ());
}

fn run(app: AppHandle, rx: std::sync::mpsc::Receiver<Msg>) {
    let t0 = Instant::now();
    let mut m = Machine::default();
    let mut rec: Option<Rec> = None;
    let mut job: Option<Job> = None;
    let mut page_next = false;
    loop {
        let msg = rx.recv_timeout(TICK);
        let now = t0.elapsed().as_millis() as u64;
        let step = match msg {
            Ok(Msg::Press) => m.press(now),
            Ok(Msg::Release) => m.release(now),
            Ok(Msg::Escape) => m.escape(),
            Ok(Msg::Cancel) => m.cancel(),
            Ok(Msg::PageStart) => match page_start(&mut m, now) {
                Ok(step) => {
                    page_next = true;
                    step
                }
                Err(e) => {
                    emit(&app, json!({"state": "error", "error": e, "page": true})); // the mic buttons reset
                    Step::None
                }
            },
            Ok(Msg::PageStop) if rec.as_ref().is_some_and(|r| r.page) => m.press(now),
            Ok(Msg::PageStop) => Step::None,
            Ok(Msg::Done(id, r)) => {
                if m.done(id) {
                    let j = job.take().unwrap_or(Job { s: Settings::default(), front: None, ms: 0, page: false });
                    let r = r.map(|t| t.trim().to_string()).and_then(|t| if t.is_empty() { Err("No speech detected".to_string()) } else { Ok(t) });
                    // a voice note's text is inserted by the page itself
                    match r.and_then(|t| if j.page { Ok((t, false)) } else { deliver(&j.s, &t).map(|pasted| (t, pasted)) }) {
                        Ok((text, pasted)) => {
                            record_history(&app, &text, j.front.as_deref(), j.ms);
                            emit(&app, json!({"state": "done", "text": text, "pasted": pasted, "inserted": j.page, "page": j.page}));
                        }
                        Err(e) => emit(&app, json!({"state": "error", "error": e, "page": j.page})),
                    }
                }
                Step::None
            }
            Err(RecvTimeoutError::Timeout) => m.tick(now),
            Err(RecvTimeoutError::Disconnected) => return,
        };
        match step {
            Step::Start | Step::Restart => {
                rec = None;
                let page = std::mem::take(&mut page_next);
                let s = load(&app);
                let started = preflight(&app, &s).and_then(|engine| {
                    let r = audio::Recorder::start(Some(s.mic.as_str()).filter(|n| !n.is_empty()))?;
                    let front = if page { Some("Notes".to_string()) } else { paste_os::foreground_app() };
                    Ok(Rec { r, engine, front, s, page })
                });
                match started {
                    Ok(r) => rec = Some(r),
                    Err(e) => {
                        m.cancel();
                        emit(&app, json!({"state": "error", "error": e, "page": page}));
                    }
                }
            }
            Step::Discard => {
                page_next = false;
                rec = None;
                job = None;
                emit(&app, json!({"state": "idle"}));
            }
            Step::Transcribe(id) => {
                let Some(Rec { r, s, engine, front, page }) = rec.take() else {
                    m.done(id);
                    continue;
                };
                emit(&app, json!({"state": "transcribing", "page": page}));
                let samples = r.stop();
                job = Some(Job { s: s.clone(), front: front.clone(), ms: samples.len() as u64 / 16, page });
                let app = app.clone();
                std::thread::spawn(move || {
                    let peak = samples.iter().fold(0f32, |p, v| p.max(v.abs()));
                    // too short or silent: nothing to send to the model (Coucou's thresholds)
                    let r = if samples.len() < 16_000 / 4 || peak < 0.004 {
                        Err("No speech detected".to_string())
                    } else {
                        // a panic must still send Done, or the controller would wait in Transcribing for good
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            transcribe(&app, engine, &s, &samples).map(|raw| process(&raw, &load_words(&app), &s.cleanup, front.as_deref()))
                        }))
                        .unwrap_or_else(|_| Err("Transcription failed".to_string()))
                    };
                    send(Msg::Done(id, r));
                });
            }
            Step::Lock | Step::None => {}
        }
        if let (Phase::Recording { start, .. } | Phase::TapWait { start, .. }, Some(r)) = (m.phase, &rec) {
            let locked = matches!(m.phase, Phase::Recording { locked: true, .. });
            // Esc always reaches the Windows keyboard hook
            emit(&app, json!({"state": "recording", "level": r.r.level(), "ms": now - start, "locked": locked, "escCancels": cfg!(windows), "page": r.page}));
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
    let s = load(&app);
    if let Some(d) = models::chosen(&app, &s.speech).and_then(|m| models::dir(&app, m).ok()) {
        local::preload(d, s.language);
    }
    std::thread::spawn(move || run(app, rx));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hold_then_release_transcribes() {
        let mut m = Machine::default();
        assert_eq!(m.press(1_000), Step::Start);
        assert_eq!(m.press(1_050), Step::None); // key repeat
        assert_eq!(m.release(1_300), Step::Transcribe(1));
        assert_eq!(m.release(1_400), Step::None);
        assert_eq!(m.press(1_500), Step::None); // busy transcribing
        assert!(!m.done(7) && m.done(1));
        assert_eq!(m.phase, Phase::Idle);
    }

    #[test]
    fn a_lone_short_tap_is_dropped_after_the_double_tap_window() {
        let mut m = Machine::default();
        m.press(1_000);
        assert_eq!(m.release(1_299), Step::None); // still recording, waiting for a second tap
        assert_eq!(m.tick(1_699), Step::None);
        assert_eq!(m.tick(1_700), Step::Discard);
        assert_eq!(m.phase, Phase::Idle);
        // a press after an unnoticed timeout starts afresh
        m.press(2_000);
        m.release(2_100);
        assert_eq!(m.press(3_000), Step::Restart);
        assert_eq!(m.phase, Phase::Recording { start: 3_000, locked: false });
    }

    #[test]
    fn two_quick_taps_go_hands_free_and_the_next_press_finishes() {
        let mut m = Machine::default();
        assert_eq!(m.press(0), Step::Start);
        assert_eq!(m.release(100), Step::None);
        assert_eq!(m.press(300), Step::Lock);
        assert_eq!(m.release(350), Step::None); // releasing doesn't stop hands-free
        assert_eq!(m.tick(60_000), Step::None);
        assert_eq!(m.phase, Phase::Recording { start: 0, locked: true });
        assert_eq!(m.press(61_000), Step::Transcribe(1));
        assert_eq!(m.release(61_100), Step::None);
    }

    #[test]
    fn max_duration_stops_recording_even_hands_free() {
        let mut m = Machine::default();
        m.press(0);
        assert_eq!(m.tick(MAX_MS - 1), Step::None);
        assert_eq!(m.tick(MAX_MS), Step::Transcribe(1));
        assert_eq!(m.release(MAX_MS + 500), Step::None); // the late key-up is harmless
        m.done(1);
        m.press(0);
        m.release(10);
        m.press(20);
        assert_eq!(m.tick(MAX_MS), Step::Transcribe(2));
    }

    #[test]
    fn esc_drops_recordings_but_not_a_running_transcription() {
        let mut m = Machine::default();
        assert_eq!(m.escape(), Step::None);
        m.press(0);
        assert_eq!(m.escape(), Step::Discard);
        m.press(1_000);
        m.release(1_100);
        assert_eq!(m.escape(), Step::Discard); // between taps
        m.press(2_000);
        m.release(2_050);
        m.press(2_100); // hands-free
        assert_eq!(m.escape(), Step::Discard);
        m.press(3_000);
        m.release(4_000);
        assert_eq!(m.escape(), Step::None);
        assert!(m.done(1));
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
        assert!(!m.done(1)); // result arrives after cancel: nothing is pasted
        assert_eq!(m.press(12_000), Step::Start);
    }

    #[test]
    fn a_voice_note_records_hands_free_until_the_next_press() {
        let mut m = Machine::default();
        assert_eq!(m.start_locked(0), Step::Start);
        assert_eq!(m.release(50), Step::None); // the click's mouse-up doesn't stop it
        assert_eq!(m.tick(30_000), Step::None);
        assert_eq!(m.start_locked(31_000), Step::None); // already recording
        assert_eq!(m.press(40_000), Step::Transcribe(1));
        assert_eq!(m.start_locked(40_100), Step::None); // busy transcribing
        assert!(m.done(1));
        assert_eq!(m.start_locked(41_000), Step::Start);
        assert_eq!(m.escape(), Step::Discard);
    }

    #[test]
    fn a_voice_note_while_dictation_is_busy_is_an_error() {
        let mut m = Machine::default();
        m.press(0); // hotkey dictation recording
        assert!(page_start(&mut m, 10).is_err());
        m.release(100); // between taps
        assert!(page_start(&mut m, 150).is_err());
        m.press(200);
        m.press(60_000); // hands-free → transcribing
        assert!(page_start(&mut m, 60_100).is_err());
        assert!(m.done(1));
        assert_eq!(page_start(&mut m, 61_000), Ok(Step::Start));
    }

    #[test]
    fn process_runs_the_text_pipeline_without_claude_when_verbatim() {
        let w = text::Words {
            words: vec!["Supabase".into()],
            replacements: vec![text::Replacement { from: "cloud code".into(), to: "Claude Code".into() }],
            snippets: vec![text::Snippet { trigger: "my signature".into(), text: "Best,\n  Alex".into() }],
        };
        assert_eq!(process("we use supabase  and cloud code , my signature", &w, "verbatim", None), "We use Supabase and Claude Code, Best,\n  Alex");
    }
}
