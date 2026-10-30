//! People: profiles as the profile popout shows them, and this account's own status.
//!
//! The status lives in Discord's settings protobuf (`/users/@me/settings-proto/1`, field 11), as
//! the web client keeps it; LockCord edits only that field and leaves the rest as it found it.
//! The protobuf reading and writing is adapted from Concord's `src/discord/rest/user_settings.rs`
//! (GPL-3.0, see THIRD_PARTY_NOTICES.md).

use base64::Engine;
use serde_json::{Value, json};

const VERSIONS_FIELD: u32 = 1;
const DATA_VERSION_FIELD: u32 = 3;
const STATUS_FIELD: u32 = 11;
// Inside the status settings: the status, a string wrapper.
const STATUS_VALUE_FIELD: u32 = 1;

pub const STATUSES: [&str; 4] = ["online", "idle", "dnd", "invisible"];

/// A user as the profile popout shows them, cut down from `/users/{id}/profile`.
pub fn profile_view(body: &Value) -> Value {
    let u = &body["user"];
    let p = &body["user_profile"];
    let member = &body["guild_member"];
    let text = |v: &Value| v.as_str().filter(|s| !s.is_empty()).map(str::to_owned);
    let mutual: Vec<Value> = body["mutual_guilds"].as_array().into_iter().flatten().take(100)
        .filter_map(|g| Some(json!({ "id": g["id"].as_str()?, "nick": g["nick"] }))).collect();
    json!({
        "user": {
            "id": u["id"], "username": u["username"], "global_name": u["global_name"], "avatar": u["avatar"],
            "discriminator": u["discriminator"], "bot": u["bot"].as_bool().unwrap_or(false),
        },
        "banner": text(&p["banner"]).or_else(|| text(&u["banner"])),
        "accent": p["accent_color"].as_u64().or(u["accent_color"].as_u64()),
        "pronouns": text(&body["guild_member_profile"]["pronouns"]).or_else(|| text(&p["pronouns"])),
        "bio": text(&body["guild_member_profile"]["bio"]).or_else(|| text(&p["bio"])),
        "member": if member.is_object() { json!({ "nick": member["nick"], "roles": member["roles"], "joined_at": member["joined_at"] }) } else { Value::Null },
        "mutual_guilds": mutual,
        "mutual_friends": body["mutual_friends_count"].as_u64(),
        "premium_since": body["premium_since"],
    })
}

/// This account's status: { status }.
pub fn status_view(status: &str) -> Value {
    json!({ "status": status })
}

/// READY's status: the settings protobuf when it's there, else the older JSON settings.
pub fn ready_status(data: &Value) -> Value {
    if let Some(v) = data["user_settings_proto"].as_str().and_then(|p| status_of_proto(p, "online")) {
        return v;
    }
    status_view(data["user_settings"]["status"].as_str().filter(|s| STATUSES.contains(s)).unwrap_or("online"))
}

/// The status in a base64 settings protobuf (READY's, or USER_SETTINGS_PROTO_UPDATE's); `was`
/// stands in when it names none (a partial update that changed something else).
pub fn status_of_proto(encoded: &str, was: &str) -> Option<Value> {
    let bytes = base64::engine::general_purpose::STANDARD.decode(encoded).ok()?;
    let status = len_field(&bytes, STATUS_FIELD)?;
    let value = len_field(status, STATUS_VALUE_FIELD).and_then(|w| len_field(w, 1)).and_then(|s| std::str::from_utf8(s).ok()).unwrap_or(was);
    Some(status_view(if STATUSES.contains(&value) { value } else { "online" }))
}

/// The PATCH body that sets the status on top of the account's current settings protobuf
/// (base64), keeping whatever else the status settings hold as it is.
pub fn status_patch(current: &str, status: &str) -> Result<Value, String> {
    let current = base64::engine::general_purpose::STANDARD.decode(current).map_err(|_| "Discord's settings didn't decode.")?;
    let old = len_field(&current, STATUS_FIELD).unwrap_or_default();
    let mut value = Vec::new();
    write_len(&mut value, 1, status.as_bytes());
    let mut updated = without(old, &[STATUS_VALUE_FIELD])?;
    write_len(&mut updated, STATUS_VALUE_FIELD, &value);
    let mut partial = Vec::new();
    write_len(&mut partial, STATUS_FIELD, &updated);
    let mut body = json!({ "settings": base64::engine::general_purpose::STANDARD.encode(partial) });
    if let Some(v) = len_field(&current, VERSIONS_FIELD).and_then(|v| varint_field(v, DATA_VERSION_FIELD)) {
        body["required_data_version"] = json!(v);
    }
    Ok(body)
}

/// The gateway's presence update (op 3) for this session, as the web client sends it after a
/// status change.
pub fn presence_frame(status: &str) -> Value {
    json!({ "op": 3, "d": { "status": status, "since": 0, "activities": [], "afk": false } })
}

/// Discord's usernames: 2-32 of lowercase letters, digits, `_` and `.`.
pub fn username_ok(name: &str) -> bool {
    (2..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'.')
}

// --- A little protobuf ----------------------------------------------------------------------------

fn write_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn write_len(out: &mut Vec<u8>, field: u32, value: &[u8]) {
    write_varint(out, u64::from(field << 3 | 2));
    write_varint(out, value.len() as u64);
    out.extend_from_slice(value);
}

fn read_varint(b: &[u8], at: &mut usize) -> Option<u64> {
    let mut v = 0u64;
    for shift in (0..64).step_by(7) {
        let byte = *b.get(*at)?;
        *at += 1;
        v |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(v);
        }
    }
    None
}

// One field at `at`: (number, wire type, value range, end).
fn field_at(b: &[u8], at: usize) -> Option<(u32, u8, std::ops::Range<usize>, usize)> {
    let mut c = at;
    let key = read_varint(b, &mut c)?;
    let (number, wire) = (u32::try_from(key >> 3).ok()?, (key & 7) as u8);
    let start = c;
    let end = match wire {
        0 => {
            read_varint(b, &mut c)?;
            c
        }
        1 => c.checked_add(8)?,
        2 => {
            let len = usize::try_from(read_varint(b, &mut c)?).ok()?;
            let s = c;
            return (s.checked_add(len)? <= b.len()).then_some((number, wire, s..s + len, s + len));
        }
        5 => c.checked_add(4)?,
        _ => return None,
    };
    (end <= b.len()).then_some((number, wire, start..end, end))
}

fn fields(b: &[u8]) -> impl Iterator<Item = (u32, u8, std::ops::Range<usize>, usize)> + '_ {
    let mut at = 0;
    std::iter::from_fn(move || {
        if at >= b.len() {
            return None;
        }
        let f = field_at(b, at)?;
        at = f.3;
        Some(f)
    })
}

fn len_field(b: &[u8], field: u32) -> Option<&[u8]> {
    fields(b).filter(|f| f.0 == field && f.1 == 2).last().map(|f| &b[f.2])
}

fn varint_field(b: &[u8], field: u32) -> Option<u64> {
    fields(b).filter(|f| f.0 == field && f.1 == 0).last().and_then(|f| read_varint(b, &mut f.2.start.clone()))
}

// The message without the given fields; an unreadable one is refused rather than half kept.
fn without(b: &[u8], drop: &[u32]) -> Result<Vec<u8>, String> {
    let mut out = Vec::with_capacity(b.len());
    let mut at = 0;
    while at < b.len() {
        let (number, _, _, end) = field_at(b, at).ok_or("Discord's settings didn't decode.")?;
        if !drop.contains(&number) {
            out.extend_from_slice(&b[at..end]);
        }
        at = end;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b64(b: &[u8]) -> String {
        base64::engine::general_purpose::STANDARD.encode(b)
    }

    // Settings with a data version, a status with a sibling field, and an unrelated top-level one.
    fn settings(status: &str) -> Vec<u8> {
        let mut versions = Vec::new();
        write_varint(&mut versions, u64::from(DATA_VERSION_FIELD << 3));
        write_varint(&mut versions, 42);
        let mut wrapper = Vec::new();
        write_len(&mut wrapper, 1, status.as_bytes());
        let mut st = Vec::new();
        write_len(&mut st, STATUS_VALUE_FIELD, &wrapper);
        write_len(&mut st, 3, &[8, 1]);
        let mut all = Vec::new();
        write_len(&mut all, VERSIONS_FIELD, &versions);
        write_len(&mut all, STATUS_FIELD, &st);
        write_len(&mut all, 99, b"unrelated");
        all
    }

    #[test]
    fn status_reads_from_the_settings_protobuf() {
        assert_eq!(status_of_proto(&b64(&settings("dnd")), "online").unwrap(), json!({ "status": "dnd" }));
        assert_eq!(status_of_proto(&b64(&settings("weird")), "online").unwrap()["status"], "online");
        assert!(status_of_proto("not base64!", "online").is_none());
        assert_eq!(ready_status(&json!({ "user_settings": { "status": "idle" } })), json!({ "status": "idle" }));
        assert_eq!(ready_status(&json!({})), json!({ "status": "online" }));
    }

    #[test]
    fn a_status_change_keeps_siblings_and_sends_the_version() {
        let body = status_patch(&b64(&settings("online")), "idle").unwrap();
        assert_eq!(body["required_data_version"], 42);
        let partial = base64::engine::general_purpose::STANDARD.decode(body["settings"].as_str().unwrap()).unwrap();
        // Only the status settings go, with the sibling kept as it was.
        assert!(len_field(&partial, 99).is_none() && len_field(&partial, VERSIONS_FIELD).is_none());
        assert_eq!(len_field(len_field(&partial, STATUS_FIELD).unwrap(), 3), Some(&[8u8, 1][..]));
        assert_eq!(status_of_proto(&b64(&partial), "online").unwrap(), json!({ "status": "idle" }));
        assert!(status_patch("@@", "online").is_err());
        assert_eq!(presence_frame("dnd"), json!({ "op": 3, "d": { "status": "dnd", "since": 0, "activities": [], "afk": false } }));
        assert!(username_ok("some.user_1") && !username_ok("Some") && !username_ok("a") && !username_ok("x/../y"));
    }

    #[test]
    fn profiles_are_cut_down() {
        let body = json!({
            "user": { "id": "5", "username": "five", "global_name": "Five", "avatar": null, "banner": "b1", "accent_color": 255, "email": "x@y" },
            "user_profile": { "bio": "hello", "pronouns": "", "accent_color": 16711680 },
            "guild_member_profile": { "pronouns": "they/them" },
            "guild_member": { "nick": "Fiver", "roles": ["9"], "joined_at": "2024-01-01T00:00:00+00:00" },
            "mutual_guilds": [{ "id": "7", "nick": null }],
            "mutual_friends_count": 3,
        });
        let v = profile_view(&body);
        assert_eq!(v["user"]["username"], "five");
        assert!(v["user"].get("email").is_none());
        assert_eq!((v["banner"].clone(), v["accent"].clone(), v["pronouns"].clone(), v["bio"].clone()), (json!("b1"), json!(16711680), json!("they/them"), json!("hello")));
        assert_eq!(v["member"]["nick"], "Fiver");
        assert_eq!(v["mutual_guilds"][0]["id"], "7");
        assert_eq!(v["mutual_friends"], 3);
    }
}
