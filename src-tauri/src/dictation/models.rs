//! The offline speech models (sherpa-onnx exports):
//! - NVIDIA Parakeet TDT 0.6B v3 int8, about 670 MB: English + 24 European languages, fastest.
//! - Meta Omnilingual ASR 1B CTC int8, about 1.03 GB: 1600+ languages (Bangla, Hindi, English, …) with no
//!   language to pick; no punctuation or capitals. English said right after Bangla may come out in Bangla script.
//!
//! Nothing is downloaded until the user clicks Download; the files come from Hugging Face at a pinned
//! revision and are checked against their size and SHA-256 before they are renamed into place in
//! `<app data>/models/<id>`. From then on everything runs offline.
//!
//! Portions adapted from Coucou (https://github.com/Louis-CFM/coucou), © 2026 Louis Raillé, MIT License
//! (full text: THIRD_PARTY_NOTICES.md).

use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{AppHandle, Manager};

pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub size: u64,
    pub sha256: &'static str,
}

/// One downloadable model. The id is also the folder name; `model-progress` events carry it as `name`.
pub struct Model {
    pub id: &'static str,
    pub files: &'static [ModelFile],
}

pub const PARAKEET: Model = Model { id: "parakeet-tdt-0.6b-v3-int8", files: &FILES };
pub const MULTILINGUAL: Model = Model { id: "omnilingual-asr-1b-ctc-int8", files: &MULTILINGUAL_FILES };
#[cfg(test)]
pub const ID: &str = PARAKEET.id;

impl Model {
    pub fn is_multilingual(&self) -> bool {
        self.id == MULTILINGUAL.id
    }
}

// csukuangfj/sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-int8-2025-11-12 at revision 17db76ea…
// (Meta, Apache-2.0). Size and LFS SHA-256 from the HF API; tokens.txt isn't in LFS, hashed from the download.
#[cfg(test)]
const MULTILINGUAL_BASE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-int8-2025-11-12/resolve/17db76eab583b0b868ffb0df104ab879145087e5/";
pub const MULTILINGUAL_FILES: [ModelFile; 2] = [
    ModelFile {
        name: "model.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-int8-2025-11-12/resolve/17db76eab583b0b868ffb0df104ab879145087e5/model.int8.onnx",
        size: 1_031_628_252,
        sha256: "f7b74c964039162423b83e3fa950ce24810c9a635d9ff8468b5f4d142b7c1e8c",
    },
    ModelFile {
        name: "tokens.txt",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-omnilingual-asr-1600-languages-1B-ctc-int8-2025-11-12/resolve/17db76eab583b0b868ffb0df104ab879145087e5/tokens.txt",
        size: 86_423,
        sha256: "a7a044c52cb29cbe8b0dc1953e92cefd4ca16b0ed968177b6beab21f9a7d0b31",
    },
];

// URLs, sizes and hashes as Coucou pins them (speech/mod.rs, revision 2bda32ec…).
pub const FILES: [ModelFile; 4] = [
    ModelFile {
        name: "encoder.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/encoder.int8.onnx",
        size: 652_184_281,
        sha256: "acfc2b4456377e15d04f0243af540b7fe7c992f8d898d751cf134c3a55fd2247",
    },
    ModelFile {
        name: "decoder.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/decoder.int8.onnx",
        size: 11_845_275,
        sha256: "179e50c43d1a9de79c8a24149a2f9bac6eb5981823f2a2ed88d655b24248db4e",
    },
    ModelFile {
        name: "joiner.int8.onnx",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/joiner.int8.onnx",
        size: 6_355_277,
        sha256: "3164c13fc2821009440d20fcb5fdc78bff28b4db2f8d0f0b329101719c0948b3",
    },
    ModelFile {
        name: "tokens.txt",
        url: "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/tokens.txt",
        size: 93_939,
        sha256: "d58544679ea4bc6ac563d1f545eb7d474bd6cfa467f0a6e2c1dc1c7d37e3c35d",
    },
];

pub fn total_size(m: &Model) -> u64 {
    m.files.iter().map(|f| f.size).sum()
}

pub fn dir(app: &AppHandle, m: &Model) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("models").join(m.id))
}

/// The model a folder holds, by its name (`dir` puts each model in `models/<id>`).
pub fn of(dir: &Path) -> &'static Model {
    if dir.ends_with(MULTILINGUAL.id) {
        &MULTILINGUAL
    } else {
        &PARAKEET
    }
}

/// Every file is there with its size (the hash was checked when it came in).
pub fn present(dir: &Path) -> bool {
    of(dir).files.iter().all(|f| std::fs::metadata(dir.join(f.name)).is_ok_and(|m| m.len() == f.size))
}

pub fn installed(app: &AppHandle, m: &Model) -> bool {
    dir(app, m).is_ok_and(|d| present(&d))
}

/// The offline model to use: the preferred one ("multilingual" = Omnilingual, "european" = Parakeet) if
/// installed, else the other one if that is installed.
pub fn pick(pref: &str, multi: bool, parakeet: bool) -> Option<&'static Model> {
    let (first, second) = if pref == "european" { ((parakeet, &PARAKEET), (multi, &MULTILINGUAL)) } else { ((multi, &MULTILINGUAL), (parakeet, &PARAKEET)) };
    [first, second].into_iter().find(|(ok, _)| *ok).map(|(_, m)| m)
}

/// `pick` with what is installed for this app.
pub fn chosen(app: &AppHandle, pref: &str) -> Option<&'static Model> {
    pick(pref, installed(app, &MULTILINGUAL), installed(app, &PARAKEET))
}

/// Stream `r` into `w`, hashing on the way; Err unless exactly `total` bytes with the expected SHA-256 arrived.
/// `progress` gets the bytes of this file so far.
pub fn copy_verified(mut r: impl Read, mut w: impl Write, sha256: &str, total: u64, mut progress: impl FnMut(u64)) -> Result<(), String> {
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
        progress(done);
    }
    w.flush().map_err(|e| format!("Saving the model failed: {e}"))?;
    let hex: String = hasher.finalize().iter().map(|b| format!("{b:02x}")).collect();
    if done != total || hex != sha256 {
        return Err("Download failed: checksum mismatch".into());
    }
    Ok(())
}

static BUSY: Mutex<()> = Mutex::new(());

/// Download what is missing into `dir` (each file via `<name>.part`, verified, then renamed).
/// `progress(received, total)` counts over the whole set.
pub fn download_to(dir: &Path, files: &[ModelFile], mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    // a poisoned lock (earlier download panicked) is free again; only WouldBlock means busy
    let _busy = match BUSY.try_lock() {
        Ok(g) => g,
        Err(std::sync::TryLockError::Poisoned(e)) => e.into_inner(),
        Err(std::sync::TryLockError::WouldBlock) => return Err("The model download is already running".into()),
    };
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let total: u64 = files.iter().map(|f| f.size).sum();
    let mut done = 0u64;
    for f in files {
        let dest = dir.join(f.name);
        if std::fs::metadata(&dest).is_ok_and(|m| m.len() == f.size) {
            done += f.size;
            progress(done, total);
            continue;
        }
        let tmp = dir.join(format!("{}.part", f.name));
        let resp = crate::monitors::http()
            .get(f.url)
            .config()
            .timeout_global(None)
            .timeout_connect(Some(Duration::from_secs(15)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(60 * 60)))
            .build()
            .call()
            .map_err(|e| format!("Download failed ({}): {e}", f.name))?;
        let reader = resp.into_body().into_with_config().limit(f.size + 1).reader();
        let file = std::fs::File::create(&tmp).map_err(|e| e.to_string())?;
        let r = copy_verified(reader, std::io::BufWriter::new(file), f.sha256, f.size, |got| progress(done + got, total))
            .and_then(|_| std::fs::rename(&tmp, &dest).map_err(|e| e.to_string()));
        if r.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        r?;
        done += f.size;
    }
    Ok(())
}

pub fn download(app: &AppHandle, m: &Model, progress: impl FnMut(u64, u64)) -> Result<PathBuf, String> {
    let d = dir(app, m)?;
    download_to(&d, m.files, progress)?;
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASE: &str = "https://huggingface.co/csukuangfj/sherpa-onnx-nemo-parakeet-tdt-0.6b-v3-int8/resolve/2bda32ec70b097a55adaa07d9a7173915b43cc78/";

    #[test]
    fn model_files_are_pinned_and_add_up() {
        assert_eq!(total_size(&PARAKEET), 652_184_281 + 11_845_275 + 6_355_277 + 93_939);
        assert_eq!(total_size(&MULTILINGUAL), 1_031_628_252 + 86_423);
        for f in &FILES {
            assert_eq!(f.url, format!("{BASE}{}", f.name));
            assert_eq!(f.sha256.len(), 64);
            assert!(f.sha256.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
        }
        for f in &MULTILINGUAL_FILES {
            assert_eq!(f.url, format!("{MULTILINGUAL_BASE}{}", f.name));
            assert_eq!(f.sha256.len(), 64);
        }
        assert!(!present(Path::new("Z:/definitely/not/here")));
        assert!(of(Path::new("Z:/m/omnilingual-asr-1b-ctc-int8")).is_multilingual() && !of(Path::new("Z:/m/parakeet-tdt-0.6b-v3-int8")).is_multilingual());
    }

    #[test]
    fn the_preferred_model_is_used_if_installed_else_the_other() {
        let id = |m: Option<&Model>| m.map(|m| m.id);
        assert_eq!(id(pick("multilingual", true, true)), Some(MULTILINGUAL.id));
        assert_eq!(id(pick("european", true, true)), Some(PARAKEET.id));
        assert_eq!(id(pick("multilingual", false, true)), Some(PARAKEET.id)); // only Parakeet: keep it
        assert_eq!(id(pick("european", true, false)), Some(MULTILINGUAL.id));
        assert_eq!(id(pick("multilingual", false, false)), None);
    }

    #[test]
    fn sha256_verify_fixture() {
        let data = b"abc";
        let good = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"; // FIPS 180-2 test vector
        let mut out = Vec::new();
        let mut seen = Vec::new();
        copy_verified(&data[..], &mut out, good, 3, |d| seen.push(d)).unwrap();
        assert_eq!(out, data);
        assert_eq!(seen.last(), Some(&3));
        let bad = good.replace('b', "c");
        assert!(copy_verified(&data[..], Vec::new(), &bad, 3, |_| {}).is_err());
        assert!(copy_verified(&data[..], Vec::new(), good, 4, |_| {}).is_err()); // truncated
        assert!(copy_verified(&data[..], Vec::new(), good, 2, |_| {}).is_err()); // too long
    }

    /// Real network: downloads the small tokens.txt through the same code path into a temp dir.
    /// `cargo test --lib real_download -- --ignored`
    #[test]
    #[ignore]
    fn real_download_of_the_tokens_file() {
        let d = std::env::temp_dir().join(format!("dn-model-{}", std::process::id()));
        let mut last = (0, 0);
        download_to(&d, &FILES[3..], |r, t| last = (r, t)).unwrap();
        assert_eq!(last, (93_939, 93_939));
        assert_eq!(std::fs::metadata(d.join("tokens.txt")).unwrap().len(), 93_939);
        let _ = std::fs::remove_dir_all(d);
    }
}
