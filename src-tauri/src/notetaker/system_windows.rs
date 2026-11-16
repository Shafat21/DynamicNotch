//! The computer's sound on Windows: WASAPI turns the default output opened for input into its loopback.

use cpal::traits::{DeviceTrait, HostTrait};

pub fn device(host: &cpal::Host) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    let dev = host.default_output_device().ok_or("No speakers or headphones found")?;
    let cfg = dev.default_output_config().map_err(|e| format!("Computer sound unavailable: {e}"))?;
    Ok((dev, cfg))
}
