//! Call capture: the microphone and the computer's sound (WASAPI loopback on Windows, a monitor source on
//! Linux), each streamed to its own 16 kHz mono 16-bit WAV file while the call goes on.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::fs::File;
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

#[cfg(windows)]
#[path = "system_windows.rs"]
mod system_os;
#[cfg(target_os = "linux")]
#[path = "system_linux.rs"]
mod system_os;

pub const RATE: u32 = 16_000;

pub enum Source {
    /// A microphone by name (`list_mics`); missing or empty = the default one.
    Mic(String),
    /// What the computer plays.
    System,
}

/// Streaming version of `dictation::audio::resample_to_16k` for mono input: box average down (or repeat up).
pub struct Down {
    ratio: f64,
    n: u64,
    sum: f32,
    count: u32,
}

impl Down {
    pub fn new(rate: u32) -> Self {
        Self { ratio: rate.max(1) as f64 / RATE as f64, n: 0, sum: 0.0, count: 0 }
    }

    pub fn push(&mut self, input: &[f32], out: &mut Vec<f32>) {
        for &v in input {
            let slot = (self.n as f64 / self.ratio) as u64;
            self.n += 1;
            let next = (self.n as f64 / self.ratio) as u64;
            self.sum += v;
            self.count += 1;
            if next > slot {
                let avg = self.sum / self.count as f32;
                out.extend(std::iter::repeat_n(avg, (next - slot) as usize));
                self.sum = 0.0;
                self.count = 0;
            }
        }
    }
}

/// A 16 kHz mono 16-bit WAV written as the samples come in (sizes filled in by `finish`).
pub struct WavWriter {
    out: BufWriter<File>,
    pub samples: u64,
}

impl WavWriter {
    pub fn create(path: &Path) -> std::io::Result<Self> {
        let mut out = BufWriter::new(File::create(path)?);
        header(&mut out, 0)?;
        Ok(Self { out, samples: 0 })
    }

    pub fn write(&mut self, samples: &[f32]) -> std::io::Result<()> {
        for &s in samples {
            self.out.write_all(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes())?;
        }
        self.samples += samples.len() as u64;
        Ok(())
    }

    pub fn finish(mut self) -> std::io::Result<()> {
        self.out.flush()?;
        let mut file = self.out.into_inner().map_err(|e| e.into_error())?;
        file.seek(SeekFrom::Start(0))?;
        header(&mut file, self.samples.min(u32::MAX as u64 / 2) as u32)?;
        file.flush()
    }
}

fn header(out: &mut impl Write, samples: u32) -> std::io::Result<()> {
    let data = samples.saturating_mul(2);
    out.write_all(b"RIFF")?;
    out.write_all(&36u32.saturating_add(data).to_le_bytes())?;
    out.write_all(b"WAVEfmt ")?;
    out.write_all(&16u32.to_le_bytes())?;
    out.write_all(&1u16.to_le_bytes())?; // PCM
    out.write_all(&1u16.to_le_bytes())?; // mono
    out.write_all(&RATE.to_le_bytes())?;
    out.write_all(&(RATE * 2).to_le_bytes())?;
    out.write_all(&2u16.to_le_bytes())?;
    out.write_all(&16u16.to_le_bytes())?;
    out.write_all(b"data")?;
    out.write_all(&data.to_le_bytes())
}

/// Any 16-bit PCM WAV → 16 kHz mono (a file cut short by a crash still reads: the data runs to the end).
pub fn read_wav(path: &Path) -> Result<Vec<f32>, String> {
    let mut bytes = Vec::new();
    File::open(path).and_then(|mut f| f.read_to_end(&mut bytes)).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() < 44 || &bytes[..4] != b"RIFF" || u16::from_le_bytes([bytes[34], bytes[35]]) != 16 {
        return Err(format!("{}: not a 16-bit WAV file", path.display()));
    }
    let ch = u16::from_le_bytes([bytes[22], bytes[23]]);
    let rate = u32::from_le_bytes([bytes[24], bytes[25], bytes[26], bytes[27]]);
    let data = bytes.windows(4).position(|w| w == b"data").map_or(44, |p| p + 8);
    let pcm: Vec<f32> = bytes[data.min(bytes.len())..].as_chunks::<2>().0.iter().map(|b| i16::from_le_bytes(*b) as f32 / 32768.0).collect();
    Ok(crate::dictation::audio::resample_to_16k(&pcm, rate, ch))
}

/// Free bytes on the disk holding `path`.
pub fn free_bytes(path: &Path) -> Option<u64> {
    let disks = sysinfo::Disks::new_with_refreshed_list();
    disks.list().iter().filter(|d| path.starts_with(d.mount_point())).max_by_key(|d| d.mount_point().as_os_str().len()).map(|d| d.available_space())
}

/// One source being written to a WAV file on its own thread (cpal streams can't move between threads).
pub struct Track {
    stop: Arc<AtomicBool>,
    level: Arc<AtomicU32>,
    thread: Option<JoinHandle<Result<u64, String>>>,
}

impl Track {
    pub fn start(source: Source, path: PathBuf) -> Result<Self, String> {
        let stop = Arc::new(AtomicBool::new(false));
        let level = Arc::new(AtomicU32::new(0));
        let (ready_tx, ready_rx) = mpsc::channel::<Result<(), String>>();
        let (s, l) = (stop.clone(), level.clone());
        let thread = std::thread::spawn(move || {
            let (tx, rx) = mpsc::channel::<Vec<f32>>();
            let opened = open(&source, tx, l).and_then(|(stream, rate)| Ok((stream, rate, WavWriter::create(&path).map_err(|e| e.to_string())?)));
            let (stream, rate, mut wav) = match opened {
                Ok(x) => x,
                Err(e) => {
                    let _ = ready_tx.send(Err(e.clone()));
                    return Err(e);
                }
            };
            let _ = ready_tx.send(Ok(()));
            let io = |e: std::io::Error| e.to_string();
            let mut down = Down::new(rate);
            let mut out = Vec::new();
            let (t0, mut last) = (Instant::now(), Instant::now());
            while !s.load(Ordering::Relaxed) {
                match rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(block) => {
                        last = Instant::now();
                        out.clear();
                        down.push(&block, &mut out);
                        wav.write(&out).map_err(io)?;
                    }
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => break,
                }
                // shortcut: loopback delivers nothing while the computer is silent, so silence is padded in to keep
                // both tracks on one clock; a drifting device clock isn't corrected (fine for "who spoke when")
                let due = (t0.elapsed().as_secs_f64() * RATE as f64) as u64;
                if last.elapsed() > Duration::from_millis(300) && wav.samples < due {
                    wav.write(&vec![0.0; (due - wav.samples) as usize]).map_err(io)?;
                }
            }
            drop(stream);
            while let Ok(block) = rx.try_recv() {
                out.clear();
                down.push(&block, &mut out);
                wav.write(&out).map_err(io)?;
            }
            let n = wav.samples;
            wav.finish().map_err(io)?;
            Ok(n)
        });
        match ready_rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => Ok(Self { stop, level, thread: Some(thread) }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                stop.store(true, Ordering::Relaxed);
                Err("The audio device did not answer".into())
            }
        }
    }

    /// Peak of the latest buffer, 0..1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// Closes the device and the file. Returns the samples written.
    pub fn finish(mut self) -> Result<u64, String> {
        self.stop.store(true, Ordering::Relaxed);
        self.thread.take().map_or(Ok(0), |t| t.join().unwrap_or_else(|_| Err("audio thread crashed".into())))
    }
}

impl Drop for Track {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

fn open(source: &Source, tx: Sender<Vec<f32>>, level: Arc<AtomicU32>) -> Result<(cpal::Stream, u32), String> {
    let host = cpal::default_host();
    let (dev, cfg) = match source {
        Source::Mic(name) => {
            let dev = Some(name.as_str())
                .filter(|n| !n.is_empty())
                .and_then(|n| host.input_devices().ok()?.find(|d| d.to_string() == n))
                .or_else(|| host.default_input_device())
                .ok_or("No microphone found")?;
            let cfg = dev.default_input_config().map_err(|e| format!("Microphone unavailable: {e}"))?;
            (dev, cfg)
        }
        Source::System => system_os::device(&host)?,
    };
    let (rate, ch, fmt) = (cfg.sample_rate(), cfg.channels(), cfg.sample_format());
    let config: cpal::StreamConfig = cfg.into();
    let stream = match fmt {
        SampleFormat::F32 => build::<f32>(&dev, config, ch, tx, level),
        SampleFormat::I16 => build::<i16>(&dev, config, ch, tx, level),
        SampleFormat::I32 => build::<i32>(&dev, config, ch, tx, level),
        SampleFormat::U16 => build::<u16>(&dev, config, ch, tx, level),
        f => return Err(format!("Unsupported audio format {f}")),
    }?;
    stream.play().map_err(|e| format!("Audio device unavailable: {e}"))?;
    Ok((stream, rate))
}

fn build<T>(dev: &cpal::Device, config: cpal::StreamConfig, ch: u16, tx: Sender<Vec<f32>>, level: Arc<AtomicU32>) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = ch.max(1) as usize;
    dev.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let mono: Vec<f32> = data.chunks_exact(ch).map(|f| f.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / ch as f32).collect();
            level.store(mono.iter().fold(0f32, |p, v| p.max(v.abs())).min(1.0).to_bits(), Ordering::Relaxed);
            let _ = tx.send(mono);
        },
        |e| eprintln!("notetaker: audio error: {e}"),
        None,
    )
    .map_err(|e| format!("Audio device unavailable: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_resample_matches_the_one_shot_one() {
        let ramp: Vec<f32> = (0..44_100).map(|i| (i as f32 * 0.01).sin()).collect();
        let whole = crate::dictation::audio::resample_to_16k(&ramp, 44_100, 1);
        let mut d = Down::new(44_100);
        let mut out = Vec::new();
        for c in ramp.chunks(441) {
            d.push(c, &mut out); // callback-sized blocks
        }
        assert_eq!(out.len(), whole.len());
        // window edges may differ by one input sample (ceil vs floor), nothing more
        assert!(out.iter().zip(&whole).all(|(a, b)| (a - b).abs() < 0.02)); // the signal moves 0.01 per input sample
        let mut up = Vec::new();
        Down::new(8_000).push(&[0.5; 800], &mut up);
        assert_eq!(up.len(), 1_600);
    }

    #[test]
    fn a_wav_reads_back_what_was_written() {
        let path = std::env::temp_dir().join(format!("dn-notetaker-{}.wav", std::process::id()));
        let mut w = WavWriter::create(&path).unwrap();
        w.write(&[0.0, 0.5, -0.5, 1.0]).unwrap();
        w.finish().unwrap();
        let back = read_wav(&path).unwrap();
        std::fs::remove_file(&path).unwrap();
        assert_eq!(back.len(), 4);
        assert!((back[1] - 0.5).abs() < 1e-3 && (back[2] + 0.5).abs() < 1e-3);
    }
}
