//! What LockCord sends, and where (Settings → Privacy, "What LockCord sends"): every request Rust
//! makes is counted here by host, for this session only, in memory. Nothing about the requests
//! themselves is kept (no paths, no contents), and nothing of it leaves the computer.
//!
//! The window's own traffic isn't in Rust's hands: a secret chat's signalling with ecdh.chat and
//! its peer-to-peer connection, hCaptcha in the captcha window, and Discord's login page in the
//! login window. The Privacy page lists those next to these counts.

use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Default)]
struct Entry {
    requests: u64,
    last: u64,
}

static LOG: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);
const MAX_HOSTS: usize = 200;

fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Counts one request to `url`'s host (a full URL, or a bare host).
pub fn note(url: &str) {
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_else(|| url.to_ascii_lowercase());
    if host.is_empty() || host.len() > 253 {
        return;
    }
    let mut log = LOG.lock().unwrap();
    let map = log.get_or_insert_with(HashMap::new);
    if !map.contains_key(&host) && map.len() >= MAX_HOSTS {
        return;
    }
    let e = map.entry(host).or_default();
    e.requests += 1;
    e.last = now_ms();
}

/// What the page shows: hosts with their purpose, most requests first.
pub fn view() -> Value {
    let log = LOG.lock().unwrap();
    let mut list: Vec<(String, Entry)> = log.as_ref().map(|m| m.iter().map(|(h, e)| (h.clone(), *e)).collect()).unwrap_or_default();
    list.sort_by(|a, b| b.1.requests.cmp(&a.1.requests).then_with(|| a.0.cmp(&b.0)));
    Value::Array(list.into_iter().map(|(host, e)| json!({ "host": host, "requests": e.requests, "last": e.last, "what": purpose(&host) })).collect())
}

/// Why LockCord talks to a host.
pub fn purpose(host: &str) -> &'static str {
    let is = |h: &str| host == h || host.ends_with(&format!(".{h}"));
    if host == "discord.com" {
        "Discord's API: your messages, chats and account"
    } else if host == "gateway.discord.gg" || is("gateway.discord.gg") {
        "Discord's live connection: new messages, typing, presence"
    } else if host.ends_with(".discord.media") {
        "Discord's voice and video servers (calls)"
    } else if is("discordapp.com") || is("discordapp.net") {
        "Discord's media: avatars, images, files"
    } else if host == "discord-attachments-uploads-prd.storage.googleapis.com" || host.ends_with("storage.googleapis.com") {
        "Discord's upload bucket: files you send"
    } else if host == "media.tenor.com" {
        "Tenor's GIFs, as Discord's picker shows them"
    } else if host == "translate-pa.googleapis.com" || host == "translate.googleapis.com" {
        "Google Translate: only when translation is on"
    } else if host == "api.github.com" || host == "github.com" || host.ends_with("githubusercontent.com") {
        "GitHub: LockCord's update check and downloads"
    } else {
        "Other"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requests_are_counted_by_host_only() {
        note("https://discord.com/api/v9/users/@me?secret=1");
        note("https://discord.com/api/v9/channels/1/messages");
        note("https://cdn.discordapp.com/avatars/1/x.png");
        let v = view();
        let discord = v.as_array().unwrap().iter().find(|e| e["host"] == "discord.com").unwrap();
        assert!(discord["requests"].as_u64().unwrap() >= 2);
        assert!(!v.to_string().contains("secret") && !v.to_string().contains("@me"), "no paths kept");
        assert_eq!(purpose("cdn.discordapp.com"), "Discord's media: avatars, images, files");
        assert_eq!(purpose("c-fra02-1a2b.discord.media"), "Discord's voice and video servers (calls)");
        assert_eq!(purpose("evil-discord.com"), "Other");
    }
}
