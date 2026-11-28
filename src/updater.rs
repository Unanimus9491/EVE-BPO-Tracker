//! In-app updates from the GitHub releases of this repo.
//!
//! The app asks GitHub for the latest release. If it's newer than this build, it can download
//! the release's exe and swap it in: Windows won't let a running exe be overwritten, but it can be
//! renamed, so the running copy is moved aside, the new one put in its place, and the app restarts.
//! The old copy is removed on the next start.

use crate::data::{USER_AGENT, VERSION};
use serde::Deserialize;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const REPO: &str = "Unanimus9491/Slipway";
pub const ASSET_NAME: &str = "Slipway.exe";
/// The exe's name before the rename to Slipway. An update from a copy with this name installs as Slipway.exe.
const LEGACY_EXE: &str = "EVE-BPO-Tracker.exe";

pub fn releases_page() -> String {
    format!("https://github.com/{REPO}/releases/latest")
}

/// "v0.3.10" / "0.3.10" -> (0, 3, 10). Anything after a '-' (pre-release) is ignored.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let core = s.split(['-', '+']).next()?;
    let mut it = core.split('.').map(|p| p.parse::<u32>().ok());
    Some((it.next()??, it.next().flatten().unwrap_or(0), it.next().flatten().unwrap_or(0)))
}

pub fn is_newer(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(a), Some(b)) => a > b,
        _ => false,
    }
}

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub url: String,
    pub size: u64,
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    body: Option<String>,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

fn pick_update(json: &str, current: &str) -> Result<Option<UpdateInfo>, String> {
    let r: Release = serde_json::from_str(json).map_err(|e| format!("Unexpected reply from GitHub: {e}"))?;
    if r.draft || r.prerelease || !is_newer(&r.tag_name, current) {
        return Ok(None);
    }
    let asset = r
        .assets
        .iter()
        .find(|a| a.name.eq_ignore_ascii_case(ASSET_NAME))
        .ok_or_else(|| format!("Release {} has no {ASSET_NAME} attached yet. Try again in a few minutes.", r.tag_name))?;
    Ok(Some(UpdateInfo {
        version: r.tag_name.trim_start_matches(['v', 'V']).to_string(),
        notes: r.body.unwrap_or_default().trim().to_string(),
        url: asset.browser_download_url.clone(),
        size: asset.size,
    }))
}

/// Ok(None) = up to date (or no public release to compare against).
pub fn check_latest() -> Result<Option<UpdateInfo>, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut resp = match ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        // No releases yet, or the repo isn't public.
        Err(ureq::Error::StatusCode(404)) => return Ok(None),
        Err(e) => return Err(format!("Couldn't reach GitHub: {e}")),
    };
    let text = resp.body_mut().read_to_string().map_err(|e| e.to_string())?;
    pick_update(&text, VERSION)
}

// ----------------------------------------------------------------------------- background work
#[derive(Default)]
pub struct Status {
    /// Result of a check: Some(Ok(Some(..))) = update available.
    pub check: Option<Result<Option<UpdateInfo>, String>>,
    pub downloaded: u64,
    pub total: u64,
    /// Result of download + install: Ok(path of the new exe to launch).
    pub install: Option<Result<PathBuf, String>>,
}
pub type Shared = Arc<Mutex<Status>>;

pub fn spawn_check(ctx: eframe::egui::Context) -> Shared {
    let st: Shared = Default::default();
    let s = st.clone();
    std::thread::spawn(move || {
        let r = check_latest();
        if let Ok(mut g) = s.lock() {
            g.check = Some(r);
        }
        ctx.request_repaint();
    });
    st
}

pub fn spawn_install(info: UpdateInfo, ctx: eframe::egui::Context) -> Shared {
    let st: Shared = Default::default();
    let s = st.clone();
    std::thread::spawn(move || {
        let r = download_and_swap(&info, &s, &ctx);
        if let Ok(mut g) = s.lock() {
            g.install = Some(r);
        }
        ctx.request_repaint();
    });
    st
}

fn old_path(exe: &Path) -> PathBuf {
    exe.with_extension("exe.old")
}

/// Where an update gets installed: over the running exe, or as Slipway.exe beside it
/// if the running copy still has the pre-rename name.
fn target_for(exe: &Path) -> PathBuf {
    let legacy = exe.file_name().and_then(|n| n.to_str()).is_some_and(|n| n.eq_ignore_ascii_case(LEGACY_EXE));
    if legacy { exe.with_file_name(ASSET_NAME) } else { exe.to_path_buf() }
}

fn download_and_swap(info: &UpdateInfo, st: &Shared, ctx: &eframe::egui::Context) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| format!("Couldn't find the running app: {e}"))?;
    swap_in(&exe, info, &mut |done, total| {
        if let Ok(mut g) = st.lock() {
            g.downloaded = done;
            g.total = total;
        }
        ctx.request_repaint();
    })
}

/// Downloads `info` and installs it (see `target_for`), keeping the running file as `<exe>.old`.
/// Returns the path of the new exe to launch.
fn swap_in(exe: &Path, info: &UpdateInfo, progress: &mut dyn FnMut(u64, u64)) -> Result<PathBuf, String> {
    let exe = exe.to_path_buf();
    let target = target_for(&exe);
    let new = target.with_extension("exe.download");

    // 1. download next to the current exe (same drive, so the swap is a simple rename)
    let mut resp = ureq::get(&info.url)
        .header("User-Agent", USER_AGENT)
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let mut reader = resp.body_mut().as_reader();
    let mut out = std::fs::File::create(&new).map_err(|e| permission_hint(e, &exe))?;
    let mut buf = vec![0u8; 256 * 1024];
    let mut done = 0u64;
    loop {
        let n = reader.read(&mut buf).map_err(|e| format!("Download interrupted: {e}"))?;
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        done += n as u64;
        progress(done, info.size);
    }
    out.sync_all().map_err(|e| e.to_string())?;
    drop(out);

    // 2. sanity checks: complete, and actually a Windows program
    let check = (|| {
        if info.size > 0 && done != info.size {
            return Err(format!("Download was incomplete ({done} of {} bytes).", info.size));
        }
        let mut head = [0u8; 2];
        std::fs::File::open(&new).and_then(|mut f| f.read_exact(&mut head)).map_err(|e| e.to_string())?;
        if &head != b"MZ" {
            return Err("The downloaded file isn't a Windows program.".into());
        }
        Ok(())
    })();
    if let Err(e) = check {
        let _ = std::fs::remove_file(&new);
        return Err(e);
    }

    // 3. swap: move the running exe aside, put the new one in its place
    let old = old_path(&exe);
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old).map_err(|e| {
        let _ = std::fs::remove_file(&new);
        permission_hint(e, &exe)
    })?;
    if let Err(e) = std::fs::rename(&new, &target) {
        let _ = std::fs::rename(&old, &exe); // put things back
        let _ = std::fs::remove_file(&new);
        return Err(permission_hint(e, &exe));
    }
    Ok(target)
}

fn permission_hint(e: std::io::Error, exe: &Path) -> String {
    if e.kind() == std::io::ErrorKind::PermissionDenied {
        format!(
            "Windows didn't allow the app to replace itself in {}. Move the exe to a folder you own \
             (e.g. Documents or Desktop), or download the new version from the releases page.",
            exe.parent().map(|p| p.display().to_string()).unwrap_or_default()
        )
    } else {
        e.to_string()
    }
}

/// Remove the copy left behind by the last update. Call at start-up.
///
/// Right after an update the previous copy is still closing and Windows keeps its file locked,
/// so this keeps retrying in the background for up to 30 seconds.
pub fn clean_up_old() {
    let Ok(exe) = std::env::current_exe() else { return };
    let _ = std::fs::remove_file(exe.with_extension("exe.download")); // once only: an update may start soon
    let mut left = vec![old_path(&exe)];
    if let Some(dir) = exe.parent() {
        left.push(old_path(&dir.join(LEGACY_EXE))); // after the update that renamed the exe
    }
    std::thread::spawn(move || {
        for _ in 0..60 {
            left.retain(|p| p.exists() && std::fs::remove_file(p).is_err());
            if left.is_empty() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });
}

/// Start the freshly installed exe. The caller then closes this one.
pub fn relaunch(exe: &Path) -> Result<(), String> {
    std::process::Command::new(exe).spawn().map(|_| ()).map_err(|e| format!("Couldn't restart the app: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert_eq!(parse_version("v0.3.10"), Some((0, 3, 10)));
        assert_eq!(parse_version("1.2"), Some((1, 2, 0)));
        assert!(is_newer("v0.3.10", "0.3.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.3.2", "0.3.2"));
        assert!(!is_newer("v0.3.1", "0.3.2"));
        assert!(!is_newer("nonsense", "0.3.2"));
    }

    #[test]
    fn picks_release_asset() {
        let json = r#"{"tag_name":"v0.4.0","body":"New stuff","draft":false,"prerelease":false,
            "assets":[{"name":"Slipway.exe","browser_download_url":"https://github.com/x/y/releases/download/v0.4.0/Slipway.exe","size":1234}]}"#;
        let u = pick_update(json, "0.3.2").unwrap().unwrap();
        assert_eq!((u.version.as_str(), u.size, u.notes.as_str()), ("0.4.0", 1234, "New stuff"));
        assert!(pick_update(json, "0.4.0").unwrap().is_none(), "same version");
        let no_asset = r#"{"tag_name":"v0.4.0","assets":[]}"#;
        assert!(pick_update(no_asset, "0.3.2").is_err(), "release still building");
        let pre = r#"{"tag_name":"v0.5.0","prerelease":true,"assets":[]}"#;
        assert!(pick_update(pre, "0.3.2").unwrap().is_none());
    }

    /// Serves `body` once over plain HTTP on localhost.
    fn serve_once(body: Vec<u8>) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut buf = [0u8; 4096];
                let _ = s.read(&mut buf);
                let head = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(head.as_bytes());
                let _ = s.write_all(&body);
            }
        });
        format!("http://127.0.0.1:{port}/Slipway.exe")
    }

    #[test]
    fn swaps_in_new_exe_and_keeps_old() {
        let dir = std::env::temp_dir().join("bpo-test-update");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Slipway.exe");
        std::fs::write(&exe, b"MZ old version").unwrap();
        let body = b"MZ new version!".to_vec();
        let info = UpdateInfo { version: "9.9.9".into(), notes: String::new(), url: serve_once(body.clone()), size: body.len() as u64 };
        let mut last = 0;
        let out = swap_in(&exe, &info, &mut |d, _| last = d).unwrap();
        assert_eq!(out, exe);
        assert_eq!(std::fs::read(&exe).unwrap(), body);
        assert_eq!(std::fs::read(old_path(&exe)).unwrap(), b"MZ old version");
        assert_eq!(last, body.len() as u64);
    }

    #[test]
    fn rejects_bad_download_and_keeps_current() {
        let dir = std::env::temp_dir().join("bpo-test-update-bad");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("Slipway.exe");
        std::fs::write(&exe, b"MZ old version").unwrap();
        let body = b"<html>not an exe</html>".to_vec();
        let info = UpdateInfo { version: "9.9.9".into(), notes: String::new(), url: serve_once(body.clone()), size: body.len() as u64 };
        assert!(swap_in(&exe, &info, &mut |_, _| {}).is_err());
        assert_eq!(std::fs::read(&exe).unwrap(), b"MZ old version", "current exe untouched");
        assert!(!exe.with_extension("exe.download").exists(), "partial download cleaned up");
    }

    #[test]
    fn renames_legacy_exe_on_update() {
        let dir = std::env::temp_dir().join("bpo-test-update-legacy");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("EVE-BPO-Tracker.exe");
        std::fs::write(&exe, b"MZ old version").unwrap();
        let body = b"MZ new version!".to_vec();
        let info = UpdateInfo { version: "9.9.9".into(), notes: String::new(), url: serve_once(body.clone()), size: body.len() as u64 };
        let out = swap_in(&exe, &info, &mut |_, _| {}).unwrap();
        assert_eq!(out, dir.join("Slipway.exe"), "installs under the new name");
        assert_eq!(std::fs::read(&out).unwrap(), body);
        assert!(!exe.exists(), "old name gone");
        assert_eq!(std::fs::read(dir.join("EVE-BPO-Tracker.exe.old")).unwrap(), b"MZ old version");
    }

    #[test]
    fn swap_paths() {
        assert_eq!(target_for(Path::new(r"C:\Tools\Slipway.exe")), Path::new(r"C:\Tools\Slipway.exe"));
        assert_eq!(target_for(Path::new("eve-bpo-tracker.exe")), Path::new("Slipway.exe"));
        let exe = Path::new(r"C:\Tools\Slipway.exe");
        assert_eq!(old_path(exe), Path::new(r"C:\Tools\Slipway.exe.old"));
    }
}
