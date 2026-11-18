//! Spotify Web API for the Music tab: search, your playlists, the queue, play something, like the current song.
//! Sign-in is Authorization Code + PKCE with the user's own Spotify app (Client ID in Settings → Integrations):
//! Spotify only lets an unreviewed app serve its owner's allow-listed accounts, so a shared Client ID would not work.
//! The redirect is a loopback listener on a fixed port the user registers once. The refresh token lives in the OS
//! credential store; the short-lived access token only in memory. Playback control needs Spotify Premium.

use crate::launcher::os::shell_open;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager};

pub const PORT: u16 = 43821;
pub const REDIRECT: &str = "http://127.0.0.1:43821/callback";
const AUTH: &str = "https://accounts.spotify.com/authorize";
const TOKEN_URL: &str = "https://accounts.spotify.com/api/token";
const API: &str = "https://api.spotify.com/v1";
const SCOPES: &str = "user-read-playback-state user-modify-playback-state user-read-currently-playing playlist-read-private playlist-read-collaborative user-library-read user-library-modify";
const CLIENT_KEY: &str = "spotify_client_id";
const USER_KEY: &str = "spotify_user";
const SERVICE: &str = "dynamic-notch-spotify";
/// How long the browser sign-in may take before the listener gives up.
const SIGN_IN_WAIT: Duration = Duration::from_secs(180);

struct Token {
    access: String,
    until: Instant,
}
static TOKEN: Mutex<Option<Token>> = Mutex::new(None);

// ---------- pure helpers (tested) ----------

/// base64url without padding (PKCE, RFC 7636).
pub fn b64url(bytes: &[u8]) -> String {
    crate::launcher::base64(bytes).replace('+', "-").replace('/', "_").trim_end_matches('=').to_string()
}

pub fn challenge(verifier: &str) -> String {
    b64url(&Sha256::digest(verifier.as_bytes()))
}

/// A Spotify Client ID is 32 hex characters.
pub fn valid_client_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// `spotify:<kind>:<22 base62>`, the only URIs the webview may ask us to play or queue.
pub fn valid_uri(uri: &str, kinds: &[&str]) -> bool {
    let mut p = uri.split(':');
    matches!((p.next(), p.next(), p.next(), p.next()), (Some("spotify"), Some(k), Some(id), None) if kinds.contains(&k) && valid_id(id))
}

pub fn valid_id(id: &str) -> bool {
    id.len() == 22 && id.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// Percent-encoding for a URL query value or path segment (space as %20).
fn enc(s: &str) -> String {
    crate::quicklinks::encode(s).replace('+', "%20")
}

pub fn auth_url(client_id: &str, challenge: &str, state: &str) -> String {
    format!(
        "{AUTH}?response_type=code&client_id={}&scope={}&redirect_uri={}&code_challenge_method=S256&code_challenge={}&state={}",
        enc(client_id),
        enc(SCOPES),
        enc(REDIRECT),
        enc(challenge),
        enc(state)
    )
}

/// The `code` from the browser's redirect request line (`GET /callback?code=…&state=… HTTP/1.1`), checked against `state`.
/// Ok(None): some other request (a favicon), keep waiting.
pub fn parse_callback(request_line: &str, state: &str) -> Result<Option<String>, String> {
    let Some(target) = request_line.strip_prefix("GET ").and_then(|r| r.split(' ').next()) else { return Ok(None) };
    let Some(query) = target.strip_prefix("/callback?") else { return Ok(None) };
    let get = |k: &str| query.split('&').find_map(|kv| kv.strip_prefix(&format!("{k}="))).map(str::to_string);
    if let Some(e) = get("error") {
        return Err(if e == "access_denied" { "Spotify sign-in was cancelled".into() } else { format!("Spotify refused the sign-in ({e})") });
    }
    if get("state").as_deref() != Some(state) {
        return Err("Spotify sign-in didn't match this request; try again".into()); // CSRF guard
    }
    get("code").filter(|c| !c.is_empty() && c.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))).map(Some).ok_or("Spotify sent no sign-in code".into())
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Track {
    pub uri: String,
    pub id: String,
    pub name: String,
    pub artists: String,
    pub album: String,
    pub art: Option<String>,
    pub ms: u64,
}

#[derive(Serialize, Debug, PartialEq)]
pub struct Playlist {
    pub uri: String,
    pub id: String,
    pub name: String,
    pub owner: String,
    pub art: Option<String>,
    pub count: u64,
}

/// The smallest image at least 60 px wide (list thumbnails), else the last one.
fn art(images: &Value) -> Option<String> {
    let list = images.as_array()?;
    let ok = |i: &&Value| i["url"].as_str().is_some_and(|u| u.starts_with("https://"));
    list.iter().filter(ok).filter(|i| i["width"].as_u64().is_none_or(|w| w >= 60)).min_by_key(|i| i["width"].as_u64().unwrap_or(u64::MAX)).or_else(|| list.iter().filter(ok).last())?["url"]
        .as_str()
        .map(str::to_string)
}

pub fn track(v: &Value) -> Option<Track> {
    let uri = v["uri"].as_str()?;
    if !valid_uri(uri, &["track"]) {
        return None; // local files and podcast episodes can't be played by URI here
    }
    Some(Track {
        uri: uri.into(),
        id: v["id"].as_str()?.into(),
        name: v["name"].as_str().unwrap_or("").into(),
        artists: v["artists"].as_array().map(|a| a.iter().filter_map(|x| x["name"].as_str()).collect::<Vec<_>>().join(", ")).unwrap_or_default(),
        album: v["album"]["name"].as_str().unwrap_or("").into(),
        art: art(&v["album"]["images"]),
        ms: v["duration_ms"].as_u64().unwrap_or(0),
    })
}

pub fn playlist(v: &Value) -> Option<Playlist> {
    let uri = v["uri"].as_str()?;
    if !valid_uri(uri, &["playlist"]) {
        return None;
    }
    Some(Playlist {
        uri: uri.into(),
        id: v["id"].as_str()?.into(),
        name: v["name"].as_str().unwrap_or("").into(),
        owner: v["owner"]["display_name"].as_str().unwrap_or("").into(),
        art: art(&v["images"]),
        count: v["tracks"]["total"].as_u64().or_else(|| v["items"]["total"].as_u64()).unwrap_or(0),
    })
}

// ---------- storage ----------

fn db_get(app: &AppHandle, k: &str) -> Option<String> {
    app.state::<crate::AppState>().db.lock().unwrap().get(k).ok().flatten().filter(|s| !s.is_empty())
}

fn db_set(app: &AppHandle, k: &str, v: &str) -> Result<(), String> {
    app.state::<crate::AppState>().db.lock().unwrap().set(k, v).map_err(|e| e.to_string())
}

fn secret() -> Result<keyring::Entry, String> {
    keyring::Entry::new(SERVICE, "refresh-token").map_err(|e| format!("Credential store unavailable: {e}"))
}

fn random_bytes(n: usize) -> Result<Vec<u8>, String> {
    let mut buf = vec![0u8; n];
    #[cfg(windows)]
    unsafe {
        use windows::Win32::Security::Cryptography::{BCryptGenRandom, BCRYPT_USE_SYSTEM_PREFERRED_RNG};
        BCryptGenRandom(None, &mut buf, BCRYPT_USE_SYSTEM_PREFERRED_RNG).ok().map_err(|e| e.to_string())?;
    }
    #[cfg(not(windows))]
    std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut buf)).map_err(|e| e.to_string())?;
    Ok(buf)
}

fn client_id(app: &AppHandle) -> Result<String, String> {
    db_get(app, CLIENT_KEY).ok_or_else(|| "Add your Spotify Client ID in Settings → Integrations first".into())
}

// ---------- tokens ----------

/// Keeps the access token; a new refresh token (Spotify may rotate it) replaces the stored one.
fn take_tokens(v: &Value) -> Result<String, String> {
    let access = v["access_token"].as_str().ok_or("Spotify sent no access token")?.to_string();
    let secs = v["expires_in"].as_u64().unwrap_or(3600);
    if let Some(r) = v["refresh_token"].as_str() {
        secret()?.set_password(r).map_err(|e| format!("Saving the Spotify sign-in failed: {e}"))?;
    }
    *TOKEN.lock().unwrap() = Some(Token { access: access.clone(), until: Instant::now() + Duration::from_secs(secs) });
    Ok(access)
}

fn token_request(form: &[(&str, &str)]) -> Result<Value, String> {
    let mut resp = crate::monitors::http()
        .post(TOKEN_URL)
        .config()
        .http_status_as_error(false)
        .build()
        .send_form(form.iter().copied())
        .map_err(|_| "Couldn't reach Spotify. Check your internet connection.".to_string())?;
    let status = resp.status().as_u16();
    let v: Value = resp.body_mut().read_json().unwrap_or_default();
    if status == 200 {
        return Ok(v);
    }
    if v["error"] == "invalid_grant" {
        let _ = secret().and_then(|s| s.delete_credential().map_err(|e| e.to_string()));
        return Err("Your Spotify sign-in expired. Connect again in Settings → Integrations.".into());
    }
    Err(format!("Spotify sign-in failed: {}", v["error_description"].as_str().or(v["error"].as_str()).unwrap_or("unknown error")))
}

fn access_token(app: &AppHandle) -> Result<String, String> {
    if let Some(t) = TOKEN.lock().unwrap().as_ref().filter(|t| t.until > Instant::now() + Duration::from_secs(30)) {
        return Ok(t.access.clone());
    }
    let refresh = secret()?.get_password().map_err(|_| "Spotify isn't connected. Connect it in Settings → Integrations.".to_string())?;
    let client = client_id(app)?;
    take_tokens(&token_request(&[("grant_type", "refresh_token"), ("refresh_token", &refresh), ("client_id", &client)])?)
}

pub fn connected() -> bool {
    secret().and_then(|s| s.get_password().map_err(|e| e.to_string())).is_ok()
}

// ---------- the API ----------

/// One Web API call. 204 / empty body → Null. A 401 refreshes the token once. Errors are short sentences for the UI;
/// "no-device" means Spotify has no player open to send playback to.
fn api(app: &AppHandle, method: &str, path: &str, body: Option<Value>) -> Result<Value, String> {
    for attempt in 0..2 {
        let token = access_token(app)?;
        let url = format!("{API}{path}");
        let auth = format!("Bearer {token}");
        let agent = crate::monitors::http();
        let sent = match (method, &body) {
            ("GET", _) => agent.get(&url).header("Authorization", &auth).config().http_status_as_error(false).build().call(),
            ("DELETE", _) => agent.delete(&url).header("Authorization", &auth).config().http_status_as_error(false).build().call(),
            ("PUT", Some(b)) => agent.put(&url).header("Authorization", &auth).config().http_status_as_error(false).build().send_json(b),
            ("PUT", None) => agent.put(&url).header("Authorization", &auth).config().http_status_as_error(false).build().send_empty(),
            (_, Some(b)) => agent.post(&url).header("Authorization", &auth).config().http_status_as_error(false).build().send_json(b),
            (_, None) => agent.post(&url).header("Authorization", &auth).config().http_status_as_error(false).build().send_empty(),
        };
        let mut resp = sent.map_err(|_| "Couldn't reach Spotify. Check your internet connection.".to_string())?;
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        let v: Value = serde_json::from_str(&text).unwrap_or(Value::Null);
        match status {
            200..=299 => return Ok(v),
            401 if attempt == 0 => *TOKEN.lock().unwrap() = None, // expired early: refresh and retry
            403 if v["error"]["reason"] == "PREMIUM_REQUIRED" => return Err("Playing from the island needs Spotify Premium".into()),
            404 if v["error"]["reason"] == "NO_ACTIVE_DEVICE" || path.starts_with("/me/player") => return Err("no-device".into()),
            429 => return Err("Spotify is busy. Try again in a moment.".into()),
            _ => return Err(format!("Spotify: {}", v["error"]["message"].as_str().unwrap_or("request failed"))),
        }
    }
    Err("Spotify rejected the sign-in. Connect again in Settings → Integrations.".into())
}

pub fn status(app: &AppHandle) -> Value {
    let user: Value = db_get(app, USER_KEY).and_then(|u| serde_json::from_str(&u).ok()).unwrap_or(Value::Null);
    json!({ "clientId": db_get(app, CLIENT_KEY).is_some(), "connected": connected(), "user": user["name"], "premium": user["premium"], "redirect": REDIRECT })
}

pub fn set_client(app: &AppHandle, id: &str) -> Result<Value, String> {
    let id = id.trim();
    if !id.is_empty() && !valid_client_id(id) {
        return Err("That isn't a Spotify Client ID (32 letters and digits from your app's page)".into());
    }
    if db_get(app, CLIENT_KEY).as_deref() != Some(id) {
        disconnect(app)?; // tokens belong to the old app
    }
    db_set(app, CLIENT_KEY, id)?;
    Ok(status(app))
}

pub fn disconnect(app: &AppHandle) -> Result<Value, String> {
    *TOKEN.lock().unwrap() = None;
    match secret()?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(e) => return Err(format!("Removing the Spotify sign-in failed: {e}")),
    }
    db_set(app, USER_KEY, "")?;
    Ok(status(app))
}

/// Opens Spotify's sign-in page in the browser and waits (up to 3 min) for its redirect on the loopback port.
pub fn connect(app: &AppHandle) -> Result<Value, String> {
    let client = client_id(app)?;
    let verifier = b64url(&random_bytes(48)?);
    let state = b64url(&random_bytes(16)?);
    let listener = TcpListener::bind(("127.0.0.1", PORT)).map_err(|_| format!("Port {PORT} is in use, so the Spotify sign-in can't come back. Close what uses it and try again."))?;
    listener.set_nonblocking(true).map_err(|e| e.to_string())?;
    shell_open(&auth_url(&client, &challenge(&verifier), &state))?;
    let deadline = Instant::now() + SIGN_IN_WAIT;
    let code = loop {
        if Instant::now() > deadline {
            return Err("The Spotify sign-in timed out. Try again.".into());
        }
        let Ok((mut sock, _)) = listener.accept() else {
            std::thread::sleep(Duration::from_millis(150));
            continue;
        };
        let _ = sock.set_nonblocking(false);
        let _ = sock.set_read_timeout(Some(Duration::from_secs(5)));
        let mut buf = [0u8; 8192];
        let n = sock.read(&mut buf).unwrap_or(0);
        let line = String::from_utf8_lossy(&buf[..n]).lines().next().unwrap_or("").to_string();
        let result = parse_callback(&line, &state);
        let (head, msg) = match &result {
            Ok(Some(_)) => ("200 OK", "Spotify is connected. You can close this tab and go back to Dynamic Notch."),
            Ok(None) => ("404 Not Found", ""),
            Err(e) => ("400 Bad Request", e.as_str()),
        };
        let page = format!("<!doctype html><meta charset=utf-8><title>Dynamic Notch</title><body style=\"font:16px system-ui;background:#111;color:#eee;display:grid;place-items:center;height:90vh\"><p>{}</p>", msg.replace('<', "&lt;"));
        let _ = write!(sock, "HTTP/1.1 {head}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{page}", page.len());
        match result {
            Ok(Some(code)) => break code,
            Ok(None) => continue,
            Err(e) => return Err(e),
        }
    };
    take_tokens(&token_request(&[("grant_type", "authorization_code"), ("code", &code), ("redirect_uri", REDIRECT), ("client_id", &client), ("code_verifier", &verifier)])?)?;
    if let Ok(me) = api(app, "GET", "/me", None) {
        let user = json!({ "name": me["display_name"].as_str().or(me["id"].as_str()).unwrap_or(""), "premium": me["product"] == "premium" });
        db_set(app, USER_KEY, &user.to_string())?;
    }
    Ok(status(app))
}

pub fn search(app: &AppHandle, q: &str) -> Result<Value, String> {
    let q = q.trim();
    if q.is_empty() || q.chars().count() > 200 {
        return Ok(json!({ "tracks": [], "playlists": [] }));
    }
    let v = api(app, "GET", &format!("/search?type=track,playlist&limit=10&q={}", enc(q)), None)?;
    let tracks: Vec<Track> = v["tracks"]["items"].as_array().into_iter().flatten().filter_map(track).collect();
    let playlists: Vec<Playlist> = v["playlists"]["items"].as_array().into_iter().flatten().filter_map(playlist).take(6).collect();
    Ok(json!({ "tracks": tracks, "playlists": playlists }))
}

pub fn playlists(app: &AppHandle) -> Result<Vec<Playlist>, String> {
    let v = api(app, "GET", "/me/playlists?limit=50", None)?;
    Ok(v["items"].as_array().into_iter().flatten().filter_map(playlist).collect())
}

pub fn playlist_tracks(app: &AppHandle, id: &str) -> Result<Vec<Track>, String> {
    if !valid_id(id) {
        return Err("Unknown playlist".into());
    }
    let v = api(app, "GET", &format!("/playlists/{id}/tracks?limit=100"), None)?;
    Ok(v["items"].as_array().into_iter().flatten().filter_map(|i| track(&i["track"])).collect())
}

pub fn queue(app: &AppHandle) -> Result<Value, String> {
    let v = match api(app, "GET", "/me/player/queue", None) {
        Err(e) if e == "no-device" => return Ok(json!({ "current": null, "queue": [] })),
        r => r?,
    };
    let q: Vec<Track> = v["queue"].as_array().into_iter().flatten().filter_map(track).take(20).collect();
    Ok(json!({ "current": track(&v["currently_playing"]), "queue": q }))
}

pub fn queue_add(app: &AppHandle, uri: &str) -> Result<(), String> {
    if !valid_uri(uri, &["track"]) {
        return Err("Only songs can be queued".into());
    }
    with_device(app, |dev| api(app, "POST", &format!("/me/player/queue?uri={}{dev}", enc(uri)), None).map(drop))
}

/// Play a song (`uri`) or a playlist / album (`context`, optionally starting at `offset`, a song in it).
pub fn play(app: &AppHandle, uri: Option<&str>, context: Option<&str>, offset: Option<&str>) -> Result<(), String> {
    let body = match (uri, context) {
        (Some(u), None) if valid_uri(u, &["track"]) => json!({ "uris": [u] }),
        (None, Some(c)) if valid_uri(c, &["playlist", "album"]) => match offset {
            Some(o) if valid_uri(o, &["track"]) => json!({ "context_uri": c, "offset": { "uri": o } }),
            _ => json!({ "context_uri": c }),
        },
        _ => return Err("Nothing to play".into()),
    };
    with_device(app, |dev| api(app, "PUT", &format!("/me/player/play{}", dev.replacen('&', "?", 1)), Some(body.clone())).map(drop))
}

/// Runs `f` (given "" or "&device_id=…"); when Spotify has no active player, retries on this PC's Spotify (or the
/// first device it knows).
fn with_device(app: &AppHandle, f: impl Fn(&str) -> Result<(), String>) -> Result<(), String> {
    match f("") {
        Err(e) if e == "no-device" => {
            let v = api(app, "GET", "/me/player/devices", None).unwrap_or_default();
            let devices = v["devices"].as_array().cloned().unwrap_or_default();
            let pick = devices.iter().find(|d| d["type"] == "Computer" && d["is_restricted"] != true).or_else(|| devices.iter().find(|d| d["is_restricted"] != true));
            let id = pick.and_then(|d| d["id"].as_str()).ok_or("Open Spotify on this PC (or any device) first, then try again.")?;
            f(&format!("&device_id={}", enc(id))).map_err(|e| if e == "no-device" { "Spotify didn't wake up. Play something in Spotify once, then try again.".into() } else { e })
        }
        r => r,
    }
}

/// The song Spotify is playing and whether it's in your Liked Songs (None when nothing / not a song).
pub fn current(app: &AppHandle) -> Result<Value, String> {
    let v = match api(app, "GET", "/me/player/currently-playing", None) {
        Err(e) if e == "no-device" => return Ok(Value::Null),
        r => r?,
    };
    let Some(t) = track(&v["item"]) else { return Ok(Value::Null) };
    let liked = api(app, "GET", &format!("/me/tracks/contains?ids={}", t.id), None)?.get(0).and_then(Value::as_bool).unwrap_or(false);
    Ok(json!({ "id": t.id, "name": t.name, "liked": liked }))
}

pub fn like(app: &AppHandle, id: &str, on: bool) -> Result<(), String> {
    if !valid_id(id) {
        return Err("Unknown song".into());
    }
    api(app, if on { "PUT" } else { "DELETE" }, &format!("/me/tracks?ids={id}"), None).map(drop)
}

/// Without a Spotify sign-in: open the search in the Spotify app, or on the web when the app isn't installed.
pub fn open_search(q: &str) -> Result<(), String> {
    let q = q.trim();
    if q.is_empty() || q.chars().count() > 200 {
        return Err("Type something to search for".into());
    }
    shell_open(&format!("spotify:search:{}", enc(q))).or_else(|_| shell_open(&format!("https://open.spotify.com/search/{}", enc(q))))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_challenge_matches_the_rfc_example() {
        // RFC 7636 appendix B
        assert_eq!(challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"), "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM");
        assert!(!b64url(&[0xfb, 0xff]).contains(['+', '/', '=']));
    }

    #[test]
    fn callback_needs_the_matching_state() {
        assert_eq!(parse_callback("GET /callback?code=AQB-x_1&state=s1 HTTP/1.1", "s1"), Ok(Some("AQB-x_1".into())));
        assert!(parse_callback("GET /callback?code=AQB&state=evil HTTP/1.1", "s1").is_err());
        assert!(parse_callback("GET /callback?error=access_denied&state=s1 HTTP/1.1", "s1").unwrap_err().contains("cancelled"));
        assert_eq!(parse_callback("GET /favicon.ico HTTP/1.1", "s1"), Ok(None));
        assert!(parse_callback("GET /callback?code=a%3Cb&state=s1 HTTP/1.1", "s1").is_err()); // odd characters refused
    }

    #[test]
    fn only_spotify_ids_and_uris_get_through() {
        assert!(valid_client_id("0123456789abcdef0123456789ABCDEF"));
        assert!(!valid_client_id("0123456789abcdef"));
        assert!(valid_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC", &["track"]));
        assert!(!valid_uri("spotify:track:4uLU6hMCjMI75M1A2tKUQC", &["playlist"]));
        assert!(!valid_uri("spotify:track:../../x", &["track"]));
        assert!(!valid_uri("spotify:local:abc:def:1", &["track"]));
        assert!(!valid_uri("https://evil.example", &["track"]));
    }

    #[test]
    fn tracks_and_playlists_are_simplified() {
        let t = json!({"uri": "spotify:track:4uLU6hMCjMI75M1A2tKUQC", "id": "4uLU6hMCjMI75M1A2tKUQC", "name": "Song", "duration_ms": 200000,
            "artists": [{"name": "A"}, {"name": "B"}], "album": {"name": "Al", "images": [
                {"url": "https://i.scdn.co/640", "width": 640}, {"url": "https://i.scdn.co/300", "width": 300}, {"url": "https://i.scdn.co/64", "width": 64}]}});
        let t = track(&t).unwrap();
        assert_eq!((t.artists.as_str(), t.art.as_deref(), t.ms), ("A, B", Some("https://i.scdn.co/64"), 200000));
        assert!(track(&json!({"uri": "spotify:local:x:y:z:1", "id": null})).is_none()); // local file
        let p = playlist(&json!({"uri": "spotify:playlist:37i9dQZF1DXcBWIGoYBM5M", "id": "37i9dQZF1DXcBWIGoYBM5M", "name": "Hits",
            "owner": {"display_name": "Spotify"}, "images": [{"url": "https://mosaic.scdn.co/a", "width": null}], "tracks": {"total": 50}})).unwrap();
        assert_eq!((p.owner.as_str(), p.count, p.art.as_deref()), ("Spotify", 50, Some("https://mosaic.scdn.co/a")));
    }

    #[test]
    fn sign_in_url_carries_pkce_and_the_loopback_redirect() {
        let u = auth_url("0123456789abcdef0123456789abcdef", "ch", "st");
        assert!(u.starts_with("https://accounts.spotify.com/authorize?response_type=code&client_id=0123456789abcdef0123456789abcdef"));
        assert!(u.contains("code_challenge_method=S256&code_challenge=ch&state=st"));
        assert!(u.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A43821%2Fcallback"));
        assert!(u.contains("scope=user-read-playback-state%20"));
    }
}
