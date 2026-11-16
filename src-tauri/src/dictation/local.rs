//! Offline transcription through sherpa-onnx, linked statically: NVIDIA Parakeet (25 European languages)
//! or Whisper large-v3-turbo (99 languages, Bangla included). The recognizer is loaded on first use
//! (about 2 s) and kept for the next dictation; one model at a time.
//! Whisper's language follows the `language` setting: "auto" detects, "bn" forces Bangla, and "bn,en"
//! detects but expects one of those (turbo often hears Bangla as Hindi or Gujarati: then it is decoded
//! again as Bangla).
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License
//! (full text: THIRD_PARTY_NOTICES.md).

use sherpa_onnx::{OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig, OfflineWhisperModelConfig};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const RATE: usize = 16_000;
/// Longest stretch given to the model at once; longer audio is cut where it is quietest.
const MAX_CHUNK_SECONDS: f64 = 28.0;
/// Whisper writes at most ~224 tokens per window and Bangla takes ~20 tokens a second (byte-level BPE),
/// so its stretches are shorter.
const WHISPER_CHUNK_SECONDS: f64 = 9.0;
/// sherpa-onnx stops Whisper after 6 tokens per second of input: silence up to the 30 s window it reads
/// anyway lifts that cap to ~174 tokens, enough for a 9 s Bangla stretch.
const WHISPER_WINDOW: usize = RATE * 29;
/// Half a second of silence first: a dictation starts with the mic's click and the first word under way,
/// and without it Parakeet tends to start in the wrong language (Coucou's finding).
const LEAD_SILENCE: usize = RATE / 2;
const TAIL_SILENCE: usize = RATE * 3 / 10;

static REC: Mutex<Option<(PathBuf, Arc<OfflineRecognizer>)>> = Mutex::new(None);

/// The language codes of a `language` setting: [] = detect, [x] = force x, [x, y, …] = detect among them.
pub fn codes(setting: &str) -> Vec<&str> {
    setting.split(',').map(str::trim).filter(|c| c.len() == 2 && c.bytes().all(|b| b.is_ascii_lowercase())).collect()
}

/// The Unicode block a language is written in, for the ones Whisper often mixes up (None: any).
fn script(code: &str) -> Option<std::ops::RangeInclusive<u32>> {
    Some(match code {
        "bn" | "as" => 0x0980..=0x09FF,
        "hi" | "mr" | "ne" | "sa" => 0x0900..=0x097F,
        "pa" => 0x0A00..=0x0A7F,
        "gu" => 0x0A80..=0x0AFF,
        "or" => 0x0B00..=0x0B7F,
        "ta" => 0x0B80..=0x0BFF,
        "te" => 0x0C00..=0x0C7F,
        "kn" => 0x0C80..=0x0CFF,
        "ml" => 0x0D00..=0x0D7F,
        "si" => 0x0D80..=0x0DFF,
        "ur" | "ar" | "fa" | "ps" => 0x0600..=0x06FF,
        "ru" | "uk" | "bg" | "sr" | "mk" | "be" => 0x0400..=0x04FF,
        "en" | "de" | "fr" | "es" | "it" | "pt" | "nl" | "pl" | "sv" | "da" | "no" | "fi" | "cs" | "ro" | "hu" | "tr" | "id" | "ms" | "vi" => 0x0041..=0x024F,
        _ => return None,
    })
}

/// Whether `text` is written in the scripts of `codes` (Latin always counts: English words, digits).
/// False when most letters are in another script, i.e. Whisper took the speech for another language.
pub fn fits(text: &str, codes: &[&str]) -> bool {
    let Some(blocks) = codes.iter().map(|c| script(c)).collect::<Option<Vec<_>>>() else { return true };
    let letters: Vec<u32> = text.chars().filter(|c| c.is_alphabetic()).map(|c| c as u32).collect();
    let foreign = letters.iter().filter(|&&c| !(0x41..=0x24F).contains(&c) && !blocks.iter().any(|b| b.contains(&c))).count();
    foreign * 2 <= letters.len()
}

// shortcut: sherpa-onnx 2.0.1's Rust API can't change Whisper's language once loaded, so the C
// SetConfig is called with the wrapper's (private, only) pointer; switch to the per-stream language
// option (#4189) once a release has it. The crate is pinned (=2.0.1) so its layout can't change unseen.
const _: () = assert!(std::mem::size_of::<OfflineRecognizer>() == std::mem::size_of::<*const ()>());
extern "C" {
    fn SherpaOnnxOfflineRecognizerSetConfig(r: *const sherpa_onnx_sys::OfflineRecognizer, c: *const sherpa_onnx_sys::OfflineRecognizerConfig);
}

/// Whisper's language for the next decode ("" = detect). Task stays "transcribe" (never translate).
fn set_whisper_language(rec: &OfflineRecognizer, language: &str) {
    let lang = std::ffi::CString::new(language).unwrap_or_default();
    unsafe {
        // all-zero = every other field at its default; SetConfig only takes the Whisper part
        let mut c: sherpa_onnx_sys::OfflineRecognizerConfig = std::mem::zeroed();
        c.model_config.whisper.language = lang.as_ptr();
        c.model_config.whisper.task = c"transcribe".as_ptr();
        c.model_config.whisper.tail_paddings = -1;
        let ptr = *(rec as *const OfflineRecognizer as *const *const sherpa_onnx_sys::OfflineRecognizer);
        SherpaOnnxOfflineRecognizerSetConfig(ptr, &c);
    }
}

fn recognizer(dir: &Path) -> Result<Arc<OfflineRecognizer>, String> {
    let mut slot = REC.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, r)) = slot.as_ref().filter(|(d, _)| d == dir) {
        return Ok(r.clone());
    }
    if !super::models::present(dir) {
        return Err("Download the offline model first — Settings → Features → Dictation".into());
    }
    *slot = None; // free the previous model before loading another (each takes about 1 GB of memory)
    let p = |f: &str| Some(dir.join(f).to_string_lossy().into_owned());
    let mut config = OfflineRecognizerConfig::default();
    if super::models::of(dir).is_whisper() {
        config.model_config.whisper = OfflineWhisperModelConfig {
            encoder: p("turbo-encoder.int8.onnx"),
            decoder: p("turbo-decoder.int8.onnx"),
            language: Some(String::new()),
            task: Some("transcribe".into()),
            tail_paddings: -1,
            ..Default::default()
        };
        config.model_config.tokens = p("turbo-tokens.txt");
    } else {
        config.model_config.transducer = OfflineTransducerModelConfig { encoder: p("encoder.int8.onnx"), decoder: p("decoder.int8.onnx"), joiner: p("joiner.int8.onnx") };
        config.model_config.tokens = p("tokens.txt");
        config.model_config.model_type = Some("nemo_transducer".into());
    }
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

fn decode(rec: &OfflineRecognizer, whisper: bool, language: &str, chunk: &[f32]) -> String {
    let mut audio = padded(chunk);
    if whisper {
        set_whisper_language(rec, language);
        audio.resize(audio.len().max(WHISPER_WINDOW), 0.0);
    }
    let stream = rec.create_stream();
    stream.accept_waveform(RATE as i32, &audio);
    rec.decode(&stream);
    stream.get_result().map(|r| r.text.trim().to_string()).unwrap_or_default()
}

/// 16 kHz mono → text. `dir` = the model folder; `language` = the setting (Whisper only, see the top).
pub fn transcribe(dir: &Path, language: &str, samples: &[f32]) -> Result<String, String> {
    let rec = recognizer(dir)?;
    let whisper = super::models::of(dir).is_whisper();
    let codes = codes(language);
    let forced = if codes.len() == 1 { codes[0] } else { "" };
    // dictation and the notetaker share one recognizer: one decode at a time
    static DECODE: Mutex<()> = Mutex::new(());
    let _one = DECODE.lock().unwrap_or_else(|e| e.into_inner());
    let mut text = String::new();
    for (_, chunk) in split_for_model(samples, if whisper { WHISPER_CHUNK_SECONDS } else { MAX_CHUNK_SECONDS }) {
        if chunk.len() < RATE / 4 {
            continue;
        }
        let mut t = decode(&rec, whisper, forced, chunk);
        if whisper && codes.len() > 1 && !fits(&t, &codes) {
            t = decode(&rec, whisper, codes[0], chunk); // heard as another language: the first expected one
        }
        if !t.is_empty() {
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
        let e = transcribe(Path::new("Z:/no/model"), "auto", &[0.0; 16_000]).unwrap_err();
        assert!(e.contains("Download the offline model"), "{e}");
    }

    #[test]
    fn language_settings_become_codes() {
        assert!(codes("auto").is_empty() && codes("").is_empty() && codes("EN").is_empty());
        assert_eq!(codes("bn"), vec!["bn"]);
        assert_eq!(codes("bn,en"), vec!["bn", "en"]);
    }

    #[test]
    fn a_transcript_in_another_script_does_not_fit() {
        let be = ["bn", "en"];
        assert!(fits("আমি বাংলায় কথা বলছি", &be));
        assert!(fits("The quick brown fox", &be));
        assert!(fits("আমি Claude Code ব্যবহার করি", &be)); // mixed Bangla + English
        assert!(!fits("शिल्पान चले धोकार मुखे", &be)); // Bangla heard as Hindi (real turbo output)
        assert!(!fits("આશામે જાતીઓ નાગ", &be)); // … or as Gujarati
        assert!(fits("", &be) && fits("123 ?", &be));
        assert!(fits("anything 你好", &["zh", "en"])); // no script known: always fits
    }

    /// Real model + a known WAV (16-bit PCM mono, any rate). Generate one with Windows SAPI:
    /// `DN_TEST_WAV=… cargo test --lib transcribes_a_known_wav -- --ignored --nocapture`
    /// The model dir defaults to %APPDATA%\com.dynamicnotch.app\models\<ID> (override: DN_MODEL_DIR).
    /// DN_MODEL=whisper for the multilingual model, DN_LANG to force a language, DN_EXPECT="a,b" words to find.
    #[test]
    #[ignore]
    fn transcribes_a_known_wav() {
        let wav = std::env::var("DN_TEST_WAV").expect("DN_TEST_WAV");
        let id = if std::env::var("DN_MODEL").as_deref() == Ok("whisper") { super::super::models::WHISPER.id } else { super::super::models::ID };
        let dir = std::env::var("DN_MODEL_DIR").map(PathBuf::from).unwrap_or_else(|_| {
            PathBuf::from(std::env::var("APPDATA").unwrap()).join("com.dynamicnotch.app").join("models").join(id)
        });
        let lang = std::env::var("DN_LANG").unwrap_or_else(|_| "auto".into());
        let wanted = std::env::var("DN_WANT").ok(); // the reference transcript, for a character error rate
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
        let text = transcribe(&dir, &lang, &samples).unwrap();
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
