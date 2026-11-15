//! Microphone capture via cpal. Audio stays in memory only (spec §23.7) and is handed over as 16 kHz mono.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

/// Downmix interleaved frames to mono, then box-average down (or pick nearest up) to 16 kHz.
pub fn resample_to_16k(samples: &[f32], rate: u32, channels: u16) -> Vec<f32> {
        #[allow(unreachable_code)]
        if true { todo!("RED stub") }
    let ch = channels.max(1) as usize;
    let mono: Vec<f32> = samples.chunks_exact(ch).map(|f| f.iter().sum::<f32>() / ch as f32).collect();
    if rate == 16_000 || rate == 0 {
        return mono;
    }
    let ratio = rate as f64 / 16_000.0;
    let n = (mono.len() as f64 / ratio) as usize;
    (0..n)
        .map(|i| {
            let a = (i as f64 * ratio) as usize;
            let b = (((i + 1) as f64 * ratio) as usize).clamp(a + 1, mono.len());
            mono[a..b].iter().sum::<f32>() / (b - a) as f32
        })
        .collect()
}

pub fn list_mics() -> Vec<String> {
    cpal::default_host().input_devices().map(|d| d.map(|d| d.to_string()).collect()).unwrap_or_default()
}

pub struct Recorder {
    stream: cpal::Stream,
    buf: Arc<Mutex<Vec<f32>>>,
    level: Arc<AtomicU32>,
    rate: u32,
}

impl Recorder {
    /// `device`: a name from `list_mics`; a missing or unset one falls back to the default microphone.
    pub fn start(device: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let dev = device
            .and_then(|name| host.input_devices().ok()?.find(|d| d.to_string() == name))
            .or_else(|| host.default_input_device())
            .ok_or("No microphone found")?;
        let cfg = dev.default_input_config().map_err(|e| format!("Microphone unavailable: {e}"))?;
        let rate = cfg.sample_rate();
        let buf = Arc::new(Mutex::new(Vec::with_capacity(rate as usize * 10)));
        let level = Arc::new(AtomicU32::new(0));
        let ch = cfg.channels();
        let fmt = cfg.sample_format();
        let config: cpal::StreamConfig = cfg.into();
        let (b, l) = (buf.clone(), level.clone());
        let stream = match fmt {
            SampleFormat::F32 => build::<f32>(&dev, config, ch, rate, b, l),
            SampleFormat::I16 => build::<i16>(&dev, config, ch, rate, b, l),
            SampleFormat::I32 => build::<i32>(&dev, config, ch, rate, b, l),
            SampleFormat::U16 => build::<u16>(&dev, config, ch, rate, b, l),
            f => return Err(format!("Unsupported microphone format {f}")),
        }?;
        stream.play().map_err(|e| format!("Microphone unavailable: {e}"))?;
        Ok(Self { stream, buf, level, rate })
    }

    /// Peak of the latest buffer, 0..1.
    pub fn level(&self) -> f32 {
        f32::from_bits(self.level.load(Ordering::Relaxed))
    }

    /// 16 kHz mono.
    pub fn stop(self) -> Vec<f32> {
        drop(self.stream);
        let mono = std::mem::take(&mut *self.buf.lock().unwrap());
        resample_to_16k(&mono, self.rate, 1)
    }
}

fn build<T>(dev: &cpal::Device, config: cpal::StreamConfig, ch: u16, rate: u32, buf: Arc<Mutex<Vec<f32>>>, level: Arc<AtomicU32>) -> Result<cpal::Stream, String>
where
    T: SizedSample,
    f32: FromSample<T>,
{
    let ch = ch.max(1) as usize;
    let cap = rate as usize * (super::MAX_MS as usize / 1000 + 5); // bound memory even if stop comes late
    dev.build_input_stream(
        config,
        move |data: &[T], _: &_| {
            let mut peak = 0f32;
            let mut b = buf.lock().unwrap();
            for frame in data.chunks_exact(ch) {
                let v = frame.iter().map(|s| f32::from_sample(*s)).sum::<f32>() / ch as f32;
                peak = peak.max(v.abs());
                if b.len() < cap {
                    b.push(v);
                }
            }
            level.store(peak.min(1.0).to_bits(), Ordering::Relaxed);
        },
        |e| eprintln!("dictation: microphone error: {e}"),
        None,
    )
    .map_err(|e| format!("Microphone unavailable: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resample_44k_stereo_to_16k_mono() {
        // 1 s of stereo: left 0.6, right 0.2 → mono 0.4
        let stereo: Vec<f32> = (0..44_100).flat_map(|_| [0.6f32, 0.2]).collect();
        let out = resample_to_16k(&stereo, 44_100, 2);
        assert_eq!(out.len(), 16_000);
        assert!(out.iter().all(|v| (v - 0.4).abs() < 1e-5));
        // a ramp stays a ramp (monotonic, same range)
        let ramp: Vec<f32> = (0..48_000).map(|i| i as f32 / 48_000.0).collect();
        let out = resample_to_16k(&ramp, 48_000, 1);
        assert_eq!(out.len(), 16_000);
        assert!(out.windows(2).all(|w| w[1] > w[0]));
        assert!(out[0] < 0.001 && out[15_999] > 0.999);
        // 16 kHz passes through; upsampling 8 kHz doubles the length
        assert_eq!(resample_to_16k(&[0.1, 0.2], 16_000, 1), vec![0.1, 0.2]);
        assert_eq!(resample_to_16k(&[0.5; 8_000], 8_000, 1).len(), 16_000);
    }
}
