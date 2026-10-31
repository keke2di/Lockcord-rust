//! GIFs for the picker, got the way Discord's own picker gets them: its `/gifs` endpoints (Tenor
//! behind them) for trending and search, and the favourites kept in the account's frecency
//! settings protobuf (settings-proto 2), read only. Results are cut down to what the picker draws;
//! previews load through the media scheme (media.rs allows media.tenor.com for them), and a GIF
//! is sent as its link, as Discord sends it.

use base64::Engine;
use serde_json::{Value, json};

use crate::{media, people};

/// The most GIFs one answer gives the window.
pub const MAX: usize = 50;
const MAX_FAVORITES: usize = 500;

/// The query string for Discord's GIF endpoints, with the search terms (if any) encoded.
pub fn query(q: Option<&str>, locale: &str) -> String {
    let mut s = url::form_urlencoded::Serializer::new(String::new());
    if let Some(q) = q {
        s.append_pair("q", q);
    }
    s.append_pair("media_format", "mp4").append_pair("provider", "tenor").append_pair("locale", locale);
    if q.is_some() {
        s.append_pair("limit", &MAX.to_string());
    }
    s.finish()
}

// A preview the media scheme will load (Discord's hosts or Tenor's), or None.
fn preview(v: &Value) -> Option<&str> {
    v.as_str().filter(|u| media::allowed(u).is_some())
}

/// One GIF as the picker takes it: the link to send, a preview and its size.
pub fn gif_view(g: &Value) -> Option<Value> {
    let url = g["url"].as_str().filter(|u| u.starts_with("https://") && u.len() <= 512 && !u.contains(char::is_whitespace))?;
    let src = preview(&g["src"]).or_else(|| preview(&g["gif_src"]))?;
    Some(json!({ "url": url, "src": src, "width": g["width"].as_u64().unwrap_or(0), "height": g["height"].as_u64().unwrap_or(0) }))
}

/// A search's (or trending GIFs') answer: a list of GIFs.
pub fn gifs_view(body: &Value) -> Value {
    json!(body.as_array().into_iter().flatten().filter_map(gif_view).take(MAX).collect::<Vec<_>>())
}

/// Trending: the categories (a name and a preview each), and one trending GIF for its own tile.
pub fn trending_view(body: &Value) -> Value {
    let categories: Vec<Value> = body["categories"].as_array().into_iter().flatten().take(40)
        .filter_map(|c| Some(json!({ "name": c["name"].as_str().filter(|n| !n.is_empty() && n.len() <= 64)?, "src": preview(&c["src"])? })))
        .collect();
    let first = body["gifs"].as_array().and_then(|g| g.first()).and_then(|g| preview(&g["src"]));
    json!({ "categories": categories, "preview": first })
}

/// The favourite GIFs in a frecency settings protobuf (base64), the latest favourite first:
/// FavoriteGIFs is field 2, its map field 1; an entry's key (1) is the link and its value (2) a
/// FavoriteGIF (src 2, width 3, height 4, order 5).
pub fn favorites(encoded: &str) -> Value {
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(encoded) else { return json!([]) };
    let Some(favs) = people::len_field(&bytes, 2) else { return json!([]) };
    let mut out: Vec<(u64, Value)> = Vec::new();
    for (number, wire, range, _) in people::fields(favs) {
        if number != 1 || wire != 2 || out.len() >= MAX_FAVORITES {
            continue;
        }
        let entry = &favs[range];
        let Some(url) = people::len_field(entry, 1).and_then(|k| std::str::from_utf8(k).ok()) else { continue };
        let Some(gif) = people::len_field(entry, 2) else { continue };
        let src = people::len_field(gif, 2).and_then(|s| std::str::from_utf8(s).ok()).unwrap_or("");
        let view = gif_view(&json!({
            "url": url, "src": src,
            "width": people::varint_field(gif, 3).unwrap_or(0), "height": people::varint_field(gif, 4).unwrap_or(0),
        }));
        if let Some(view) = view {
            out.push((people::varint_field(gif, 5).unwrap_or(0), view));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    json!(out.into_iter().map(|(_, v)| v).collect::<Vec<_>>())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn varint(out: &mut Vec<u8>, mut v: u64) {
        while v >= 0x80 {
            out.push((v as u8) | 0x80);
            v >>= 7;
        }
        out.push(v as u8);
    }
    fn len(out: &mut Vec<u8>, field: u32, value: &[u8]) {
        varint(out, u64::from(field << 3 | 2));
        varint(out, value.len() as u64);
        out.extend_from_slice(value);
    }
    fn num(out: &mut Vec<u8>, field: u32, value: u64) {
        varint(out, u64::from(field << 3));
        varint(out, value);
    }
    fn entry(url: &str, src: &str, order: u64) -> Vec<u8> {
        let mut gif = Vec::new();
        num(&mut gif, 1, 2);
        len(&mut gif, 2, src.as_bytes());
        num(&mut gif, 3, 220);
        num(&mut gif, 4, 124);
        num(&mut gif, 5, order);
        let mut e = Vec::new();
        len(&mut e, 1, url.as_bytes());
        len(&mut e, 2, &gif);
        e
    }

    #[test]
    fn favorites_come_from_the_frecency_protobuf() {
        let mut favs = Vec::new();
        len(&mut favs, 1, &entry("https://tenor.com/view/old-123", "https://media.tenor.com/a/old.mp4", 1));
        len(&mut favs, 1, &entry("https://tenor.com/view/new-456", "https://media.tenor.com/b/new.mp4", 7));
        // A preview from anywhere else isn't loaded, so that favourite isn't offered.
        len(&mut favs, 1, &entry("https://example.com/x.gif", "https://example.com/x.gif", 9));
        let mut settings = Vec::new();
        len(&mut settings, 1, &[8, 1]);
        len(&mut settings, 2, &favs);
        let v = favorites(&base64::engine::general_purpose::STANDARD.encode(&settings));
        assert_eq!(v.as_array().unwrap().len(), 2);
        assert_eq!(v[0], json!({ "url": "https://tenor.com/view/new-456", "src": "https://media.tenor.com/b/new.mp4", "width": 220, "height": 124 }));
        assert_eq!(v[1]["url"], "https://tenor.com/view/old-123");
        assert_eq!(favorites("not base64!"), json!([]));
        assert_eq!(favorites(""), json!([]));
    }

    #[test]
    fn answers_are_cut_down() {
        let body = json!([
            { "id": "1", "title": "t", "url": "https://tenor.com/view/a-1", "src": "https://media.tenor.com/x/a.mp4", "gif_src": "https://media.tenor.com/x/a.gif", "width": 498, "height": 280, "extra": 1 },
            { "url": "javascript:alert(1)", "src": "https://media.tenor.com/x/b.mp4" },
            { "url": "https://tenor.com/view/c-3", "src": "https://evil.example/c.mp4" },
        ]);
        assert_eq!(gifs_view(&body), json!([{ "url": "https://tenor.com/view/a-1", "src": "https://media.tenor.com/x/a.mp4", "width": 498, "height": 280 }]));
        let trending = json!({ "categories": [{ "name": "happy", "src": "https://media.tenor.com/h.mp4" }, { "name": "", "src": "https://media.tenor.com/e.mp4" }], "gifs": [{ "src": "https://media.tenor.com/t.mp4" }] });
        assert_eq!(trending_view(&trending), json!({ "categories": [{ "name": "happy", "src": "https://media.tenor.com/h.mp4" }], "preview": "https://media.tenor.com/t.mp4" }));
        assert_eq!(query(Some("cat & dog"), "en-US"), "q=cat+%26+dog&media_format=mp4&provider=tenor&locale=en-US&limit=50");
        assert_eq!(query(None, "en-US"), "media_format=mp4&provider=tenor&locale=en-US");
    }
}
