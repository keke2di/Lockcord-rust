//! Signing in: Discord's own login page, in a separate, incognito window (WebView2 InPrivate:
//! nothing of it reaches the disk), so the password, the QR code, two-factor and any captcha are
//! all Discord's own page doing its job. LockCord never sees a password.
//!
//! Once the page is signed in, it calls Discord's API with the session token; a script that runs
//! only in that window's top frame hands the token over by starting a navigation to an address
//! that doesn't exist, which the window refuses and LockCord reads. The login window gets no IPC
//! at all. Same idea as Serein's handoff (MIT OR Apache-2.0, see THIRD_PARTY_NOTICES.md).
//!
//! The token is kept in Windows Credential Manager, one entry per profile, and only after the
//! gateway accepted it.

use crate::fingerprint;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

const HANDOFF_HOST: &str = "lockcord.invalid";
pub const LOGIN_LABEL: &str = "login";

const HANDOFF_JS: &str = r#"(() => {
  if (window !== window.top || location.origin !== "https://discord.com") return;
  const key = "__LC_KEY__";
  const opened = Date.now();
  let delivered = false;
  const ours = (value) => {
    try {
      const url = new URL(value, location.href);
      return url.origin === "https://discord.com" && /^\/api\/v\d+\//.test(url.pathname);
    } catch { return false; }
  };
  const deliver = (value) => {
    if (delivered || Date.now() - opened > 600000 || typeof value !== "string" || value.length < 16 || value.length > 2048 || /\s/.test(value)) return;
    delivered = true;
    location.href = "https://lockcord.invalid/handoff?k=" + key + "&t=" + encodeURIComponent(value);
  };
  const targets = new WeakMap();
  const open = XMLHttpRequest.prototype.open;
  const setHeader = XMLHttpRequest.prototype.setRequestHeader;
  XMLHttpRequest.prototype.open = function (method, url, ...rest) {
    targets.set(this, ours(url));
    return open.call(this, method, url, ...rest);
  };
  XMLHttpRequest.prototype.setRequestHeader = function (name, value) {
    if (targets.get(this) && String(name).toLowerCase() === "authorization") deliver(value);
    return setHeader.call(this, name, value);
  };
  const originalFetch = window.fetch;
  window.fetch = function (input, init) {
    try {
      if (ours(input instanceof Request ? input.url : input)) {
        deliver(new Headers((init && init.headers) || (input instanceof Request ? input.headers : undefined)).get("authorization"));
      }
    } catch {}
    return originalFetch.apply(this, arguments);
  };
})();"#;

pub fn plausible_token(token: &str) -> bool {
    (16..=2048).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}

// The session in the vault (vault.rs): Credential Manager, this machine only, DPAPI-wrapped.
fn name(profile: &str) -> String {
    format!("discord-session:{profile}")
}

pub fn load(profile: &str) -> Option<String> {
    crate::vault::get(&name(profile)).map(|t| t.to_string()).filter(|t| plausible_token(t))
}

pub fn save(profile: &str, token: &str) -> bool {
    crate::vault::set(&name(profile), token).is_ok()
}

pub fn forget(profile: &str) {
    crate::vault::delete(&name(profile));
}

// Only Discord, and the captcha it embeds, may load in the login window.
fn allowed(url: &url::Url) -> bool {
    if url.scheme() != "https" {
        return url.scheme() == "about";
    }
    let host = url.host_str().unwrap_or("");
    host == "discord.com" || host.ends_with(".discord.com") || host == "hcaptcha.com" || host.ends_with(".hcaptcha.com")
}

/// Opens the login window. `on_token` gets the token once, from this window only; `on_closed`
/// runs when the window goes without one.
/// `data_dir` is the profile's own WebView2 folder (main.rs `webview_dir`): two profiles can't share one.
pub fn open(app: &AppHandle, data_dir: std::path::PathBuf, on_token: impl Fn(String) + Send + Sync + 'static, on_closed: impl Fn() + Send + Sync + 'static) -> tauri::Result<()> {
    if let Some(existing) = app.get_webview_window(LOGIN_LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }
    let mut raw = [0u8; 24];
    let _ = getrandom::fill(&mut raw);
    let key: String = raw.iter().map(|b| format!("{b:02x}")).collect();
    let script = HANDOFF_JS.replace("__LC_KEY__", &key);
    let handle = app.clone();
    let delivered = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = delivered.clone();

    let window = WebviewWindowBuilder::new(app, LOGIN_LABEL, WebviewUrl::External("https://discord.com/login".parse().unwrap()))
        .title("Sign in to Discord — LockCord")
        .inner_size(520.0, 780.0)
        .incognito(true)
        .data_directory(data_dir)
        .user_agent(&fingerprint::user_agent())
        .initialization_script(&script)
        .on_navigation(move |url| {
            if url.host_str() == Some(HANDOFF_HOST) {
                let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
                if pairs.get("k") == Some(&key) && !seen.swap(true, std::sync::atomic::Ordering::SeqCst) {
                    if let Some(token) = pairs.get("t").filter(|t| plausible_token(t)) {
                        on_token(token.clone());
                    }
                    if let Some(w) = handle.get_webview_window(LOGIN_LABEL) {
                        let _ = w.destroy();
                    }
                }
                return false;
            }
            allowed(url)
        })
        .build()?;
    window.on_window_event(move |event| {
        if matches!(event, tauri::WindowEvent::Destroyed) && !delivered.load(std::sync::atomic::Ordering::SeqCst) {
            on_closed();
        }
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_discord_and_its_captcha_load_in_the_login_window() {
        for ok in ["https://discord.com/login", "https://newassets.hcaptcha.com/captcha/v1", "about:blank"] {
            assert!(allowed(&ok.parse().unwrap()), "{ok}");
        }
        for bad in ["http://discord.com/login", "https://discord.com.evil.example/", "https://evil.example/", "file:///c:/"] {
            assert!(!allowed(&bad.parse().unwrap()), "{bad}");
        }
    }

    #[test]
    fn token_shape() {
        assert!(plausible_token("abcdefghijklmnopqrstuvwxyz.123456"));
        assert!(!plausible_token("short"));
        assert!(!plausible_token("has a space in it, which no token has"));
    }
}
