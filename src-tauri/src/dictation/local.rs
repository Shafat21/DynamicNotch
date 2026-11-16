//! Offline transcription: NVIDIA Parakeet (25 languages, detected by itself) through sherpa-onnx,
//! linked statically. The recognizer is loaded on first use (about 2 s) and kept for the next dictation.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License.

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const RATE: usize = 16_000;
/// Longest stretch given to the model at once; longer audio is cut where it is quietest.
const MAX_CHUNK_SECONDS: f64 = 28.0;
/// Half a second of silence first: a dictation starts with the mic's click and the first word under way,
/// and without it Parakeet tends to start in the wrong language (Coucou's finding).
const LEAD_SILENCE: usize = RATE / 2;
const TAIL_SILENCE: usize = RATE * 3 / 10;

static REC: Mutex<Option<(PathBuf, Arc<OfflineRecognizer>)>> = Mutex::new(None);

fn recognizer(dir: &Path) -> Result<Arc<OfflineRecognizer>, String> {
    let mut slot = REC.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, r)) = slot.as_ref().filter(|(d, _)| d == dir) {
        return Ok(r.clone());
    }
    if !super::models::present(dir) {
        return Err("Download the offline model first — Settings → Features → Dictation".into());
    }
    let p = |f: &str| Some(dir.join(f).to_string_lossy().into_owned());
    let mut config = OfflineRecognizerConfig::default();
    config.model_config.transducer = OfflineTransducerModelConfig { encoder: p("encoder.int8.onnx"), decoder: p("decoder.int8.onnx"), joiner: p("joiner.int8.onnx") };
    config.model_config.tokens = p("tokens.txt");
    config.model_config.model_type = Some("nemo_transducer".into());
    config.model_config.num_threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4) as i32);
    let r = Arc::new(OfflineRecognizer::create(&config).ok_or("Loading the offline model failed")?);
    *slot = Some((dir.to_path_buf(), r.clone()));
    Ok(r)
}

/// Load the model in the background so the first dictation isn't slowed down.
pub fn preload(dir: PathBuf) {
    if super::models::present(&dir) {
        std::thread::spawn(move || {
            let _ = recognizer(&dir);
        });
    }
}

/// `chunk` between the silences the model is given it with.
pub fn padded(chunk: &[f32]) -> Vec<f32> {
    let mut out = vec![0.0; LEAD_SILENCE];
    out.extend_from_slice(chunk);
    out.resize(out.len() + TAIL_SILENCE, 0.0);
    out
}

/// Long audio in stretches the model takes at once, each cut at the quietest 100 ms in the last 8 s.
/// Returns (offset, samples).
pub fn split_for_model(samples: &[f32], max_seconds: f64) -> Vec<(usize, &[f32])> {
    let max = (max_seconds * RATE as f64) as usize;
    let window = RATE / 10;
    let mut out = Vec::new();
    let mut start = 0;
    while start < samples.len() {
        if samples.len() - start <= max {
            out.push((start, &samples[start..]));
            break;
        }
        let mut best = start + max;
        let mut best_energy = f64::MAX;
        let mut at = start + max.saturating_sub(8 * RATE);
        while at + window <= start + max {
            let e: f64 = samples[at..at + window].iter().map(|&s| (s as f64) * (s as f64)).sum();
            if e < best_energy {
                best_energy = e;
                best = at + window / 2;
            }
            at += window / 2;
        }
        out.push((start, &samples[start..best]));
        start = best;
    }
    out
}

/// 16 kHz mono → text. `dir` = the model folder.
pub fn transcribe(dir: &Path, samples: &[f32]) -> Result<String, String> {
    let rec = recognizer(dir)?;
    // dictation and the notetaker share one recognizer: one decode at a time
    static DECODE: Mutex<()> = Mutex::new(());
    let _one = DECODE.lock().unwrap_or_else(|e| e.into_inner());
    let mut text = String::new();
    for (_, chunk) in split_for_model(samples, MAX_CHUNK_SECONDS) {
        if chunk.len() < RATE / 4 {
            continue;
        }
        let stream = rec.create_stream();
        stream.accept_waveform(RATE as i32, &padded(chunk));
        rec.decode(&stream);
        if let Some(t) = stream.get_result().map(|r| r.text.trim().to_string()).filter(|t| !t.is_empty()) {
            if !text.is_empty() {
                text.push(' ');
            }
            text.push_str(&t);
        }
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(seconds: f64) -> Vec<f32> {
        (0..(RATE as f64 * seconds) as usize).map(|i| (i as f32 * 0.1).sin() * 0.5).collect()
    }

    #[test]
    fn the_model_hears_silence_around_the_words() {
        let chunk = vec![0.5f32; 1000];
        let a = padded(&chunk);
        assert_eq!(a.len(), LEAD_SILENCE + 1000 + TAIL_SILENCE);
        assert!(a[..LEAD_SILENCE].iter().all(|&s| s == 0.0));
        assert_eq!(&a[LEAD_SILENCE..LEAD_SILENCE + 1000], &chunk[..]);
        assert!(a[LEAD_SILENCE + 1000..].iter().all(|&s| s == 0.0));
    }

    #[test]
    fn long_audio_is_cut_in_the_quiet() {
        let mut s = tone(25.0);
        s.extend(vec![0.0; RATE]); // a pause from 25 s to 26 s
        s.extend(tone(20.0));
        let parts = split_for_model(&s, 28.0);
        assert_eq!(parts.len(), 2);
        let cut = parts[1].0 as f64 / RATE as f64;
        assert!((25.0..=26.0).contains(&cut), "cut at {cut}");
        assert_eq!(parts.iter().map(|p| p.1.len()).sum::<usize>(), s.len());
        assert_eq!(split_for_model(&s[..1000], 28.0).len(), 1);
    }

    #[test]
    fn missing_model_is_a_clear_error() {
        let e = transcribe(Path::new("Z:/no/model"), &[0.0; 16_000]).unwrap_err();
        assert!(e.contains("Download the offline model"), "{e}");
    }

    /// Real model + a known WAV (16-bit PCM mono, any rate). Generate one with Windows SAPI:
    /// `DN_TEST_WAV=… cargo test --lib transcribes_a_known_wav -- --ignored --nocapture`
    /// The model dir defaults to %APPDATA%\com.dynamicnotch.app\models\<ID> (override: DN_MODEL_DIR).
    #[test]
    #[ignore]
    fn transcribes_a_known_wav() {
        let wav = std::env::var("DN_TEST_WAV").expect("DN_TEST_WAV");
        let dir = std::env::var("DN_MODEL_DIR").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.dynamicnotch.app").join("models").join(super::super::models::ID)
        });
        let bytes = std::fs::read(wav).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        let ch = u16::from_le_bytes([bytes[22], bytes[23]]);
        let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let data = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let pcm: Vec<f32> = bytes[data..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect();
        let samples = super::super::audio::resample_to_16k(&pcm, rate, ch);
        let t0 = std::time::Instant::now();
        let text = transcribe(&dir, &samples).unwrap();
        println!("TRANSCRIPT ({:.1} s audio, {:?}): {text}", samples.len() as f64 / 16_000.0, t0.elapsed());
        let lower = text.to_lowercase();
        for w in ["quick", "brown", "fox", "jumps", "lazy", "dog"] {
            assert!(lower.contains(w), "missing {w:?} in {text:?}");
        }
    }
}
