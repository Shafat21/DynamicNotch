//! Offline transcription through sherpa-onnx, linked statically: NVIDIA Parakeet (25 European languages,
//! detected by itself) or Meta Omnilingual ASR (1600+ languages, Bangla included; no language to pick).
//! The recognizer (about 1 GB of RAM) is loaded when a dictation starts (while the user speaks, about 2 s) and
//! dropped after IDLE_UNLOAD without use; one model at a time.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License
//! (full text: THIRD_PARTY_NOTICES.md).

use sherpa_onnx::{OfflineOmnilingualAsrCtcModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const RATE: usize = 16_000;
/// Longest stretch given to the model at once; longer audio is cut where it is quietest.
const MAX_CHUNK_SECONDS: f64 = 28.0;
/// Half a second of silence first: a dictation starts with the mic's click and the first word under way,
/// and without it Parakeet tends to start in the wrong language (Coucou's finding).
const LEAD_SILENCE: usize = RATE / 2;
const TAIL_SILENCE: usize = RATE * 3 / 10;

static REC: Mutex<Option<(PathBuf, Arc<OfflineRecognizer>)>> = Mutex::new(None);
/// Unix ms of the last load / use, read by the unloader.
static LAST_USED: AtomicU64 = AtomicU64::new(0);
const IDLE_UNLOAD: Duration = Duration::from_secs(5 * 60);

fn touch() {
    LAST_USED.store(SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64), Ordering::Relaxed);
}

/// Once: a thread that frees the model after IDLE_UNLOAD of no use. Never while a decode holds it (the decode's
/// own Arc keeps the count above 1), and the next dictation just loads it again.
fn start_unloader() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::thread::spawn(|| loop {
            std::thread::sleep(Duration::from_secs(30));
            let idle = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64).saturating_sub(LAST_USED.load(Ordering::Relaxed));
            if idle >= IDLE_UNLOAD.as_millis() as u64 {
                let mut slot = REC.lock().unwrap_or_else(|e| e.into_inner());
                if slot.as_ref().is_some_and(|(_, r)| Arc::strong_count(r) == 1) {
                    *slot = None;
                }
            }
        });
    });
}

fn recognizer(dir: &Path) -> Result<Arc<OfflineRecognizer>, String> {
    let mut slot = REC.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, r)) = slot.as_ref().filter(|(d, _)| d == dir) {
        touch();
        return Ok(r.clone());
    }
    if !super::models::present(dir) {
        return Err("Download the offline model first — Settings → Features → Dictation".into());
    }
    *slot = None; // free the previous model before loading another (each takes about 1 GB of memory)
    let p = |f: &str| Some(dir.join(f).to_string_lossy().into_owned());
    let mut config = OfflineRecognizerConfig::default();
    if super::models::of(dir).is_multilingual() {
        config.model_config.omnilingual = OfflineOmnilingualAsrCtcModelConfig { model: p("model.int8.onnx") };
    } else {
        config.model_config.transducer = OfflineTransducerModelConfig { encoder: p("encoder.int8.onnx"), decoder: p("decoder.int8.onnx"), joiner: p("joiner.int8.onnx") };
        config.model_config.model_type = Some("nemo_transducer".into());
    }
    config.model_config.tokens = p("tokens.txt");
    config.model_config.num_threads = std::thread::available_parallelism().map_or(2, |n| n.get().clamp(1, 4) as i32);
    let r = Arc::new(OfflineRecognizer::create(&config).ok_or("Loading the offline model failed")?);
    *slot = Some((dir.to_path_buf(), r.clone()));
    touch();
    start_unloader();
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
    touch(); // a long decode counts as use: the idle clock starts when it ends
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Character error rate over letters and digits (Levenshtein distance / reference length).
    fn cer(want: &str, got: &str) -> f64 {
        let k = |s: &str| s.chars().filter(|c| c.is_alphanumeric() || unicode_normalization::char::is_combining_mark(*c)).collect::<Vec<_>>();
        let (a, b) = (k(want), k(got));
        let mut row: Vec<usize> = (0..=b.len()).collect();
        for (i, ca) in a.iter().enumerate() {
            let mut prev = row[0];
            row[0] = i + 1;
            for (j, cb) in b.iter().enumerate() {
                let cur = row[j + 1];
                row[j + 1] = (prev + (ca != cb) as usize).min(row[j] + 1).min(cur + 1);
                prev = cur;
            }
        }
        row[b.len()] as f64 / a.len().max(1) as f64
    }

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
    /// DN_MODEL=multilingual for Omnilingual, DN_EXPECT="a,b" words to find ("" = none), DN_WANT = the
    /// reference transcript, for a character error rate.
    #[test]
    #[ignore]
    fn transcribes_a_known_wav() {
        let wav = std::env::var("DN_TEST_WAV").expect("DN_TEST_WAV");
        let id = if std::env::var("DN_MODEL").as_deref() == Ok("multilingual") { super::super::models::MULTILINGUAL.id } else { super::super::models::ID };
        let dir = std::env::var("DN_MODEL_DIR").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.dynamicnotch.app").join("models").join(id)
        });
        let wanted = std::env::var("DN_WANT").ok();
        let bytes = std::fs::read(wav).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        let ch = u16::from_le_bytes([bytes[22], bytes[23]]);
        let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
        let data = bytes.windows(4).position(|w| w == b"data").unwrap() + 8;
        let pcm: Vec<f32> = bytes[data..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect();
        let samples = super::super::audio::resample_to_16k(&pcm, rate, ch);
        let t0 = std::time::Instant::now();
        recognizer(&dir).unwrap();
        let load = t0.elapsed();
        let t1 = std::time::Instant::now();
        let text = transcribe(&dir, &samples).unwrap();
        let secs = samples.len() as f64 / 16_000.0;
        println!("TRANSCRIPT ({secs:.1} s audio, load {load:?}, decode {:?}, RTF {:.2}): {text}", t1.elapsed(), t1.elapsed().as_secs_f64() / secs);
        if let Some(w) = wanted {
            println!("CER {:.1} %", cer(&w, &text) * 100.0);
        }
        let lower = text.to_lowercase();
        let expect = std::env::var("DN_EXPECT").unwrap_or_else(|_| "quick,brown,fox,jumps,lazy,dog".into());
        for w in expect.split(',') {
            assert!(lower.contains(w), "missing {w:?} in {text:?}");
        }
    }
}
