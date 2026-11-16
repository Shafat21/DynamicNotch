//! The computer's sound on Linux, best effort: the first PulseAudio / PipeWire "monitor" input.

use cpal::traits::{DeviceTrait, HostTrait};

pub fn device(host: &cpal::Host) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    let dev = host
        .input_devices()
        .ok()
        .and_then(|mut it| it.find(|d| d.to_string().to_lowercase().contains("monitor")))
        .ok_or("No monitor source for the computer's sound")?;
    let cfg = dev.default_input_config().map_err(|e| format!("Computer sound unavailable: {e}"))?;
    Ok((dev, cfg))
}
