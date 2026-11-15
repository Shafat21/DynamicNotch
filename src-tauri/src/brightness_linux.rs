//! Linux (best effort, compile-unverified): backlight via `brightnessctl` when installed, else a
//! writable `/sys/class/backlight/*`; external monitors via `ddcutil` when installed.
//! Ids: `bl:<device>`, `ddc:<display number>`.

use super::{from_percent, parse, to_percent, BrightDisplay};
use crate::sound::sh;
use std::fs;
use std::path::PathBuf;
use std::sync::OnceLock;

const SYS: &str = "/sys/class/backlight";

fn has_brightnessctl() -> bool {
    static HAS: OnceLock<bool> = OnceLock::new();
    *HAS.get_or_init(|| sh("brightnessctl", &["--version"]).is_ok())
}

pub fn init_thread() {}

fn sys_read(dev: &str, file: &str) -> Option<u32> {
    fs::read_to_string(PathBuf::from(SYS).join(dev).join(file)).ok()?.trim().parse().ok()
}

fn backlights() -> Vec<BrightDisplay> {
    let panel = |dev: &str, cur, max| BrightDisplay { id: format!("bl:{dev}"), name: "Built-in display".into(), percent: to_percent(0, cur, max), kind: "internal" };
    if let Some(out) = has_brightnessctl().then(|| sh("brightnessctl", &["-m", "-l", "-c", "backlight"]).ok()).flatten() {
        return parse::brightnessctl(&out).into_iter().map(|(dev, cur, max)| panel(&dev, cur, max)).collect();
    }
    let Ok(dir) = fs::read_dir(SYS) else { return Vec::new() };
    dir.flatten()
        .filter_map(|e| {
            let dev = e.file_name().to_string_lossy().into_owned();
            let (cur, max) = (sys_read(&dev, "brightness")?, sys_read(&dev, "max_brightness")?);
            fs::OpenOptions::new().write(true).open(e.path().join("brightness")).ok()?; // hidden unless we may write it
            Some(panel(&dev, cur, max))
        })
        .collect()
}

fn externals() -> Vec<BrightDisplay> {
    let Ok(out) = sh("ddcutil", &["detect", "--terse"]) else { return Vec::new() };
    parse::ddcutil_detect(&out)
        .into_iter()
        .filter_map(|(n, name)| {
            let (cur, max) = parse::ddcutil_vcp(&sh("ddcutil", &["getvcp", "10", "--brief", "-d", &n]).ok()?)?;
            Some(BrightDisplay { id: format!("ddc:{n}"), name, percent: to_percent(0, cur, max), kind: "external" })
        })
        .collect()
}

pub fn displays() -> Vec<BrightDisplay> {
    let mut list = backlights();
    list.extend(externals());
    list
}

pub fn set(id: &str, percent: u8) -> Result<(), String> {
    if let Some(dev) = id.strip_prefix("bl:") {
        let percent = percent.max(1); // 0 switches some panels off entirely
        if has_brightnessctl() {
            return sh("brightnessctl", &["-q", "-d", dev, "set", &format!("{percent}%")]).map(drop);
        }
        let max = sys_read(dev, "max_brightness").ok_or("Backlight not found")?;
        return fs::write(PathBuf::from(SYS).join(dev).join("brightness"), from_percent(0, max, percent).to_string()).map_err(|e| e.to_string());
    }
    if let Some(n) = id.strip_prefix("ddc:") {
        let (_, max) = parse::ddcutil_vcp(&sh("ddcutil", &["getvcp", "10", "--brief", "-d", n])?).ok_or("Unexpected ddcutil output")?;
        return sh("ddcutil", &["setvcp", "10", &from_percent(0, max, percent).to_string(), "-d", n]).map(drop);
    }
    Err("Unknown display".into())
}
