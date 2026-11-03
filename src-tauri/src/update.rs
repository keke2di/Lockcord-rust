//! The update check: at most once a day (and on "Check now"), unless turned off in Settings → App.
//! It asks GitHub's API for the newest release of LockCord's own repository and, when that's
//! newer, offers it. Updating downloads that release's installer, checks it against the sha256
//! GitHub publishes for it, and starts it once the user agrees. A portable copy only opens the
//! release page. Nothing about the user is sent: no token, no ids, just LockCord's user agent.

use sha2::{Digest, Sha256};
use serde_json::{Value, json};
use std::io::Write;
use std::path::{Path, PathBuf};

pub const FEED: &str = "https://api.github.com/repos/keke2di/Lockcord-rust/releases/latest";
const MAX_NOTES: usize = 8 * 1024;
const MAX_INSTALLER: u64 = 64 * 1024 * 1024;
/// Hosts an installer download may be served from (GitHub redirects release assets).
const ASSET_HOSTS: [&str; 3] = ["github.com", "objects.githubusercontent.com", "release-assets.githubusercontent.com"];
/// How long a check counts: the next automatic one waits this long.
pub const CHECK_EVERY_MS: u64 = 20 * 60 * 60 * 1000;

/// A newer release, as the window and the download need it.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: String,
    pub notes: String,
    pub page: String,
    pub asset: String,
    pub size: u64,
    pub digest: Option<[u8; 32]>,
}

impl Release {
    pub fn view(&self) -> Value {
        json!({ "version": self.version, "notes": self.notes, "page": self.page, "size": self.size })
    }
}

/// "1.2.3" or "v1.2.3" as numbers; anything else isn't a version.
fn parse_version(v: &str) -> Option<(u64, u64, u64)> {
    let mut parts = v.trim().trim_start_matches('v').split('.');
    let n = |p: Option<&str>| p.and_then(|s| s.parse::<u64>().ok());
    let version = (n(parts.next())?, n(parts.next())?, n(parts.next())?);
    parts.next().is_none().then_some(version)
}

pub fn newer(current: &str, candidate: &str) -> bool {
    matches!((parse_version(current), parse_version(candidate)), (Some(c), Some(n)) if n > c)
}

fn digest_of(v: &Value) -> Option<[u8; 32]> {
    let hex = v.as_str()?.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// GitHub's "latest release" answer: the release if it's newer than `current` and has our
/// installer. Drafts and prereleases never count.
pub fn parse_latest(release: &Value, current: &str) -> Option<Release> {
    if release["draft"] == true || release["prerelease"] == true {
        return None;
    }
    let version = release["tag_name"].as_str()?.trim_start_matches('v').to_owned();
    parse_version(&version)?;
    if !newer(current, &version) {
        return None;
    }
    let name = format!("LockCord_{version}_x64-setup.exe");
    let asset = release["assets"].as_array()?.iter().find(|a| a["name"] == name.as_str())?;
    let url = asset["browser_download_url"].as_str().filter(|u| allowed_asset_url(u))?.to_owned();
    let notes: String = release["body"].as_str().unwrap_or("").chars().take(MAX_NOTES).collect();
    let page = release["html_url"].as_str().filter(|u| u.starts_with("https://github.com/")).unwrap_or("https://github.com/keke2di/Lockcord-rust/releases").to_owned();
    Some(Release { version, notes, page, asset: url, size: asset["size"].as_u64().unwrap_or(0), digest: digest_of(&asset["digest"]) })
}

/// An https address on one of GitHub's release hosts (or, in debug builds, a local test feed).
pub fn allowed_asset_url(address: &str) -> bool {
    let Ok(url) = url::Url::parse(address) else { return false };
    let host = url.host_str().unwrap_or("");
    if cfg!(debug_assertions) && url.scheme() == "http" && host == "127.0.0.1" {
        return true;
    }
    url.scheme() == "https" && ASSET_HOSTS.contains(&host)
}

/// Whether this copy was installed (the installer leaves its uninstaller beside the exe): only
/// then is an update installed for you; a portable copy is pointed at the release page.
pub fn installed() -> bool {
    std::env::current_exe().ok().and_then(|exe| exe.parent().map(|d| d.join("uninstall.exe").is_file())).unwrap_or(false)
}

/// The feed to ask: GitHub's, or in debug builds the `--update-feed=` given for testing.
pub fn feed() -> String {
    if cfg!(debug_assertions) {
        if let Some(f) = std::env::args().find_map(|a| a.strip_prefix("--update-feed=").map(str::to_owned)) {
            return f;
        }
    }
    FEED.to_owned()
}

pub async fn check(http: &reqwest::Client, current: &str) -> Result<Option<Release>, String> {
    crate::netlog::note(&feed());
    let answer = http
        .get(feed())
        .header("Accept", "application/vnd.github+json")
        .header("User-Agent", format!("LockCord/{current}"))
        .send()
        .await
        .map_err(|e| format!("Couldn't reach GitHub: {e}"))?;
    // Not found: the repository is private or gone (it always has releases), so "you're up to
    // date" would be a guess.
    if answer.status().as_u16() == 404 {
        return Err("GitHub didn't show LockCord's releases (is the repository public?).".into());
    }
    if !answer.status().is_success() {
        return Err(format!("GitHub answered {}", answer.status().as_u16()));
    }
    let body: Value = answer.json().await.map_err(|_| "GitHub's answer wasn't readable.".to_owned())?;
    Ok(parse_latest(&body, current))
}

/// Where the installer is downloaded to.
pub fn download_path() -> PathBuf {
    std::env::temp_dir().join("LockCord-update").join("LockCord-setup.exe")
}

/// Downloads the release's installer, following redirects only onto GitHub's hosts, and keeps it
/// only if its sha256 is the one GitHub published. `progress` hears (bytes so far, total).
pub async fn download(release: &Release, current: &str, progress: impl Fn(u64, u64)) -> Result<PathBuf, String> {
    let expected = release.digest.ok_or("This release has no published checksum, so it isn't installed automatically. Download it from the release page.")?;
    let client = reqwest::Client::builder()
        .use_rustls_tls()
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            if attempt.previous().len() < 5 && allowed_asset_url(attempt.url().as_str()) { attempt.follow() } else { attempt.stop() }
        }))
        .build()
        .map_err(|e| e.to_string())?;
    crate::netlog::note(&release.asset);
    let mut answer = client.get(&release.asset).header("User-Agent", format!("LockCord/{current}")).send().await.map_err(|e| format!("The download failed: {e}"))?;
    if !answer.status().is_success() {
        return Err(format!("The download failed ({}).", answer.status().as_u16()));
    }
    let total = answer.content_length().unwrap_or(release.size);
    if total > MAX_INSTALLER {
        return Err("That installer is far too big.".into());
    }
    let path = download_path();
    let dir = path.parent().ok_or("no folder")?;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let mut file = std::fs::File::create(&path).map_err(|e| e.to_string())?;
    let mut hash = Sha256::new();
    let mut done = 0u64;
    while let Some(chunk) = answer.chunk().await.map_err(|e| format!("The download failed: {e}"))? {
        done += chunk.len() as u64;
        if done > MAX_INSTALLER {
            drop(file);
            let _ = std::fs::remove_file(&path);
            return Err("That installer is far too big.".into());
        }
        hash.update(&chunk);
        file.write_all(&chunk).map_err(|e| e.to_string())?;
        progress(done, total);
    }
    file.flush().map_err(|e| e.to_string())?;
    drop(file);
    if !verify(&hash.finalize().into(), &expected) {
        let _ = std::fs::remove_file(&path);
        return Err("The downloaded installer didn't match its published checksum, so it was deleted.".into());
    }
    Ok(path)
}

fn verify(got: &[u8; 32], expected: &[u8; 32]) -> bool {
    got == expected
}

/// The file at `path` still has the sha256 it was downloaded with (checked again just before it's
/// started, so nothing swapped it in between).
pub fn still_matches(path: &Path, expected: &[u8; 32]) -> bool {
    std::fs::read(path).map(|bytes| verify(&Sha256::digest(&bytes).into(), expected)).unwrap_or(false)
}

/// Starts the installer (per user, so no administrator prompt).
#[cfg(windows)]
pub fn start_installer(path: &Path) -> Result<(), String> {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, file) = (wide("open"), wide(&path.to_string_lossy()));
    // SAFETY: both strings are NUL-terminated and outlive the call; the rest are null.
    let result = unsafe { ShellExecuteW(std::ptr::null_mut(), verb.as_ptr(), file.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL) };
    if result as isize <= 32 {
        return Err("Windows couldn't start the installer.".into());
    }
    Ok(())
}

/// Elsewhere there's no installer to start: `update_install` opens the release page instead
/// (`installed` is false there).
#[cfg(not(windows))]
pub fn start_installer(_path: &Path) -> Result<(), String> {
    Err("Updating in place isn't available here; the release page has the new version.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str, digest: Option<&str>) -> Value {
        let v = tag.trim_start_matches('v');
        json!({
            "tag_name": tag, "draft": false, "prerelease": false, "body": "Notes", "html_url": "https://github.com/keke2di/Lockcord-rust/releases/tag/".to_owned() + tag,
            "assets": [
                { "name": format!("LockCord_{v}_x64-portable.exe"), "size": 1, "browser_download_url": "https://github.com/x/portable" },
                { "name": format!("LockCord_{v}_x64-setup.exe"), "size": 5, "digest": digest, "browser_download_url": "https://github.com/keke2di/Lockcord-rust/releases/download/a/setup.exe" },
            ],
        })
    }

    #[test]
    fn versions_compare_as_numbers() {
        assert!(newer("1.1.0", "1.2.0"));
        assert!(newer("1.9.0", "1.10.0"));
        assert!(newer("1.1.0", "v2.0.0"));
        assert!(!newer("1.2.0", "1.2.0"));
        assert!(!newer("1.2.0", "1.1.9"));
        assert!(!newer("1.2.0", "1.3"));
        assert!(!newer("1.2.0", "1.3.0-beta"));
    }

    #[test]
    fn the_installer_and_its_digest_are_picked() {
        let hex = "ab".repeat(32);
        let r = parse_latest(&release("v1.2.0", Some(&format!("sha256:{hex}"))), "1.1.0").unwrap();
        assert_eq!(r.version, "1.2.0");
        assert!(r.asset.ends_with("setup.exe"));
        assert_eq!(r.digest, Some([0xab; 32]));
        assert_eq!(r.size, 5);
        assert!(parse_latest(&release("v1.2.0", None), "1.1.0").unwrap().digest.is_none(), "no digest: offered, never installed by itself");
        assert!(parse_latest(&release("v1.2.0", Some("md5:00")), "1.1.0").unwrap().digest.is_none());
        assert!(parse_latest(&release("v1.1.0", None), "1.1.0").is_none(), "not newer");
        let mut pre = release("v1.3.0", None);
        pre["prerelease"] = json!(true);
        assert!(parse_latest(&pre, "1.1.0").is_none());
        let mut off = release("v1.3.0", None);
        off["assets"][1]["browser_download_url"] = json!("https://evil.example/setup.exe");
        assert!(parse_latest(&off, "1.1.0").is_none(), "an installer off GitHub's hosts");
    }

    #[test]
    fn downloads_stay_on_githubs_hosts() {
        assert!(allowed_asset_url("https://github.com/keke2di/Lockcord-rust/releases/download/v1/a.exe"));
        assert!(allowed_asset_url("https://release-assets.githubusercontent.com/x"));
        assert!(allowed_asset_url("https://objects.githubusercontent.com/x"));
        assert!(!allowed_asset_url("http://github.com/x"));
        assert!(!allowed_asset_url("https://github.com.evil.example/x"));
        assert!(!allowed_asset_url("https://gist.github.com/x"));
    }

    #[test]
    fn a_changed_file_fails_the_check() {
        let path = std::env::temp_dir().join(format!("lockcord-update-test-{}.bin", std::process::id()));
        std::fs::write(&path, b"installer bytes").unwrap();
        let good: [u8; 32] = Sha256::digest(b"installer bytes").into();
        assert!(still_matches(&path, &good));
        std::fs::write(&path, b"swapped").unwrap();
        assert!(!still_matches(&path, &good));
        let _ = std::fs::remove_file(&path);
    }
}
