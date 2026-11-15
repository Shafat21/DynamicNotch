//! Whisper ggml models: download from the official whisper.cpp Hugging Face repo, SHA-256 verified.

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub struct ModelInfo {
    pub name: &'static str,
    pub file: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

// sha256 = Git LFS oid from https://huggingface.co/api/models/ggerganov/whisper.cpp/tree/main (2026-11-15)
pub const MODELS: [ModelInfo; 3] = [
    ModelInfo {
        name: "tiny",
        file: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        size: 77_691_713,
        sha256: "be07e048e1e599ad46341c8d2a135645097a538221678b7acdd1b1919c6e1b21",
    },
    ModelInfo {
        name: "base",
        file: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        size: 147_951_465,
        sha256: "60ed5bc3dd14eea856493d334349b405782ddcaf0028d4b5df4088345fba2efe",
    },
    ModelInfo {
        name: "small",
        file: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        size: 487_601_967,
        sha256: "1be3a9b2063867b937e64e2ec7483364a79917e157fa98c5d94b5c1fffea987b",
    },
];

pub fn info(name: &str) -> Option<&'static ModelInfo> {
    MODELS.iter().find(|m| m.name == name)
}

pub fn path(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let m = info(name).ok_or_else(|| format!("Unknown model \"{name}\""))?;
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("models").join(m.file))
}

/// Downloads are verified before the rename, so a file of the right size is a good model.
pub fn installed(app: &AppHandle, name: &str) -> bool {
    let size = info(name).map(|m| m.size);
    path(app, name).ok().and_then(|p| std::fs::metadata(p).ok()).map(|md| md.len()) == size
}

/// Stream `r` into `w`, hashing on the way; Err unless exactly `total` bytes with the expected SHA-256 arrived.
pub fn copy_verified(mut r: impl Read, mut w: impl Write, sha256: &str, total: u64, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
        #[allow(unreachable_code)]
        if true { todo!("RED stub") }
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 64 * 1024];
    let mut done = 0u64;
    loop {
        let n = r.read(&mut buf).map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > total {
            return Err("Download failed: file is larger than expected".into());
        }
        hasher.update(&buf[..n]);
        w.write_all(&buf[..n]).map_err(|e| format!("Saving the model failed: {e}"))?;
        progress(done, total);
    }
    w.flush().map_err(|e| format!("Saving the model failed: {e}"))?;
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != total || hex != sha256 {
        return Err("Download failed: checksum mismatch".into());
    }
    Ok(())
}

static BUSY: Mutex<()> = Mutex::new(());

/// Download into `<app data>/models/<file>.part`, verify, then rename into place.
pub fn download(app: &AppHandle, name: &str, progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let m = info(name).ok_or_else(|| format!("Unknown model \"{name}\""))?;
    let _busy = BUSY.try_lock().map_err(|_| "A model download is already running")?;
    let dest = path(app, name)?;
    std::fs::create_dir_all(dest.parent().unwrap()).map_err(|e| e.to_string())?;
    let tmp = dest.with_extension("part");
    let resp = crate::monitors::http()
        .get(m.url)
        .config()
        .timeout_global(None)
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_recv_response(Some(Duration::from_secs(30)))
        .timeout_recv_body(Some(Duration::from_secs(60 * 60)))
        .build()
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let reader = resp.into_body().into_with_config().limit(m.size + 1).reader();
    let file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
    let r = copy_verified(reader, std::io::BufWriter::new(file), m.sha256, m.size, progress).and_then(|_| std::fs::rename(&tmp, &dest).map_err(|e| e.to_string()));
    if r.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    r.map(|_| dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_list_integrity() {
        let names: Vec<&str> = MODELS.iter().map(|m| m.name).collect();
        assert_eq!(names, ["tiny", "base", "small"]);
        for m in &MODELS {
            assert_eq!(m.url, format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{}", m.file));
            assert!(m.file.starts_with("ggml-") && !m.file.contains(".en"), "multilingual ggml only");
            assert_eq!(m.sha256.len(), 64);
            assert!(m.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
            assert!(m.size > 10_000_000);
        }
        assert!(info("large").is_none());
    }

    #[test]
    fn sha256_verify_fixture() {
        let data = b"abc";
        let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"; // FIPS 180-2 test vector
        let mut out = Vec::new();
        let mut seen = Vec::new();
        copy_verified(&data[..], &mut out, good, 3, |d, t| seen.push((d, t))).unwrap();
        assert_eq!(out, data);
        assert_eq!(seen.last(), Some(&(3, 3)));
        let bad = good.replace('b', "c");
        assert!(copy_verified(&data[..], Vec::new(), &bad, 3, |_, _| {}).is_err());
        assert!(copy_verified(&data[..], Vec::new(), good, 4, |_, _| {}).is_err()); // truncated
        assert!(copy_verified(&data[..], Vec::new(), good, 2, |_, _| {}).is_err()); // too long
    }
}
