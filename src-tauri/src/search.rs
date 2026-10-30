//! Message search: the query string the web client sends to Discord's search, built from what the
//! UI asks for (search.js reads `from:`, `has:` and the rest; it sends ids and times). Everything
//! is checked here before it goes into the URL. The parameters follow Concord's
//! `src/discord/rest/search.rs` (GPL-3.0, see THIRD_PARTY_NOTICES.md).

use crate::deletion::snowflake_at;
use percent_encoding::{NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

pub const PAGE: u32 = 25;
const MAX_OFFSET: u32 = 9_975;
const MAX_CONTENT: usize = 1024;
const HAS: [&str; 8] = ["link", "embed", "file", "image", "video", "sound", "sticker", "poll"];
const DAY_MS: u64 = 24 * 3600 * 1000;

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

fn ids<'a>(v: &'a Value, max: usize) -> Result<Vec<&'a str>, String> {
    let list: Vec<&str> = v.as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if list.len() > max || !list.iter().all(|s| is_id(s)) {
        return Err("bad search".into());
    }
    Ok(list)
}

/// The query string for a search: { content, author_id: [ids], mentions: [ids], has: [kinds],
/// channel_id: [ids] (a server's channels; `channel_ok` says which may be named), pinned,
/// after / before / during (ms) }. A search with nothing in it is refused.
pub fn params(q: &Value, offset: u32, channel_ok: impl Fn(&str) -> bool) -> Result<String, String> {
    let mut out: Vec<(&str, String)> = vec![("limit", PAGE.to_string()), ("offset", offset.min(MAX_OFFSET).to_string()), ("sort_by", "timestamp".into()), ("sort_order", "desc".into())];
    let mut any = false;
    if let Some(content) = q["content"].as_str().map(str::trim).filter(|c| !c.is_empty()) {
        out.push(("content", content.chars().take(MAX_CONTENT).collect()));
        any = true;
    }
    for (key, name) in [("author_id", "author_id"), ("mentions", "mentions")] {
        for id in ids(&q[key], 10)? {
            out.push((name, id.to_owned()));
            any = true;
        }
    }
    for id in ids(&q["channel_id"], 10)? {
        if !channel_ok(id) {
            return Err("You can't search that channel.".into());
        }
        out.push(("channel_id", id.to_owned()));
        any = true;
    }
    for kind in q["has"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        if !HAS.contains(&kind) {
            return Err("bad search".into());
        }
        out.push(("has", kind.to_owned()));
        any = true;
    }
    if let Some(p) = q["pinned"].as_bool() {
        out.push(("pinned", p.to_string()));
        any = true;
    }
    // Times as snowflake bounds: after a day, before a day, or during one.
    let (mut min, mut max) = (q["after"].as_u64(), q["before"].as_u64());
    if let Some(day) = q["during"].as_u64() {
        min = Some(day);
        max = Some(day + DAY_MS);
    }
    if let Some(ms) = min {
        out.push(("min_id", snowflake_at(ms).to_string()));
        any = true;
    }
    if let Some(ms) = max {
        out.push(("max_id", snowflake_at(ms).to_string()));
        any = true;
    }
    if !any {
        return Err("Type something to search for.".into());
    }
    Ok(out.iter().map(|(k, v)| format!("{k}={}", utf8_percent_encode(v, NON_ALPHANUMERIC))).collect::<Vec<_>>().join("&"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn queries_are_built_and_checked() {
        let q = json!({ "content": "hello there", "author_id": ["5"], "has": ["image", "link"], "channel_id": ["10"], "pinned": true });
        let p = params(&q, 25, |c| c == "10").unwrap();
        assert_eq!(p, "limit=25&offset=25&sort_by=timestamp&sort_order=desc&content=hello%20there&author_id=5&channel_id=10&has=image&has=link&pinned=true");
        // A day is a pair of snowflake bounds.
        let day = 1_700_000_000_000u64;
        let p = params(&json!({ "during": day }), 0, |_| false).unwrap();
        assert!(p.contains(&format!("min_id={}", snowflake_at(day))) && p.contains(&format!("max_id={}", snowflake_at(day + DAY_MS))));
        assert!(params(&json!({ "content": "  " }), 0, |_| true).is_err(), "empty");
        assert!(params(&json!({ "channel_id": ["10"] }), 0, |_| false).is_err(), "a channel it can't see");
        assert!(params(&json!({ "author_id": ["5&x=1"] }), 0, |_| true).is_err());
        assert!(params(&json!({ "has": ["everything"] }), 0, |_| true).is_err());
        assert!(params(&json!({ "content": "a&b=c" }), 99_999, |_| true).unwrap().contains("offset=9975&sort_by=timestamp&sort_order=desc&content=a%26b%3Dc"));
    }
}
