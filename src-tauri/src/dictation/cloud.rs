//! Cloud transcription over an OpenAI-compatible `/audio/transcriptions` endpoint (OpenAI, Groq).
//! The API key lives only in the OS credential store; it never reaches SQLite, logs or events.

use serde_json::Value;
use std::time::Duration;

const SERVICE: &str = "DynamicNotchIsland";

pub fn endpoint(provider: &str) -> Result<&'static str, String> {
    match provider {
        "openai" => Ok("https://api.openai.com/v1/audio/transcriptions"),
        "groq" => Ok("https://api.groq.com/openai/v1/audio/transcriptions"),
        _ => Err(format!("Unknown cloud provider \"{provider}\"")),
    }
}

/// 16 kHz mono 16-bit PCM WAV, in memory.
pub fn wav(samples: &[f32]) -> Vec<u8> {
    let data_len = samples.len() as u32 * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&16_000u32.to_le_bytes());
    out.extend_from_slice(&32_000u32.to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&((s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16).to_le_bytes());
    }
    out
}

/// multipart/form-data body: text fields, then one `file` part (audio/wav).
pub fn multipart(boundary: &str, fields: &[(&str, &str)], file_name: &str, file: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(file.len() + 512);
    for (k, v) in fields {
        out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"{k}\"\r\n\r\n{v}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{file_name}\"\r\nContent-Type: audio/wav\r\n\r\n").as_bytes());
    out.extend_from_slice(file);
    out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    out
}

pub fn transcribe(provider: &str, model: &str, key: &str, samples: &[f32], language: Option<&str>) -> Result<String, String> {
    let url = endpoint(provider)?;
    let boundary = format!("----DynamicNotch{:x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
    let mut fields = vec![("model", model), ("response_format", "json")];
    fields.extend(language.map(|l| ("language", l)));
    let body = multipart(&boundary, &fields, "audio.wav", &wav(samples));
    let resp = crate::monitors::http()
        .post(url)
        .header("Authorization", &format!("Bearer {key}"))
        .header("Content-Type", &format!("multipart/form-data; boundary={boundary}"))
        .config()
        .timeout_global(Some(Duration::from_secs(30)))
        .build()
        .send(&body[..]);
    let mut resp = match resp {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(401 | 403)) => return Err("Cloud API key was rejected".into()),
        Err(ureq::Error::StatusCode(429)) => return Err("Cloud transcription rate limit reached".into()),
        Err(ureq::Error::StatusCode(c)) => return Err(format!("Cloud transcription failed (HTTP {c})")),
        Err(ureq::Error::Timeout(_)) => return Err("Cloud transcription timed out".into()),
        Err(_) => return Err("Cloud transcription failed: network error".into()),
    };
    let v: Value = resp.body_mut().read_json().map_err(|_| "Cloud transcription returned an unreadable reply")?;
    v["text"].as_str().map(str::to_string).ok_or_else(|| "Cloud transcription returned no text".into())
}

fn entry(provider: &str) -> Result<keyring::Entry, String> {
    endpoint(provider)?;
    keyring::Entry::new(SERVICE, provider).map_err(|e| format!("Credential store unavailable: {e}"))
}

pub fn set_key(provider: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() || key.len() > 512 || !key.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("That doesn't look like an API key".into());
    }
    entry(provider)?.set_password(key).map_err(|e| format!("Saving the key failed: {e}"))
}

pub fn get_key(provider: &str) -> Result<String, String> {
    entry(provider)?.get_password().map_err(|_| "No cloud API key saved — add one in Settings → Dictation".into())
}

pub fn has_key(provider: &str) -> bool {
    get_key(provider).is_ok()
}

pub fn clear_key(provider: &str) -> Result<(), String> {
    match entry(provider)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(format!("Removing the key failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wav_header_bytes() {
        let w = wav(&[0.0, 1.0, -1.0, 2.0]);
        assert_eq!(w.len(), 44 + 8);
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(u32::from_le_bytes(w[4..8].try_into().unwrap()), 36 + 8);
        assert_eq!(&w[8..16], b"WAVEfmt ");
        assert_eq!(
            &w[16..36],
            &[16, 0, 0, 0, 1, 0, 1, 0, 0x80, 0x3e, 0, 0, 0x00, 0x7d, 0, 0, 2, 0, 16, 0] // fmt size, PCM, mono, 16000 Hz, 32000 B/s, align 2, 16 bit
        );
        assert_eq!(&w[36..40], b"data");
        assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 8);
        let s: Vec<i16> = w[44..].chunks(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(s, [0, 32767, -32767, 32767]); // clamped
    }

    #[test]
    fn multipart_body_format() {
        let body = multipart("XYZ", &[("model", "whisper-1"), ("language", "de")], "audio.wav", b"RIFFdata");
        let expected = "--XYZ\r\nContent-Disposition: form-data; name=\"model\"\r\n\r\nwhisper-1\r\n\
                        --XYZ\r\nContent-Disposition: form-data; name=\"language\"\r\n\r\nde\r\n\
                        --XYZ\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\nRIFFdata\r\n\
                        --XYZ--\r\n";
        assert_eq!(String::from_utf8(body).unwrap(), expected);
    }

    #[test]
    fn providers() {
        assert!(endpoint("openai").unwrap().ends_with("/v1/audio/transcriptions"));
        assert!(endpoint("groq").unwrap().starts_with("https://api.groq.com/"));
        assert!(endpoint("evil").is_err());
        assert!(set_key("evil", "sk-123").is_err());
        assert!(set_key("openai", "  ").is_err()); // rejected before touching the store
        assert!(set_key("openai", "sk 123").is_err());
    }
}
