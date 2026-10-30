//! Servers: what this account can see and do in each, from READY and the gateway. Permissions
//! follow Discord's documented calculation (the @everyone role and one's own roles, then the
//! channel's @everyone, role and member overwrites); a channel this account can't view is never
//! shown, and the UI is told what it may do in the rest.
//!
//! Threads (and forum posts, which are threads) are kept apart from the channels: the active ones
//! READY and the gateway tell of, and any this client fetched since. A thread's permissions are its
//! parent's, with Send Messages in Threads in place of Send Messages; a private one only for its
//! members (or Manage Threads). Thread state as Concord's `discord/thread` keeps it (GPL-3.0, see
//! THIRD_PARTY_NOTICES.md).

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};

pub const ADMINISTRATOR: u64 = 1 << 3;
pub const ADD_REACTIONS: u64 = 1 << 6;
pub const VIEW_CHANNEL: u64 = 1 << 10;
pub const SEND_MESSAGES: u64 = 1 << 11;
pub const MANAGE_MESSAGES: u64 = 1 << 13;
pub const ATTACH_FILES: u64 = 1 << 15;
pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;
pub const MANAGE_THREADS: u64 = 1 << 34;
pub const SEND_MESSAGES_IN_THREADS: u64 = 1 << 38;
pub const PIN_MESSAGES: u64 = 1 << 51;

// Channel types listed: 0 text, 2 voice and 13 stage (shown, not joinable), 4 category,
// 5 announcement, 15 forum and 16 media (their posts are threads).
const LISTED: [u64; 7] = [0, 2, 4, 5, 13, 15, 16];
// Thread types: 10 an announcement thread, 11 public, 12 private.
pub const THREAD_TYPES: [u64; 3] = [10, 11, 12];
const MAX_CHANNELS: usize = 1000;
const MAX_ROLES: usize = 1000;
const MAX_THREADS: usize = 2000;

pub fn is_thread(c: &Value) -> bool {
    c["type"].as_u64().is_some_and(|t| THREAD_TYPES.contains(&t))
}

pub struct Guild {
    pub id: String,
    pub name: String,
    pub icon: Value,
    pub owner_id: String,
    pub roles: Vec<Value>,
    pub channels: Vec<Value>,
    pub my_roles: Vec<String>,
    pub unavailable: bool,
    // Threads known here (active ones, and any fetched since), and the ones this account is in.
    pub threads: Vec<Value>,
    pub joined: HashSet<String>,
}

// Newer READYs put a guild's own fields under `properties`.
fn field<'a>(g: &'a Value, name: &str) -> &'a Value {
    if g[name].is_null() { &g["properties"][name] } else { &g[name] }
}

fn bits(v: &Value) -> u64 {
    // Discord sends permissions as decimal strings; bits past 63 aren't used here.
    v.as_str().and_then(|s| s.parse::<u128>().ok()).map(|b| b as u64).or(v.as_u64()).unwrap_or(0)
}

fn roles_of(member: &Value) -> Vec<String> {
    member["roles"].as_array().map(|r| r.iter().filter_map(|id| id.as_str().map(str::to_owned)).collect()).unwrap_or_default()
}

impl Guild {
    /// From READY or GUILD_CREATE. `own` is this account's member record when it came apart from
    /// the guild (READY's merged_members); otherwise it's looked for in the guild's members.
    pub fn parse(g: &Value, me: &str, own: Option<&Value>) -> Option<Guild> {
        let id = g["id"].as_str()?.to_owned();
        if g["unavailable"].as_bool() == Some(true) {
            return Some(Guild { id, name: String::new(), icon: Value::Null, owner_id: String::new(), roles: Vec::new(), channels: Vec::new(), my_roles: Vec::new(), unavailable: true, threads: Vec::new(), joined: HashSet::new() });
        }
        let member = own.cloned().or_else(|| g["members"].as_array()?.iter().find(|m| m["user"]["id"] == me || m["user_id"] == me).cloned());
        let threads: Vec<Value> = g["threads"].as_array().map(|t| t.iter().filter(|t| is_thread(t)).take(MAX_THREADS).cloned().collect()).unwrap_or_default();
        // A thread READY lists with a `member` is one this account is in.
        let joined = threads.iter().filter(|t| t["member"].is_object()).filter_map(|t| t["id"].as_str().map(str::to_owned)).collect();
        Some(Guild {
            id,
            name: field(g, "name").as_str().unwrap_or("Server").chars().take(100).collect(),
            icon: field(g, "icon").clone(),
            owner_id: field(g, "owner_id").as_str().unwrap_or("").to_owned(),
            roles: g["roles"].as_array().map(|r| r.iter().take(MAX_ROLES).cloned().collect()).unwrap_or_default(),
            channels: g["channels"].as_array().map(|c| c.iter().take(MAX_CHANNELS).cloned().collect()).unwrap_or_default(),
            my_roles: member.as_ref().map(roles_of).unwrap_or_default(),
            unavailable: false,
            threads,
            joined,
        })
    }

    pub fn set_member(&mut self, member: &Value) {
        self.my_roles = roles_of(member);
    }

    fn role_bits(&self, id: &str) -> u64 {
        self.roles.iter().find(|r| r["id"] == id).map(|r| bits(&r["permissions"])).unwrap_or(0)
    }

    /// The @everyone role's permissions and one's own roles', before any channel.
    fn base(&self, me: &str) -> u64 {
        if self.owner_id == me {
            return u64::MAX;
        }
        let mut b = self.role_bits(&self.id);
        for role in &self.my_roles {
            b |= self.role_bits(role);
        }
        if b & ADMINISTRATOR != 0 { u64::MAX } else { b }
    }

    pub fn permissions(&self, me: &str, channel: &Value) -> u64 {
        if is_thread(channel) {
            return self.thread_permissions(me, channel);
        }
        let base = self.base(me);
        if base == u64::MAX {
            return base;
        }
        let overwrites = channel["permission_overwrites"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        // Overwrite type: 0 (or "role") a role, 1 (or "member") a member.
        let is_member = |o: &Value| o["type"] == 1 || o["type"] == "member";
        let mut b = base;
        if let Some(o) = overwrites.iter().find(|o| !is_member(o) && o["id"] == self.id.as_str()) {
            b = (b & !bits(&o["deny"])) | bits(&o["allow"]);
        }
        let (mut allow, mut deny) = (0, 0);
        for o in overwrites.iter().filter(|o| !is_member(o) && o["id"].as_str().is_some_and(|id| self.my_roles.iter().any(|r| r == id))) {
            allow |= bits(&o["allow"]);
            deny |= bits(&o["deny"]);
        }
        b = (b & !deny) | allow;
        if let Some(o) = overwrites.iter().find(|o| is_member(o) && o["id"] == me) {
            b = (b & !bits(&o["deny"])) | bits(&o["allow"]);
        }
        b
    }

    // A thread's: its parent's, sending by Send Messages in Threads, and a private thread or a
    // locked one only with membership or Manage Threads.
    fn thread_permissions(&self, me: &str, t: &Value) -> u64 {
        let Some(parent) = t["parent_id"].as_str().and_then(|p| self.channels.iter().find(|c| c["id"] == p)) else { return 0 };
        let mut p = self.permissions(me, parent);
        let manage = p & MANAGE_THREADS != 0;
        let id = t["id"].as_str().unwrap_or("");
        if t["type"] == 12 && !manage && !self.joined.contains(id) {
            return 0;
        }
        let may_send = p & SEND_MESSAGES_IN_THREADS != 0 && (manage || t["thread_metadata"]["locked"].as_bool() != Some(true));
        p &= !SEND_MESSAGES;
        if may_send {
            p |= SEND_MESSAGES;
        }
        p
    }

    /// A channel or a known thread.
    pub fn channel(&self, id: &str) -> Option<&Value> {
        self.channels.iter().find(|c| c["id"] == id).or_else(|| self.threads.iter().find(|t| t["id"] == id))
    }

    /// A thread from the gateway or a fetch, kept (replacing what was known of it). The oldest go
    /// first once there are too many.
    pub fn upsert_thread(&mut self, t: &Value) {
        let Some(id) = t["id"].as_str().filter(|_| is_thread(t)) else { return };
        if t["member"].is_object() {
            self.joined.insert(id.to_owned());
        }
        match self.threads.iter_mut().find(|x| x["id"] == id) {
            Some(slot) => {
                // An update may leave out what didn't change.
                if let (Some(old), Some(new)) = (slot.as_object_mut(), t.as_object()) {
                    for (k, v) in new {
                        old.insert(k.clone(), v.clone());
                    }
                }
            }
            None => {
                if self.threads.len() >= MAX_THREADS {
                    let gone = self.threads.remove(0);
                    if let Some(g) = gone["id"].as_str() {
                        self.joined.remove(g);
                    }
                }
                self.threads.push(t.clone());
            }
        }
    }

    pub fn remove_thread(&mut self, id: &str) {
        self.threads.retain(|t| t["id"] != id);
        self.joined.remove(id);
    }

    /// A new message in a channel or thread: its newest message (and a thread's count).
    pub fn touch(&mut self, channel: &str, message: &Value) {
        if let Some(c) = self.channels.iter_mut().find(|c| c["id"] == channel) {
            c["last_message_id"] = message["id"].clone();
        } else if let Some(t) = self.threads.iter_mut().find(|t| t["id"] == channel) {
            t["last_message_id"] = message["id"].clone();
            t["message_count"] = json!(t["message_count"].as_u64().unwrap_or(0) + 1);
        }
    }

    /// What the UI gets of a thread this account can see.
    pub fn thread_view(&self, me: &str, t: &Value) -> Option<Value> {
        let p = self.permissions(me, t);
        if p & VIEW_CHANNEL == 0 {
            return None;
        }
        let has = |bit: u64| p & bit != 0;
        let parent = t["parent_id"].as_str().and_then(|id| self.channels.iter().find(|c| c["id"] == id));
        let overwrites = parent.and_then(|c| c["permission_overwrites"].as_array()).map(Vec::as_slice).unwrap_or(&[]);
        let meta = &t["thread_metadata"];
        Some(json!({
            "id": t["id"], "guild_id": self.id, "type": t["type"], "name": t["name"], "parent_id": t["parent_id"], "owner_id": t["owner_id"],
            "last_message_id": t["last_message_id"], "message_count": t["message_count"], "member_count": t["member_count"],
            "archived": meta["archived"].as_bool().unwrap_or(false), "locked": meta["locked"].as_bool().unwrap_or(false),
            "created": meta["create_timestamp"], "applied_tags": t["applied_tags"], "pinned": t["flags"].as_u64().unwrap_or(0) & 2 != 0,
            "joined": self.joined.contains(t["id"].as_str().unwrap_or("")), "thread": true,
            "can_send": has(SEND_MESSAGES), "can_history": has(READ_MESSAGE_HISTORY), "can_react": has(ADD_REACTIONS),
            "can_attach": has(ATTACH_FILES), "can_manage": has(MANAGE_MESSAGES), "can_pin": has(MANAGE_MESSAGES) || has(PIN_MESSAGES),
            "member_list": member_list_id(self.role_bits(&self.id), overwrites),
        }))
    }

    /// The known threads under a channel this account can see, newest activity first.
    pub fn threads_of(&self, me: &str, parent: &str) -> Vec<Value> {
        let mut list: Vec<Value> = self.threads.iter().filter(|t| t["parent_id"] == parent).filter_map(|t| self.thread_view(me, t)).collect();
        let last = |v: &Value| v["last_message_id"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
        list.sort_by_key(|v| std::cmp::Reverse(last(v)));
        list
    }

    pub fn can_view(&self, me: &str, channel_id: &str) -> bool {
        self.channel(channel_id).is_some_and(|c| self.permissions(me, c) & VIEW_CHANNEL != 0)
    }

    fn channel_view(&self, me: &str, c: &Value) -> Option<Value> {
        let kind = c["type"].as_u64()?;
        if !LISTED.contains(&kind) {
            return None;
        }
        let p = self.permissions(me, c);
        if p & VIEW_CHANNEL == 0 {
            return None;
        }
        let has = |bit: u64| p & bit != 0;
        let overwrites = c["permission_overwrites"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        Some(json!({
            "id": c["id"], "guild_id": self.id, "type": kind, "name": c["name"], "position": c["position"], "parent_id": c["parent_id"],
            "topic": c["topic"], "last_message_id": c["last_message_id"], "nsfw": c["nsfw"],
            "can_send": has(SEND_MESSAGES), "can_history": has(READ_MESSAGE_HISTORY), "can_react": has(ADD_REACTIONS),
            "can_attach": has(ATTACH_FILES), "can_manage": has(MANAGE_MESSAGES), "can_pin": has(MANAGE_MESSAGES) || has(PIN_MESSAGES),
            "member_list": member_list_id(self.role_bits(&self.id), overwrites),
            "available_tags": c["available_tags"].as_array().map(|tags| tags.iter().take(20).map(|t| json!({ "id": t["id"], "name": t["name"], "emoji": t["emoji_name"] })).collect::<Vec<_>>()),
        }))
    }

    /// What the UI gets: the server, its roles (for names' colours and mentions), the channels
    /// this account can see, and the active threads it's in (listed under their channels).
    pub fn view(&self, me: &str) -> Value {
        if self.unavailable {
            return json!({ "id": self.id, "unavailable": true });
        }
        let roles: Vec<Value> = self.roles.iter().map(|r| json!({ "id": r["id"], "name": r["name"], "color": r["color"], "position": r["position"] })).collect();
        let channels: Vec<Value> = self.channels.iter().filter_map(|c| self.channel_view(me, c)).collect();
        let threads: Vec<Value> = self.threads.iter()
            .filter(|t| self.joined.contains(t["id"].as_str().unwrap_or("")) && t["thread_metadata"]["archived"].as_bool() != Some(true))
            .filter_map(|t| self.thread_view(me, t)).collect();
        json!({ "id": self.id, "name": self.name, "icon": self.icon, "owner_id": self.owner_id, "roles": roles, "my_roles": self.my_roles, "channels": channels, "threads": threads })
    }

    /// Whether a message here mentions this account: by name, by one of its roles, or @everyone.
    pub fn mentions_me(&self, me: &str, message: &Value) -> bool {
        let named = message["mentions"].as_array().is_some_and(|m| m.iter().any(|u| u["id"] == me));
        let role = message["mention_roles"].as_array().is_some_and(|r| r.iter().any(|id| id.as_str().is_some_and(|id| self.my_roles.iter().any(|m| m == id))));
        named || role || message["mention_everyone"].as_bool() == Some(true)
    }
}

/// The servers in the order the user keeps them (folders, then positions), then any others.
pub fn order(data: &Value) -> Vec<String> {
    let settings = &data["user_settings"];
    let folders = settings["guild_folders"].as_array().into_iter().flatten().flat_map(|f| f["guild_ids"].as_array().into_iter().flatten());
    let positions = settings["guild_positions"].as_array().into_iter().flatten();
    let mut out: Vec<String> = Vec::new();
    for id in folders.chain(positions).filter_map(|v| v.as_str().map(str::to_owned).or_else(|| v.as_u64().map(|n| n.to_string()))) {
        if !out.contains(&id) {
            out.push(id);
        }
    }
    out
}

/// Muted servers and channels, from READY's user_guild_settings (a list, or { entries }).
pub fn muted(data: &Value) -> (Vec<String>, Vec<String>) {
    let entries = data["user_guild_settings"].as_array().or(data["user_guild_settings"]["entries"].as_array());
    let (mut guilds, mut channels) = (Vec::new(), Vec::new());
    for e in entries.into_iter().flatten() {
        if e["muted"].as_bool() == Some(true) {
            if let Some(id) = e["guild_id"].as_str() {
                guilds.push(id.to_owned());
            }
        }
        for o in e["channel_overrides"].as_array().into_iter().flatten() {
            if o["muted"].as_bool() == Some(true) {
                if let Some(id) = o["channel_id"].as_str() {
                    channels.push(id.to_owned());
                }
            }
        }
    }
    (guilds, channels)
}

/// READY's merged_members: this account's member record per guild, in the guilds' order.
pub fn merged_own(data: &Value, me: &str) -> HashMap<String, Value> {
    let guilds = data["guilds"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    let members = data["merged_members"].as_array().map(Vec::as_slice).unwrap_or(&[]);
    guilds
        .iter()
        .zip(members)
        .filter_map(|(g, list)| {
            let own = list.as_array()?.iter().find(|m| m["user_id"] == me || m["user"]["id"] == me)?;
            Some((g["id"].as_str()?.to_owned(), own.clone()))
        })
        .collect()
}

/// The op 37 the web client sends when a channel is opened: typing and activity for the server,
/// and the first 100 of the channel's member list.
pub fn subscription(guild_id: &str, channel_id: &str) -> Value {
    json!({ "op": 37, "d": { "subscriptions": { guild_id: {
        "typing": true, "threads": true, "activities": true, "member_updates": false, "members": [],
        "channels": { channel_id: [[0, 99]] }, "thread_member_lists": [],
    } } } })
}

// Which member list a channel shows (GUILD_MEMBER_LIST_UPDATE's id): "everyone" when everyone
// can see it, otherwise a hash of who's allowed and denied. Unofficial, as observed by
// discord.py-self; adapted from Serein's `crates/model/src/permissions.rs` (MIT, see
// THIRD_PARTY_NOTICES.md).
pub fn member_list_id(everyone: u64, overwrites: &[Value]) -> String {
    let view = |o: &Value, key: &str| bits(&o[key]) & VIEW_CHANNEL != 0;
    if everyone & VIEW_CHANNEL != 0 && !overwrites.iter().any(|o| view(o, "deny")) {
        return "everyone".into();
    }
    let mut entries: Vec<String> = overwrites
        .iter()
        .filter_map(|o| {
            let id = o["id"].as_str()?;
            if view(o, "allow") {
                Some(format!("allow:{id}"))
            } else if view(o, "deny") {
                Some(format!("deny:{id}"))
            } else {
                None
            }
        })
        .collect();
    entries.sort();
    murmur3(entries.join(",").as_bytes()).to_string()
}

// MurmurHash3 (x86, 32-bit, seed 0), as Discord's client hashes member list ids.
fn murmur3(bytes: &[u8]) -> u32 {
    let mix = |n: u32| n.wrapping_mul(0xcc9e2d51).rotate_left(15).wrapping_mul(0x1b873593);
    let mut hash = 0u32;
    let chunks = bytes.chunks_exact(4);
    let remainder = chunks.remainder();
    for part in chunks {
        hash ^= mix(u32::from_le_bytes([part[0], part[1], part[2], part[3]]));
        hash = hash.rotate_left(13).wrapping_mul(5).wrapping_add(0xe6546b64);
    }
    if !remainder.is_empty() {
        let tail = remainder.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (i * 8)));
        hash ^= mix(tail);
    }
    hash ^= bytes.len() as u32;
    hash ^= hash >> 16;
    hash = hash.wrapping_mul(0x85ebca6b);
    hash ^= hash >> 13;
    hash = hash.wrapping_mul(0xc2b2ae35);
    hash ^ (hash >> 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn guild() -> Guild {
        let g = json!({
            "id": "1", "name": "G", "owner_id": "99",
            "roles": [
                { "id": "1", "permissions": (VIEW_CHANNEL | SEND_MESSAGES | READ_MESSAGE_HISTORY).to_string() },
                { "id": "2", "permissions": MANAGE_MESSAGES.to_string() },
                { "id": "3", "permissions": ADMINISTRATOR.to_string() },
            ],
            "channels": [
                { "id": "10", "type": 0, "name": "general", "permission_overwrites": [] },
                { "id": "11", "type": 0, "name": "staff", "permission_overwrites": [
                    { "id": "1", "type": 0, "allow": "0", "deny": VIEW_CHANNEL.to_string() },
                    { "id": "2", "type": 0, "allow": VIEW_CHANNEL.to_string(), "deny": "0" },
                ] },
                { "id": "12", "type": 0, "name": "read-only", "permission_overwrites": [
                    { "id": "1", "type": "role", "allow": "0", "deny": SEND_MESSAGES.to_string() },
                    { "id": "5", "type": "member", "allow": SEND_MESSAGES.to_string(), "deny": "0" },
                ] },
                { "id": "13", "type": 11, "name": "a thread" },
            ],
            "members": [{ "user": { "id": "5" }, "roles": [] }],
        });
        Guild::parse(&g, "5", None).unwrap()
    }

    #[test]
    fn overwrites_apply_in_discords_order() {
        let mut g = guild();
        let general = g.channel("10").unwrap().clone();
        let staff = g.channel("11").unwrap().clone();
        let read_only = g.channel("12").unwrap().clone();
        assert!(g.permissions("5", &general) & SEND_MESSAGES != 0);
        assert_eq!(g.permissions("5", &staff) & VIEW_CHANNEL, 0, "@everyone denied, no allowing role");
        // The member overwrite beats the @everyone deny.
        assert!(g.permissions("5", &read_only) & SEND_MESSAGES != 0);
        assert_eq!(g.permissions("6", &read_only) & SEND_MESSAGES, 0);

        g.my_roles = vec!["2".into()];
        assert!(g.permissions("5", &staff) & VIEW_CHANNEL != 0, "a role allow beats the @everyone deny");
        g.my_roles = vec!["3".into()];
        assert_eq!(g.permissions("5", &staff), u64::MAX, "administrator sees everything");
        assert_eq!(g.permissions("99", &staff), u64::MAX, "so does the owner");
    }

    #[test]
    fn the_view_lists_only_what_can_be_seen() {
        let view = guild().view("5");
        let ids: Vec<&str> = view["channels"].as_array().unwrap().iter().map(|c| c["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["10", "12"], "staff is hidden and threads aren't listed");
        assert_eq!(view["channels"][0]["member_list"], "everyone");
        assert_eq!(view["channels"][1]["can_send"], true);
    }

    #[test]
    fn threads_take_their_parents_permissions() {
        let mut g = guild();
        let send_in_threads = (VIEW_CHANNEL | SEND_MESSAGES | READ_MESSAGE_HISTORY | SEND_MESSAGES_IN_THREADS).to_string();
        g.roles[0]["permissions"] = json!(send_in_threads);
        let public = json!({ "id": "20", "type": 11, "parent_id": "10", "name": "t", "thread_metadata": { "archived": false } });
        let private = json!({ "id": "21", "type": 12, "parent_id": "10", "name": "p" });
        let locked = json!({ "id": "22", "type": 11, "parent_id": "10", "thread_metadata": { "locked": true } });
        let hidden = json!({ "id": "23", "type": 11, "parent_id": "11" });
        for t in [&public, &private, &locked, &hidden] {
            g.upsert_thread(t);
        }
        assert!(g.can_view("5", "20") && g.permissions("5", g.channel("20").unwrap()) & SEND_MESSAGES != 0);
        assert!(!g.can_view("5", "21"), "private, not a member");
        g.joined.insert("21".into());
        assert!(g.can_view("5", "21"));
        assert_eq!(g.permissions("5", g.channel("22").unwrap()) & SEND_MESSAGES, 0, "locked");
        assert!(!g.can_view("5", "23"), "its parent is hidden");
        // Without Send Messages in Threads, Send Messages alone doesn't let it send.
        g.roles[0]["permissions"] = json!((VIEW_CHANNEL | SEND_MESSAGES).to_string());
        assert_eq!(g.permissions("5", g.channel("20").unwrap()) & SEND_MESSAGES, 0);
        assert_eq!(g.thread_view("5", g.channel("20").unwrap()).unwrap()["can_send"], false);
        // The view lists only active threads this account is in.
        g.roles[0]["permissions"] = json!(send_in_threads);
        let ids: Vec<String> = g.view("5")["threads"].as_array().unwrap().iter().map(|t| t["id"].as_str().unwrap().to_owned()).collect();
        assert_eq!(ids, ["21"]);
        g.remove_thread("21");
        assert!(g.channel("21").is_none() && !g.joined.contains("21"));
        // An update keeps what it leaves out; a message counts.
        g.upsert_thread(&json!({ "id": "20", "type": 11, "name": "renamed" }));
        assert_eq!(g.channel("20").unwrap()["parent_id"], "10");
        g.touch("20", &json!({ "id": "500" }));
        assert_eq!(g.channel("20").unwrap()["message_count"], 1);
        assert_eq!(g.threads_of("5", "10")[0]["id"], "20");
    }

    #[test]
    fn member_list_ids_match_discords_hash() {
        assert_eq!(murmur3(b""), 0);
        assert_eq!(murmur3(b"hello"), 0x248bfa47);
        let deny = json!({ "id": "5", "allow": "0", "deny": VIEW_CHANNEL.to_string() });
        let allow = json!({ "id": "9", "allow": (VIEW_CHANNEL | (1 << 40)).to_string(), "deny": "0" });
        assert_eq!(member_list_id(VIEW_CHANNEL, &[]), "everyone");
        // Serein's own test vector for the same pair.
        assert_eq!(member_list_id(VIEW_CHANNEL, &[deny, allow]), murmur3(b"allow:9,deny:5").to_string());
    }

    #[test]
    fn ready_order_mutes_and_merged_members() {
        let data = json!({
            "user_settings": { "guild_folders": [{ "guild_ids": ["3", "1"] }], "guild_positions": ["2", "1"] },
            "user_guild_settings": { "entries": [{ "guild_id": "1", "muted": true, "channel_overrides": [{ "channel_id": "10", "muted": true }] }] },
            "guilds": [{ "id": "1" }, { "id": "2" }],
            "merged_members": [[{ "user_id": "5", "roles": ["7"] }], [{ "user_id": "6", "roles": [] }]],
        });
        assert_eq!(order(&data), ["3", "1", "2"]);
        assert_eq!(muted(&data), (vec!["1".to_owned()], vec!["10".to_owned()]));
        let own = merged_own(&data, "5");
        assert_eq!(own["1"]["roles"][0], "7");
        assert!(!own.contains_key("2"));
    }

    #[test]
    fn mentions_by_name_role_or_everyone() {
        let mut g = guild();
        g.my_roles = vec!["7".into()];
        assert!(g.mentions_me("5", &json!({ "mentions": [{ "id": "5" }] })));
        assert!(g.mentions_me("5", &json!({ "mention_roles": ["7"] })));
        assert!(g.mentions_me("5", &json!({ "mention_everyone": true })));
        assert!(!g.mentions_me("5", &json!({ "mentions": [{ "id": "6" }], "mention_roles": ["8"] })));
    }
}
