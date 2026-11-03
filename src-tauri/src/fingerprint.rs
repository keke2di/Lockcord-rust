//! The one identity LockCord presents to Discord, on REST, the gateway and the login window.
//!
//! Discord's anti-spam judges a user session by how its client presents itself: a custom user
//! agent, or no `X-Super-Properties` header, reads as an automated tool and gets the account
//! restricted. So LockCord presents itself as the web client in the browser engine it actually
//! runs, with the web client's current build number: Chrome on Windows (WebView2 is Chromium),
//! Safari on macOS (WKWebView is WebKit). Nothing here carries account data; it's the same for
//! everyone on the same system.
//!
//! Adapted from Serein's `client_core::fingerprint` (MIT OR Apache-2.0, see
//! THIRD_PARTY_NOTICES.md).

use base64::Engine;
use std::sync::OnceLock;

/// The system's language (e.g. "fi-FI"), which is what the browser engine, and so Discord's own
/// login page, reports. Claiming another would be one more mismatch.
pub fn system_locale() -> &'static str {
    static LOCALE: OnceLock<String> = OnceLock::new();
    LOCALE.get_or_init(|| {
        sys_locale::get_locale()
            .filter(|l| !l.is_empty() && l.len() <= 16 && l.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'))
            .unwrap_or_else(|| "en-US".to_owned())
    })
}

/// Discord's name for that language (`X-Discord-Locale`): most are the bare language, a few keep
/// their region.
pub fn discord_locale() -> String {
    discord_locale_of(system_locale())
}

fn discord_locale_of(locale: &str) -> String {
    let lang = locale.split('-').next().unwrap_or("en").to_ascii_lowercase();
    match lang.as_str() {
        "en" => if locale.eq_ignore_ascii_case("en-GB") { "en-GB".into() } else { "en-US".into() },
        "sv" => "sv-SE".into(),
        "es" => "es-ES".into(),
        "pt" => "pt-BR".into(),
        "zh" => if locale.to_ascii_uppercase().contains("TW") { "zh-TW".into() } else { "zh-CN".into() },
        _ => lang,
    }
}

/// The web client's build number when this was written; replaced at startup by the live one
/// (`refresh_build_number`), since Discord gates some routes on a recent build.
const FALLBACK_BUILD_NUMBER: u64 = 653_945;

static BUILD_NUMBER: OnceLock<u64> = OnceLock::new();
static CHROME_MAJOR: OnceLock<String> = OnceLock::new();

/// The Chromium major version of the WebView2 runtime, which is what the login page really runs
/// in. A captcha solved in one engine but submitted under another's name reads as relayed.
pub fn set_engine_version(webview_version: &str) {
    let major = webview_version.split('.').next().filter(|m| m.chars().all(|c| c.is_ascii_digit()) && !m.is_empty());
    let _ = CHROME_MAJOR.set(major.unwrap_or("140").to_owned());
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn chrome_major() -> &'static str {
    CHROME_MAJOR.get().map(String::as_str).unwrap_or("140")
}

pub fn build_number() -> u64 {
    *BUILD_NUMBER.get().unwrap_or(&FALLBACK_BUILD_NUMBER)
}

/// Reads `BUILD_NUMBER` from `window.GLOBAL_ENV` on discord.com/app, the page every browser loads.
pub async fn refresh_build_number(http: &reqwest::Client) {
    crate::netlog::note("https://discord.com/app");
    let Ok(response) = http.get("https://discord.com/app").header("User-Agent", user_agent()).send().await else { return };
    let Ok(html) = response.text().await else { return };
    if let Some(number) = parse_build_number(&html) {
        let _ = BUILD_NUMBER.set(number);
    }
}

pub fn parse_build_number(html: &str) -> Option<u64> {
    let at = html.find("\"BUILD_NUMBER\":\"")? + "\"BUILD_NUMBER\":\"".len();
    let digits: String = html[at..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok().filter(|n| *n > 100_000 && *n < 100_000_000)
}

#[cfg(not(target_os = "macos"))]
pub fn user_agent() -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{}.0.0.0 Safari/537.36",
        chrome_major()
    )
}

/// Safari's user agent, as it reads on every Mac (the OS and WebKit versions in it are frozen;
/// only `Version/` moves, with the Safari that comes with the system).
#[cfg(target_os = "macos")]
pub fn user_agent() -> String {
    format!("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{} Safari/605.1.15", safari_version())
}

/// The Safari version that comes with this macOS (macOS 26 ships Safari 26, 15 ships 18, …): the
/// WebKit the login window runs is that one.
#[cfg(target_os = "macos")]
fn safari_version() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(|| {
        let os = std::process::Command::new("/usr/bin/sw_vers").arg("-productVersion").output().ok()
            .and_then(|o| String::from_utf8(o.stdout).ok()).unwrap_or_default();
        safari_for(os.trim())
    })
}

#[cfg(any(target_os = "macos", test))]
fn safari_for(macos: &str) -> String {
    let mut parts = macos.split('.').map(|p| p.parse::<u32>().ok());
    let (major, minor) = (parts.next().flatten(), parts.next().flatten().unwrap_or(0));
    let safari = match major {
        Some(m) if m >= 26 => m,
        Some(15) => 18,
        Some(14) => 17,
        Some(13) => 16,
        _ => return "18.6".to_owned(),
    };
    format!("{safari}.{minor}")
}

/// What the properties say about the browser and the system.
#[cfg(not(target_os = "macos"))]
fn platform() -> (&'static str, &'static str, String, &'static str) {
    ("Windows", "Chrome", format!("{}.0.0.0", chrome_major()), "10")
}

#[cfg(target_os = "macos")]
fn platform() -> (&'static str, &'static str, String, &'static str) {
    ("Mac OS X", "Safari", safari_version().to_owned(), "10.15.7")
}

/// The gateway's `identify.properties`, and the decoded `X-Super-Properties`.
pub fn properties() -> serde_json::Value {
    let (os, browser, browser_version, os_version) = platform();
    serde_json::json!({
        "os": os,
        "browser": browser,
        "device": "",
        "system_locale": system_locale(),
        "browser_user_agent": user_agent(),
        "browser_version": browser_version,
        "os_version": os_version,
        "referrer": "",
        "referring_domain": "",
        "referrer_current": "",
        "referring_domain_current": "",
        "release_channel": "stable",
        "client_build_number": build_number(),
        "client_event_source": null,
    })
}

pub fn super_properties() -> String {
    base64::engine::general_purpose::STANDARD.encode(properties().to_string())
}

const DISCORD_EPOCH_MS: u128 = 1_420_070_400_000;

/// A snowflake-shaped message nonce, like the official client's.
pub fn nonce(unix_ms: u128, sequence: u64) -> String {
    let elapsed = unix_ms.saturating_sub(DISCORD_EPOCH_MS).min(u64::MAX as u128) as u64;
    ((elapsed << 22) | (sequence & 0x3F_FFFF)).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_number_comes_from_global_env() {
        let html = r#"<script>window.GLOBAL_ENV = {"NODE_ENV":"production","BUILD_NUMBER":"653945","PROJECT_ENV":"production"}</script>"#;
        assert_eq!(parse_build_number(html), Some(653_945));
        assert_eq!(parse_build_number("no env here"), None);
        assert_eq!(parse_build_number(r#""BUILD_NUMBER":"12""#), None);
    }

    #[test]
    fn properties_match_the_user_agent() {
        let p = properties();
        assert_eq!(p["browser_user_agent"], user_agent());
        if cfg!(target_os = "macos") {
            assert!(user_agent().contains(&format!("Version/{} Safari/", p["browser_version"].as_str().unwrap())));
            assert_eq!(p["browser"], "Safari");
        } else {
            assert!(user_agent().contains(&format!("Chrome/{}.0.0.0", chrome_major())));
            assert_eq!(p["browser"], "Chrome");
        }
        let decoded = base64::engine::general_purpose::STANDARD.decode(super_properties()).unwrap();
        assert_eq!(serde_json::from_slice::<serde_json::Value>(&decoded).unwrap(), p);
    }

    #[test]
    fn safari_follows_macos() {
        assert_eq!(safari_for("15.5"), "18.5");
        assert_eq!(safari_for("14.7.1"), "17.7");
        assert_eq!(safari_for("26.1"), "26.1");
        assert_eq!(safari_for(""), "18.6");
    }

    #[test]
    fn discord_locales() {
        assert_eq!(discord_locale_of("fi-FI"), "fi");
        assert_eq!(discord_locale_of("en-GB"), "en-GB");
        assert_eq!(discord_locale_of("en-AU"), "en-US");
        assert_eq!(discord_locale_of("pt-PT"), "pt-BR");
        assert_eq!(discord_locale_of("zh-TW"), "zh-TW");
    }

    #[test]
    fn nonce_is_a_snowflake() {
        let n: u64 = nonce(1_700_000_000_000, 5).parse().unwrap();
        assert_eq!(n & 0x3F_FFFF, 5);
        assert_eq!((n >> 22) as u128 + DISCORD_EPOCH_MS, 1_700_000_000_000);
    }
}
