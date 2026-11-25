//! EVE Online login (SSO with PKCE, so no secret is needed) and blueprint sync via ESI.
//!
//! Characters and their refresh tokens are stored in `accounts.json` in the data folder.

use crate::data::{USER_AGENT, atomic_write};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const CLIENT_ID: &str = "397ffb1171354fa6a686dd5c37603d5d";
pub const CALLBACK_PORT: u16 = 51791;
pub const REDIRECT_URI: &str = "http://localhost:51791/callback";
pub const SCOPES: &str = "esi-characters.read_blueprints.v1";
const AUTHORIZE_URL: &str = "https://login.eveonline.com/v2/oauth/authorize/";
const TOKEN_URL: &str = "https://login.eveonline.com/v2/oauth/token";
const ESI_BASE: &str = "https://esi.evetech.net";
/// ESI behaviour version this code was written against.
const COMPATIBILITY_DATE: &str = "2025-09-30";
const LOGIN_TIMEOUT: Duration = Duration::from_secs(300);

// ----------------------------------------------------------------------------- accounts
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub character_id: u64,
    pub name: String,
    pub refresh_token: String,
    /// Set when the token stops working and the character needs to log in again.
    #[serde(default)]
    pub needs_login: bool,
}

pub fn load_accounts(path: &Path) -> Vec<Account> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_accounts(path: &Path, accounts: &[Account]) {
    if let Ok(json) = serde_json::to_vec_pretty(accounts) {
        let _ = atomic_write(path, &json);
    }
}

// ----------------------------------------------------------------------------- helpers
fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn url_decode(s: &str) -> String {
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(h), Some(l)) => {
                    out.push(h * 16 + l);
                    i += 2;
                }
                _ => out.push(b'%'),
            },
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn random_token(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    // Falls back to a time-based value only if the OS RNG is unavailable (practically never).
    if getrandom::fill(&mut buf).is_err() {
        let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
        for (i, b) in buf.iter_mut().enumerate() {
            *b = (t.as_nanos() >> (i % 16 * 8)) as u8 ^ i as u8;
        }
    }
    URL_SAFE_NO_PAD.encode(buf)
}

fn pkce_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

pub fn authorize_url(state: &str, challenge: &str) -> String {
    format!(
        "{AUTHORIZE_URL}?response_type=code&redirect_uri={}&client_id={CLIENT_ID}&scope={}&code_challenge={challenge}&code_challenge_method=S256&state={state}",
        url_encode(REDIRECT_URI),
        url_encode(SCOPES),
    )
}

pub fn open_in_browser(url: &str) {
    #[cfg(windows)]
    let _ = std::process::Command::new("rundll32").args(["url.dll,FileProtocolHandler", url]).spawn();
    #[cfg(target_os = "macos")]
    let _ = std::process::Command::new("open").arg(url).spawn();
    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = std::process::Command::new("xdg-open").arg(url).spawn();
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
}

/// Character ID and name from the access token (a JWT). ESI checks the signature itself
/// whenever the token is used, so it's only read here, not verified.
fn character_from_jwt(token: &str) -> Result<(u64, String), String> {
    let payload = token.split('.').nth(1).ok_or("Unexpected token format")?;
    let bytes = URL_SAFE_NO_PAD
        .decode(payload.trim_end_matches('='))
        .map_err(|e| format!("Unexpected token format: {e}"))?;
    #[derive(Deserialize)]
    struct Claims {
        sub: String,
        name: String,
    }
    let c: Claims = serde_json::from_slice(&bytes).map_err(|e| format!("Unexpected token contents: {e}"))?;
    let id = c
        .sub
        .rsplit(':')
        .next()
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| format!("Unexpected character id in token: {}", c.sub))?;
    Ok((id, c.name))
}

fn token_request(form: &[(&str, &str)]) -> Result<TokenResponse, TokenError> {
    let body: String = form.iter().map(|(k, v)| format!("{k}={}", url_encode(v))).collect::<Vec<_>>().join("&");
    let mut resp = ureq::post(TOKEN_URL)
        .header("Content-Type", "application/x-www-form-urlencoded")
        .header("Host", "login.eveonline.com")
        .header("User-Agent", USER_AGENT)
        .send(body.as_str())
        .map_err(|e| match e {
            ureq::Error::StatusCode(400 | 401 | 403) => TokenError::Rejected,
            e => TokenError::Other(format!("EVE login server: {e}")),
        })?;
    let text = resp.body_mut().read_to_string().map_err(|e| TokenError::Other(e.to_string()))?;
    serde_json::from_str(&text).map_err(|e| TokenError::Other(format!("Unexpected login response: {e}")))
}

enum TokenError {
    /// The refresh token was revoked or expired: the character must log in again.
    Rejected,
    Other(String),
}

// ----------------------------------------------------------------------------- login flow
#[derive(Default)]
pub struct LoginState {
    pub url: String,
    pub result: Option<Result<Account, String>>,
}

pub struct Login {
    pub state: Arc<Mutex<LoginState>>,
    pub cancel: Arc<AtomicBool>,
}

/// Starts a login: listens on localhost for EVE's redirect, opens the browser, and swaps
/// the returned code for tokens. Poll `state` for the result.
pub fn start_login(ctx: eframe::egui::Context) -> Login {
    let verifier = random_token(32);
    let challenge = pkce_challenge(&verifier);
    let csrf = random_token(16);
    let url = authorize_url(&csrf, &challenge);
    let state = Arc::new(Mutex::new(LoginState { url: url.clone(), result: None }));
    let cancel = Arc::new(AtomicBool::new(false));
    let (st, cn) = (state.clone(), cancel.clone());
    std::thread::spawn(move || {
        let result = run_login(&url, &csrf, &verifier, &cn);
        if !cn.load(Ordering::SeqCst) {
            if let Ok(mut g) = st.lock() {
                g.result = Some(result);
            }
            ctx.request_repaint();
        }
    });
    Login { state, cancel }
}

fn run_login(url: &str, csrf: &str, verifier: &str, cancel: &AtomicBool) -> Result<Account, String> {
    // Browsers may resolve "localhost" to IPv4 or IPv6, so listen on both where possible.
    let mut listeners = Vec::new();
    for addr in [format!("127.0.0.1:{CALLBACK_PORT}"), format!("[::1]:{CALLBACK_PORT}")] {
        if let Ok(l) = TcpListener::bind(&addr) {
            let _ = l.set_nonblocking(true);
            listeners.push(l);
        }
    }
    if listeners.is_empty() {
        return Err(format!(
            "Couldn't listen on port {CALLBACK_PORT} for the login result. Is another copy of the tracker open?"
        ));
    }
    open_in_browser(url);

    let started = Instant::now();
    let code = loop {
        if cancel.load(Ordering::SeqCst) {
            return Err("Cancelled".into());
        }
        if started.elapsed() > LOGIN_TIMEOUT {
            return Err("Timed out waiting for the EVE login. Please try again.".into());
        }
        let mut got = None;
        for l in &listeners {
            if let Ok((stream, _)) = l.accept() {
                got = handle_callback(stream, csrf);
                if got.is_some() {
                    break;
                }
            }
        }
        match got {
            Some(Ok(code)) => break code,
            Some(Err(e)) => return Err(e),
            None => std::thread::sleep(Duration::from_millis(100)),
        }
    };

    let tokens = token_request(&[
        ("grant_type", "authorization_code"),
        ("code", &code),
        ("client_id", CLIENT_ID),
        ("code_verifier", verifier),
    ])
    .map_err(|e| match e {
        TokenError::Rejected => "EVE rejected the login code. Please try again.".to_string(),
        TokenError::Other(m) => m,
    })?;
    let (character_id, name) = character_from_jwt(&tokens.access_token)?;
    Ok(Account { character_id, name, refresh_token: tokens.refresh_token, needs_login: false })
}

/// Reads one HTTP request. Returns None for requests that aren't the login callback (e.g. favicon).
fn handle_callback(mut stream: TcpStream, csrf: &str) -> Option<Result<String, String>> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let mut buf = [0u8; 8192];
    let n = stream.read(&mut buf).ok()?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let path = req.lines().next()?.split_whitespace().nth(1)?.to_string();
    if !path.starts_with("/callback") {
        let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
        return None;
    }
    let query = path.split_once('?').map(|(_, q)| q).unwrap_or("");
    let params: HashMap<String, String> = query
        .split('&')
        .filter_map(|kv| kv.split_once('='))
        .map(|(k, v)| (k.to_string(), url_decode(v)))
        .collect();
    let result = if params.get("state").map(String::as_str) != Some(csrf) {
        Err("The login response didn't match this request. Please try again.".to_string())
    } else if let Some(code) = params.get("code") {
        Ok(code.clone())
    } else {
        Err(params
            .get("error_description")
            .or_else(|| params.get("error"))
            .cloned()
            .unwrap_or_else(|| "Login was cancelled.".into()))
    };
    let (title, msg) = match &result {
        Ok(_) => ("Logged in", "You can close this tab and go back to the tracker."),
        Err(_) => ("Login failed", "Go back to the tracker for details."),
    };
    let html = format!(
        "<!DOCTYPE html><html><head><meta charset=utf-8><title>EVE BPO Tracker</title></head>\
         <body style=\"margin:0;height:100vh;display:flex;align-items:center;justify-content:center;\
         background:#181615;color:#ece6dc;font-family:Segoe UI,Inter,sans-serif\">\
         <div style=\"background:#232120;border:1px solid #34302d;border-radius:12px;padding:32px 40px;text-align:center\">\
         <div style=\"color:#FFAE00;font-size:22px;font-weight:700;margin-bottom:8px\">{title}</div>\
         <div style=\"color:#9a928a\">{msg}</div></div></body></html>"
    );
    let _ = stream.write_all(
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{html}",
            html.len()
        )
        .as_bytes(),
    );
    Some(result)
}

// ----------------------------------------------------------------------------- blueprint sync
#[derive(Deserialize, Debug, Clone)]
pub struct EsiBlueprint {
    pub type_id: u32,
    /// -1: a single original, -2: a copy, positive: a stack of originals.
    pub quantity: i64,
    pub material_efficiency: i64,
    pub time_efficiency: i64,
}

/// Per blueprint type: how many originals, and the best ME/TE among them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Owned {
    pub qty: u32,
    pub me: u8,
    pub te: u8,
}

/// One line of the duplicate breakdown: originals a character holds at a given ME/TE.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SyncedOriginal {
    pub character: String,
    pub me: u8,
    pub te: u8,
    pub count: u32,
}

/// Adds one character's blueprints. Cards show the total count and the best ME/TE;
/// `details` keeps every distinct original for the side panel.
pub fn summarise(
    character: &str,
    bps: &[EsiBlueprint],
    into: &mut HashMap<u32, Owned>,
    details: &mut HashMap<u32, Vec<SyncedOriginal>>,
) {
    for bp in bps {
        let count = match bp.quantity {
            -1 => 1,
            q if q > 0 => q as u32,
            _ => continue, // copies don't count
        };
        let me = bp.material_efficiency.clamp(0, 10) as u8;
        let te = bp.time_efficiency.clamp(0, 20) as u8;
        let o = into.entry(bp.type_id).or_default();
        o.qty += count;
        o.me = o.me.max(me);
        o.te = o.te.max(te);
        let list = details.entry(bp.type_id).or_default();
        match list.iter_mut().find(|d| d.character == character && d.me == me && d.te == te) {
            Some(d) => d.count += count,
            None => list.push(SyncedOriginal { character: character.to_string(), me, te, count }),
        }
    }
    for list in details.values_mut() {
        // best researched first
        list.sort_by(|a, b| (b.me, b.te).cmp(&(a.me, a.te)).then_with(|| a.character.cmp(&b.character)));
    }
}

fn fetch_blueprints(character_id: u64, access_token: &str) -> Result<Vec<EsiBlueprint>, String> {
    let mut all = Vec::new();
    let mut page = 1;
    loop {
        let url = format!("{ESI_BASE}/characters/{character_id}/blueprints?page={page}");
        let mut resp = ureq::get(&url)
            .header("Authorization", &format!("Bearer {access_token}"))
            .header("X-Compatibility-Date", COMPATIBILITY_DATE)
            .header("User-Agent", USER_AGENT)
            .header("Accept", "application/json")
            .call()
            .map_err(|e| format!("ESI blueprints request failed: {e}"))?;
        let pages: u32 = resp
            .headers()
            .get("x-pages")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(1);
        let text = resp.body_mut().with_config().limit(50 * 1024 * 1024).read_to_string().map_err(|e| e.to_string())?;
        let mut batch: Vec<EsiBlueprint> =
            serde_json::from_str(&text).map_err(|e| format!("Unexpected blueprint data from ESI: {e}"))?;
        all.append(&mut batch);
        if page >= pages {
            break;
        }
        page += 1;
    }
    Ok(all)
}

#[derive(Default)]
pub struct SyncResult {
    pub owned: HashMap<u32, Owned>,
    pub details: HashMap<u32, Vec<SyncedOriginal>>,
    /// Characters that synced successfully.
    pub synced: Vec<String>,
    /// Problems, per character.
    pub errors: Vec<String>,
    /// Accounts with refreshed tokens / login flags, to save back.
    pub accounts: Vec<Account>,
}

pub struct Sync {
    pub result: Arc<Mutex<Option<SyncResult>>>,
}

pub fn start_sync(accounts: Vec<Account>, ctx: eframe::egui::Context) -> Sync {
    let result = Arc::new(Mutex::new(None));
    let r = result.clone();
    std::thread::spawn(move || {
        let mut out = SyncResult::default();
        for mut acc in accounts {
            if acc.needs_login {
                out.errors.push(format!("{}: needs to log in again", acc.name));
                out.accounts.push(acc);
                continue;
            }
            let tokens = token_request(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", &acc.refresh_token),
                ("client_id", CLIENT_ID),
            ]);
            match tokens {
                Ok(t) => {
                    acc.refresh_token = t.refresh_token; // EVE rotates refresh tokens
                    match fetch_blueprints(acc.character_id, &t.access_token) {
                        Ok(bps) => {
                            summarise(&acc.name, &bps, &mut out.owned, &mut out.details);
                            out.synced.push(acc.name.clone());
                        }
                        Err(e) => out.errors.push(format!("{}: {e}", acc.name)),
                    }
                }
                Err(TokenError::Rejected) => {
                    acc.needs_login = true;
                    out.errors.push(format!("{}: login expired, please log in again", acc.name));
                }
                Err(TokenError::Other(e)) => out.errors.push(format!("{}: {e}", acc.name)),
            }
            out.accounts.push(acc);
        }
        if let Ok(mut g) = r.lock() {
            *g = Some(out);
        }
        ctx.request_repaint();
    });
    Sync { result }
}

pub fn accounts_path(dir: &Path) -> PathBuf {
    dir.join("accounts.json")
}

pub fn details_path(dir: &Path) -> PathBuf {
    dir.join("sync_details.json")
}

pub fn load_details(path: &Path) -> HashMap<u32, Vec<SyncedOriginal>> {
    std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

pub fn save_details(path: &Path, details: &HashMap<u32, Vec<SyncedOriginal>>) {
    if let Ok(json) = serde_json::to_vec(details) {
        let _ = atomic_write(path, &json);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_rfc7636_example() {
        assert_eq!(
            pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn authorize_url_is_encoded() {
        let u = authorize_url("st", "ch");
        assert!(u.contains("redirect_uri=http%3A%2F%2Flocalhost%3A51791%2Fcallback"));
        assert!(u.contains("scope=esi-characters.read_blueprints.v1"));
        assert!(u.contains("client_id=397ffb1171354fa6a686dd5c37603d5d"));
        assert!(u.contains("code_challenge_method=S256"));
    }

    #[test]
    fn reads_character_from_jwt() {
        let payload = URL_SAFE_NO_PAD.encode(br#"{"sub":"CHARACTER:EVE:2112345678","name":"Unan Test","scp":"x"}"#);
        let token = format!("aGVhZGVy.{payload}.c2ln");
        assert_eq!(character_from_jwt(&token).unwrap(), (2112345678, "Unan Test".to_string()));
    }

    #[test]
    fn summarises_originals_only() {
        let json = r#"[
            {"item_id":1,"location_flag":"Hangar","location_id":60003760,"material_efficiency":9,"quantity":-1,"runs":-1,"time_efficiency":10,"type_id":32881},
            {"item_id":2,"location_flag":"Hangar","location_id":60003760,"material_efficiency":10,"quantity":-1,"runs":-1,"time_efficiency":4,"type_id":32881},
            {"item_id":3,"location_flag":"Hangar","location_id":60003760,"material_efficiency":10,"quantity":-2,"runs":10,"time_efficiency":20,"type_id":691},
            {"item_id":4,"location_flag":"Hangar","location_id":60003760,"material_efficiency":0,"quantity":3,"runs":-1,"time_efficiency":0,"type_id":1153}
        ]"#;
        let bps: Vec<EsiBlueprint> = serde_json::from_str(json).unwrap();
        let mut owned = HashMap::new();
        let mut details = HashMap::new();
        summarise("Alt", &bps, &mut owned, &mut details);
        assert_eq!(owned[&32881], Owned { qty: 2, me: 10, te: 10 });
        assert_eq!(details[&32881].len(), 2);
        assert_eq!((details[&32881][0].me, details[&32881][0].te), (10, 4), "best ME first");
        assert_eq!(owned[&1153], Owned { qty: 3, me: 0, te: 0 });
        assert!(!owned.contains_key(&691), "copies are ignored");
    }

    #[test]
    fn decodes_callback_params() {
        assert_eq!(url_decode("abc%2Bdef%3D%3D"), "abc+def==");
        assert_eq!(url_decode("a+b"), "a b");
    }

    #[test]
    fn callback_round_trip() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
            s.write_all(b"GET /callback?code=abc%2D123&state=xyz HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
            let mut out = String::new();
            let _ = s.read_to_string(&mut out);
            out
        });
        let (stream, _) = listener.accept().unwrap();
        let r = handle_callback(stream, "xyz").unwrap();
        assert_eq!(r.unwrap(), "abc-123");
        assert!(t.join().unwrap().contains("Logged in"));
    }
}
