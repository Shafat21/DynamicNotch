//! Linux (best effort, compile-unverified): `wpctl` (PipeWire/WirePlumber), else `pactl` (PulseAudio or
//! pipewire-pulse). Device ids are wpctl node ids or pactl sink names.

use super::parse;
use super::{sh, AudioDevice, SoundState};
use std::sync::OnceLock;

const WP_SINK: &str = "@DEFAULT_AUDIO_SINK@";
const PA_SINK: &str = "@DEFAULT_SINK@";

fn wp() -> bool {
    static HAS: OnceLock<bool> = OnceLock::new();
    *HAS.get_or_init(|| sh("wpctl", &["status"]).is_ok())
}

fn unexpected(prog: &str) -> String {
    format!("Unexpected {prog} output")
}

pub fn init_thread() {}

pub fn state() -> Result<SoundState, String> {
    if wp() {
        let (volume, muted) = parse::wpctl_volume(&sh("wpctl", &["get-volume", WP_SINK])?).ok_or_else(|| unexpected("wpctl"))?;
        // device_id is filled in from the device list by the caller (sound::with_default)
        Ok(SoundState { volume, muted, device_id: String::new() })
    } else {
        let volume = parse::pactl_volume(&sh("pactl", &["get-sink-volume", PA_SINK])?).ok_or_else(|| unexpected("pactl"))?;
        let muted = parse::pactl_mute(&sh("pactl", &["get-sink-mute", PA_SINK])?).ok_or_else(|| unexpected("pactl"))?;
        Ok(SoundState { volume, muted, device_id: sh("pactl", &["get-default-sink"])?.trim().to_string() })
    }
}

pub fn set_volume(v: f32) -> Result<(), String> {
    if wp() {
        sh("wpctl", &["set-volume", WP_SINK, &format!("{v:.2}")])
    } else {
        sh("pactl", &["set-sink-volume", PA_SINK, &format!("{}%", (v * 100.0).round())])
    }
    .map(drop)
}

pub fn set_mute(muted: bool) -> Result<(), String> {
    let flag = if muted { "1" } else { "0" };
    if wp() { sh("wpctl", &["set-mute", WP_SINK, flag]) } else { sh("pactl", &["set-sink-mute", PA_SINK, flag]) }.map(drop)
}

pub fn devices() -> Result<Vec<AudioDevice>, String> {
    if wp() {
        Ok(parse::wpctl_sinks(&sh("wpctl", &["status"])?))
    } else {
        let default = sh("pactl", &["get-default-sink"])?;
        Ok(parse::pactl_sinks(&sh("pactl", &["list", "sinks"])?, &default))
    }
}

pub fn set_default(id: &str) -> Result<(), String> {
    if wp() { sh("wpctl", &["set-default", id]) } else { sh("pactl", &["set-default-sink", id]) }.map(drop)
}
