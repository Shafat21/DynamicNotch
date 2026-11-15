//! Local transcription with whisper.cpp (`whisper-rs`), only in builds with the `local-whisper` feature.

use std::path::Path;

pub const AVAILABLE: bool = cfg!(feature = "local-whisper");
pub const UNAVAILABLE: &str = "local Whisper not built into this build";

#[cfg(not(feature = "local-whisper"))]
pub fn transcribe(_model: &Path, _samples: &[f32], _language: Option<&str>) -> Result<String, String> {
    Err(UNAVAILABLE.into())
}

#[cfg(feature = "local-whisper")]
pub fn transcribe(model: &Path, samples: &[f32], language: Option<&str>) -> Result<String, String> {
    use std::sync::Mutex;
    use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

    // the loaded model is kept for the next dictation (loading `small` takes a second)
    static CTX: Mutex<Option<(std::path::PathBuf, WhisperContext)>> = Mutex::new(None);
    whisper_rs::install_logging_hooks(); // silences whisper.cpp's stderr chatter
    let mut cache = CTX.lock().unwrap();
    if cache.as_ref().map(|c| c.0.as_path()) != Some(model) {
        *cache = None;
        let ctx = WhisperContext::new_with_params(model, WhisperContextParameters::default()).map_err(|e| format!("Loading the Whisper model failed: {e}"))?;
        *cache = Some((model.to_path_buf(), ctx));
    }
    let ctx = &cache.as_ref().unwrap().1;
    let mut state = ctx.create_state().map_err(|e| e.to_string())?;
    let mut p = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
    p.set_language(Some(language.unwrap_or("auto")));
    p.set_n_threads(std::thread::available_parallelism().map_or(4, |n| n.get().min(8)) as _);
    p.set_print_special(false);
    p.set_print_progress(false);
    p.set_print_realtime(false);
    p.set_print_timestamps(false);
    p.set_suppress_blank(true);
    p.set_suppress_nst(true);
    p.set_no_context(true);
    state.full(p, samples).map_err(|e| format!("Transcription failed: {e}"))?;
    Ok(state.as_iter().map(|s| s.to_str_lossy().map(|t| t.into_owned()).unwrap_or_default()).collect::<String>().trim().to_string())
}
