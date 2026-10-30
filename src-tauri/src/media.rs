//! Discord's media for the UI: attachments, embeds' images and GIFs, custom emoji, stickers and
//! avatars, served to LockCord's own window through the `lcmedia:` scheme. Fetched here, so the
//! page never loads anything from Discord itself: from Discord's own hosts only (embeds come
//! through Discord's proxy, so no third-party site is contacted), images and GIF-like videos
//! only, size-capped, and kept in a bounded memory cache.
//!
//! Uploads go the other way, the way the web client sends them: Discord hands out an upload URL
//! on its storage bucket, the file is PUT there (without the token), and the message then names
//! the uploaded file.

use crate::fingerprint;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use url::Url;

pub const SCHEME: &str = "lcmedia";
// Discord's CDN, its media proxy, and its older external-media proxies (embeds' images and GIFs).
const HOSTS: [&str; 4] = ["cdn.discordapp.com", "media.discordapp.net", "images-ext-1.discordapp.net", "images-ext-2.discordapp.net"];
const UPLOAD_HOST: &str = "discord-attachments-uploads-prd.storage.googleapis.com";
const TYPES: [&str; 6] = ["image/png", "image/jpeg", "image/gif", "image/webp", "video/mp4", "video/webm"];
const MAX_BYTES: usize = 25 * 1024 * 1024;
const CACHE_BYTES: usize = 96 * 1024 * 1024;

/// A link the media scheme will fetch: https on one of Discord's media hosts, nothing else.
pub fn allowed(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    let host = url.host_str()?;
    (url.scheme() == "https" && HOSTS.contains(&host) && url.port().is_none() && url.username().is_empty() && url.password().is_none()).then_some(url)
}

/// The HTTP client for Discord's media: a redirect is followed only to another of Discord's own
/// media hosts, five at most, so no request for media ever ends up anywhere else.
pub fn client() -> reqwest::Client {
    let policy = reqwest::redirect::Policy::custom(|attempt| {
        if attempt.previous().len() >= 5 || allowed(attempt.url().as_str()).is_none() { attempt.stop() } else { attempt.follow() }
    });
    reqwest::Client::builder().use_rustls_tls().gzip(true).redirect(policy).build().expect("media client")
}

/// Any file from Discord's hosts, whole, up to `max` bytes (saving a message's files).
pub async fn download(http: &reqwest::Client, raw: &str, max: usize) -> Result<Vec<u8>, String> {
    let url = allowed(raw).ok_or("not a Discord file")?;
    let response = http.get(url).header("User-Agent", fingerprint::user_agent()).header("Referer", "https://discord.com/").send().await.map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err(format!("download {}", response.status()));
    }
    if response.content_length().is_some_and(|n| n as usize > max) {
        return Err(format!("larger than {} MB", max / (1024 * 1024)));
    }
    let bytes = response.bytes().await.map_err(|e| e.without_url().to_string())?;
    if bytes.len() > max {
        return Err(format!("larger than {} MB", max / (1024 * 1024)));
    }
    Ok(bytes.to_vec())
}

/// Where Discord said to PUT an upload: its storage bucket, over https.
pub fn upload_allowed(raw: &str) -> Option<Url> {
    let url = Url::parse(raw).ok()?;
    (url.scheme() == "https" && url.host_str() == Some(UPLOAD_HOST) && url.port().is_none()).then_some(url)
}

/// The media type, if it's one the UI may show.
fn media_type(header: Option<&str>) -> Option<&'static str> {
    let base = header?.split(';').next()?.trim().to_ascii_lowercase();
    TYPES.iter().copied().find(|t| *t == base)
}

/// Fetched media, oldest dropped first once the total passes CACHE_BYTES.
#[derive(Default)]
pub struct Cache {
    items: HashMap<String, (&'static str, Arc<Vec<u8>>)>,
    order: VecDeque<String>,
    size: usize,
}

impl Cache {
    fn get(&self, url: &str) -> Option<(&'static str, Arc<Vec<u8>>)> {
        self.items.get(url).cloned()
    }

    fn put(&mut self, url: String, kind: &'static str, bytes: Arc<Vec<u8>>) {
        if self.items.contains_key(&url) || bytes.len() > CACHE_BYTES / 4 {
            return;
        }
        self.size += bytes.len();
        self.items.insert(url.clone(), (kind, bytes));
        self.order.push_back(url);
        while self.size > CACHE_BYTES {
            let Some(old) = self.order.pop_front() else { break };
            if let Some((_, b)) = self.items.remove(&old) {
                self.size -= b.len();
            }
        }
    }
}

pub async fn fetch(http: &reqwest::Client, cache: &std::sync::Mutex<Cache>, raw: &str) -> Result<(&'static str, Arc<Vec<u8>>), String> {
    let url = allowed(raw).ok_or("not Discord media")?;
    if let Some(hit) = cache.lock().unwrap().get(url.as_str()) {
        return Ok(hit);
    }
    // As an <img> on discord.com would ask for it.
    let response = http
        .get(url.clone())
        .header("User-Agent", fingerprint::user_agent())
        .header("Referer", "https://discord.com/")
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err(format!("media {}", response.status()));
    }
    let kind = media_type(response.headers().get("content-type").and_then(|v| v.to_str().ok())).ok_or("not an image")?;
    if response.content_length().is_some_and(|n| n as usize > MAX_BYTES) {
        return Err("too large".into());
    }
    let bytes = response.bytes().await.map_err(|e| e.without_url().to_string())?;
    if bytes.len() > MAX_BYTES {
        return Err("too large".into());
    }
    let bytes = Arc::new(bytes.to_vec());
    cache.lock().unwrap().put(url.to_string(), kind, bytes.clone());
    Ok((kind, bytes))
}

/// The scheme's path is the Discord URL, percent-encoded (the UI's `media()` in api.js).
pub fn requested_url(path: &str) -> Option<String> {
    let encoded = path.trim_start_matches('/');
    percent_encoding::percent_decode_str(encoded).decode_utf8().ok().map(|s| s.into_owned())
}

/// The largest file this account may upload: Discord's limit by Nitro tier, capped where the
/// whole file passing through memory gets silly.
pub fn upload_limit(premium_type: Option<u64>) -> usize {
    const MB: usize = 1024 * 1024;
    match premium_type {
        Some(2) => 100 * MB, // Nitro allows 500
        Some(1) | Some(3) => 50 * MB,
        _ => 10 * MB,
    }
}

/// A file name for Discord: no path, no control characters, at most 200 characters.
pub fn clean_name(name: &str) -> Option<String> {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let clean: String = base.chars().filter(|c| !c.is_control()).take(200).collect();
    (!clean.is_empty() && clean != "." && clean != "..").then_some(clean)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_discords_media_hosts_over_https() {
        assert!(allowed("https://cdn.discordapp.com/emojis/123.webp?size=48").is_some());
        assert!(allowed("https://media.discordapp.net/external/abc/https/example.com/a.png").is_some());
        assert!(allowed("https://images-ext-1.discordapp.net/external/abc/https/media.tenor.com/x.mp4").is_some());
        for bad in [
            "http://cdn.discordapp.com/a.png",
            "https://example.com/a.png",
            "https://cdn.discordapp.com.evil.com/a.png",
            "https://user@cdn.discordapp.com/a.png",
            "https://cdn.discordapp.com:444/a.png",
            "file:///C:/a.png",
        ] {
            assert!(allowed(bad).is_none(), "{bad}");
        }
        assert!(upload_allowed("https://discord-attachments-uploads-prd.storage.googleapis.com/x?upload_id=1").is_some());
        assert!(upload_allowed("https://storage.googleapis.com/x").is_none());
    }

    #[test]
    fn only_image_and_gif_video_types() {
        assert_eq!(media_type(Some("image/png")), Some("image/png"));
        assert_eq!(media_type(Some("Image/WEBP; charset=binary")), Some("image/webp"));
        assert_eq!(media_type(Some("text/html")), None);
        assert_eq!(media_type(Some("image/svg+xml")), None);
        assert_eq!(media_type(None), None);
    }

    #[test]
    fn the_scheme_path_decodes_to_the_url() {
        assert_eq!(requested_url("/https%3A%2F%2Fcdn.discordapp.com%2Femojis%2F1.webp%3Fsize%3D48").as_deref(), Some("https://cdn.discordapp.com/emojis/1.webp?size=48"));
    }

    #[test]
    fn file_names_lose_paths_and_control_characters() {
        assert_eq!(clean_name("C:\\Users\\x\\cat.png").as_deref(), Some("cat.png"));
        assert_eq!(clean_name("../../a\u{0}b.txt").as_deref(), Some("ab.txt"));
        assert_eq!(clean_name(".."), None);
        assert_eq!(clean_name(""), None);
    }

    #[test]
    fn the_cache_drops_the_oldest_past_its_limit() {
        let mut cache = Cache::default();
        let chunk = Arc::new(vec![0u8; CACHE_BYTES / 4]);
        for i in 0..5 {
            cache.put(format!("u{i}"), "image/png", chunk.clone());
        }
        assert!(cache.get("u0").is_none());
        assert!(cache.get("u4").is_some());
        assert!(cache.size <= CACHE_BYTES);
    }
}
