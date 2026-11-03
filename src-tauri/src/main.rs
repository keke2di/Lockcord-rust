// LockCord: a Discord client of its own, with secret chats built in. No Discord code runs in it:
// the window is LockCord's own UI (ui/), and Rust talks to Discord's API and gateway.
//
//   --profile=<name>   a separate account (its own saved login), so two can run side by side
//   --safe-mode        start with the default theme, for a theme that broke the window

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod auth;
mod call;
mod captcha;
mod clean;
mod deletion;
mod desktop;
mod e2e;
mod fingerprint;
mod gateway;
mod gifs;
mod guard;
mod guilds;
mod media;
mod netlog;
mod people;
mod privacy;
mod rest;
mod search;
mod settings;
mod translate;
mod update;
mod vault;

use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_opener::OpenerExt;
use tokio::sync::{mpsc, watch};

const MAIN: &str = "main";
const MAX_CONTENT: usize = 4000;
const MAX_OWN: usize = 50_000;

struct Live {
    rest: Arc<rest::Rest>,
    stop: watch::Sender<bool>,
    me: Value,
    dms: Vec<Value>,
    gateway: &'static str,
    saved: bool,
    // Ids of messages this account wrote, seen in history or on the gateway: the only ones it will
    // edit or delete.
    own: HashSet<String>,
    upload_limit: usize,
    // Files this client uploaded to Discord's bucket and hasn't sent yet: the only ones a message
    // may name.
    uploads: HashSet<String>,
    // Friends, requests and blocks, as rel_view gives them.
    relationships: Vec<Value>,
    // User id -> online, idle, dnd or offline, for friends.
    presences: HashMap<String, String>,
    // User id -> custom status ({ text, emoji }), for friends who have one.
    customs: HashMap<String, Value>,
    // Channel id -> { last: the last message read, mentions: unread mentions }.
    reads: HashMap<String, Value>,
    guilds: Vec<guilds::Guild>,
    // Server ids in the order the user keeps them.
    guild_order: Vec<String>,
    // Muted servers and channels: no unread marks.
    muted: (Vec<String>, Vec<String>),
    // Frames for the gateway to send (op 37), and the server and channel subscribed to.
    outbound: mpsc::Sender<Value>,
    subscribed: Option<(String, String, Vec<[u32; 2]>)>,
    // Every delete goes through this one queue (deletion.rs).
    queue: deletion::Queue,
    guard: Vec<guard::Rule>,
    // Channels Guard is wiping right now.
    wiping: HashSet<String>,
    purge: Option<Purge>,
    e2e: E2eState,
    // This account's status (people::status_view).
    my_status: Value,
    // People whose profile showed a server in common: a DM may be opened with them too.
    dm_ok: HashSet<String>,
    // Who is in which voice channel or DM call, and the call this device is in (call.rs).
    calls: call::Calls,
}

// Encrypted DMs (e2e.rs): the identity (if made), the contacts with their keys, the peers they're
// on with (also in Credential Manager), and why it can't work, if it can't.
#[derive(Default)]
struct E2eState {
    identity: Option<e2e::Identity>,
    book: e2e::Book,
    pinned: HashSet<String>,
    // Peers this side is posting an answer to right now (one at a time).
    answering: HashSet<String>,
    problem: Option<String>,
}

// A purge: what its scan found (channel, message), waiting to be confirmed, and its stop switch.
struct Purge {
    cancel: Arc<AtomicBool>,
    found: Vec<(String, String)>,
}

impl Live {
    /// The other person in a 1:1 DM.
    fn dm_peer(&self, channel: &str) -> Option<String> {
        self.dms.iter().find(|d| d["id"] == channel && d["type"] == 1).and_then(|d| d["recipients"][0]["id"].as_str()).map(str::to_owned)
    }

    /// Whether this channel is an encrypted DM: nothing may go out there in plain text unless asked.
    fn encrypted_peer(&self, channel: &str) -> Option<String> {
        let peer = self.dm_peer(channel)?;
        let status = self.e2e.identity.as_ref().map(|id| self.e2e.book.status(id, &peer)).unwrap_or(e2e::Status::Off);
        let on = matches!(status, e2e::Status::On | e2e::Status::Changed);
        // Pinned in Credential Manager but nothing known here: blocked, never plain.
        let lost = self.e2e.pinned.contains(&peer) && !matches!(status, e2e::Status::On | e2e::Status::Changed | e2e::Status::Waiting);
        (on || lost).then_some(peer)
    }

    /// A LockCord key message in a 1:1 DM, yours or theirs: recorded, and shown as a line rather
    /// than its text. Returns (peer, what it did, whether this side should answer now).
    fn e2e_key(&mut self, profile: &str, m: &mut Value) -> Option<(String, e2e::Event, bool)> {
        let (role, key, nonce) = m["content"].as_str().and_then(e2e::parse_key_message)?;
        let channel = m["channel_id"].as_str().unwrap_or("").to_owned();
        let author = m["author"]["id"].as_str().unwrap_or("").to_owned();
        let me = self.me_id();
        m["content"] = json!("");
        let peer = self.dm_peer(&channel).filter(|p| author == me || &author == p);
        let Some(peer) = peer else {
            m["lc_key"] = json!("ignored");
            return None;
        };
        let from_me = author == me;
        m["lc_key"] = json!(if from_me { "mine" } else { "theirs" });
        m["lc_key_role"] = json!(if role == e2e::Role::Offer { "offer" } else { "answer" });
        // Their key needs an identity here to answer and derive with: made the first time.
        if self.e2e.identity.is_none() {
            self.e2e.identity = e2e::identity(profile).ok();
        }
        let identity = self.e2e.identity.as_ref()?;
        let id = m["id"].as_str().unwrap_or("").to_owned();
        let ann = e2e::Announcement { id: id.clone(), key, nonce, role, at: deletion::time_of(&id) };
        let event = self.e2e.book.record(identity, &me, &peer, ann, from_me);
        if event == e2e::Event::Changed {
            m["lc_key_changed"] = json!(true);
        }
        // Pinned once this side chose encryption with them (a stranger's offer pins nothing).
        if self.e2e.book.contacts.get(&peer).is_some_and(|c| c.chosen(identity.public.as_bytes())) {
            self.e2e.pinned.insert(peer.clone());
        }
        let answer = self.e2e.book.contacts.get(&peer).is_some_and(|c| c.needs_answer(identity.public.as_bytes())) && !self.e2e.answering.contains(&peer);
        Some((peer, event, answer))
    }

    /// A message (and its reply preview) with LC1 content, opened: the text in `content`, marked
    /// `lc_e2e`; or emptied and marked `lc_e2e_locked` (no key) / `lc_e2e_failed` (doesn't check).
    fn e2e_open(&mut self, m: &mut Value) {
        if let Some(r) = m.get_mut("referenced_message").filter(|r| r.is_object()) {
            self.e2e_open_one(r);
        }
        // A forward carries a copy that belongs to another channel: never readable, never raw.
        for snap in m["message_snapshots"].as_array_mut().into_iter().flatten() {
            let inner = &mut snap["message"];
            if inner["content"].as_str().is_some_and(e2e::is_lc1) {
                inner["content"] = json!("");
                inner["lc_e2e_locked"] = json!(true);
            }
        }
        self.e2e_open_one(m);
    }

    fn e2e_open_one(&mut self, m: &mut Value) {
        let Some(content) = m["content"].as_str().filter(|c| e2e::is_lc1(c)).map(str::to_owned) else { return };
        let channel = m["channel_id"].as_str().unwrap_or("").to_owned();
        let sender = m["author"]["id"].as_str().unwrap_or("").to_owned();
        let id = m["id"].as_str().unwrap_or("").to_owned();
        let posted = deletion::time_of(&id);
        let me = self.me_id();
        m["content"] = json!("");
        let peer = self.dm_peer(&channel).filter(|peer| sender == me || &sender == peer);
        let opened = match &peer {
            Some(peer) => self.e2e.book.open(peer, &channel, &sender, &content, posted),
            None => Err(e2e::E2eError::NoKey),
        };
        // The same ciphertext under a second Discord message: shown, but marked as a copy.
        if let (Ok(_), Some(peer)) = (&opened, &peer) {
            if self.e2e.book.replayed(peer, &content, &id) {
                m["lc_e2e_replayed"] = json!(true);
            }
        }
        match opened {
            Ok(e2e::Opened::Text(text)) => {
                m["content"] = json!(text);
                m["lc_e2e"] = json!(true);
            }
            Ok(e2e::Opened::Stale(text)) => {
                m["content"] = json!(text);
                m["lc_e2e"] = json!(true);
                m["lc_e2e_stale"] = json!(true);
            }
            Err(e2e::E2eError::Broken) => m["lc_e2e_failed"] = json!(true),
            Err(e2e::E2eError::Untrusted) => m["lc_e2e_untrusted"] = json!(true),
            Err(_) => m["lc_e2e_locked"] = json!(true),
        }
    }

    /// What the UI may know: the contacts (no keys), their DMs, the problem if any.
    fn e2e_view(&self) -> Value {
        let channel_of = |peer: &str| self.dms.iter().find(|d| d["type"] == 1 && d["recipients"][0]["id"] == peer).map(|d| d["id"].clone()).unwrap_or(Value::Null);
        let mut contacts: Vec<Value> = match &self.e2e.identity {
            Some(identity) => self.e2e.book.contacts.keys().filter_map(|peer| {
                let mut v = self.e2e.book.view(identity, peer)?;
                v["channel"] = channel_of(peer);
                Some(v)
            }).collect(),
            None => Vec::new(),
        };
        // Pinned but unknown here (the file was lost): shown as blocked.
        for peer in self.e2e.pinned.iter().filter(|p| !self.e2e.book.contacts.contains_key(*p)) {
            contacts.push(json!({ "peer": peer, "status": "lost", "channel": channel_of(peer) }));
        }
        let notice = self.e2e.book.code_upgraded.then_some("Safety codes are longer now (60 digits), so earlier comparisons were cleared. Compare the new code once more with each contact.");
        json!({ "contacts": contacts, "problem": self.e2e.problem, "notice": notice })
    }

    fn me_id(&self) -> String {
        self.me["id"].as_str().unwrap_or("").to_owned()
    }

    /// The server a channel this account can see belongs to.
    fn guild_with(&self, channel_id: &str) -> Option<usize> {
        let me = self.me_id();
        self.guilds.iter().position(|g| g.can_view(&me, channel_id))
    }

    fn guild_view(&self, index: usize) -> Value {
        self.guilds[index].view(&self.me_id())
    }

    fn guild_views(&self) -> Vec<Value> {
        let me = self.me_id();
        let rank = |id: &str| self.guild_order.iter().position(|o| o == id).unwrap_or(usize::MAX);
        let mut list: Vec<&guilds::Guild> = self.guilds.iter().collect();
        list.sort_by_key(|g| rank(&g.id));
        list.iter().map(|g| g.view(&me)).collect()
    }

    fn resubscribe(&self) {
        if let Some((guild, channel, ranges)) = &self.subscribed {
            let _ = self.outbound.try_send(guilds::subscription(guild, channel, ranges));
        }
    }

    fn note_own(&mut self, message: &Value) {
        if message["author"]["id"] != self.me["id"] {
            return;
        }
        if let Some(id) = message["id"].as_str() {
            // Bounded: a message older than what's loaded is seen again when its history loads.
            if self.own.len() >= MAX_OWN {
                self.own.clear();
            }
            self.own.insert(id.to_owned());
        }
    }
}

struct App {
    profile: String,
    safe_mode: bool,
    http: reqwest::Client,
    live: Mutex<Option<Live>>,
    media: Mutex<media::Cache>,
    nonces: AtomicU64,
    // The captcha window's page and its one-at-a-time lock (captcha.rs).
    captcha: Arc<captcha::Showing>,
    // For Discord's media: redirects only onto Discord's own hosts (media.rs).
    media_http: reqwest::Client,
    // The channel the window shows (notifications skip it while the window has focus).
    viewing: Mutex<Option<String>>,
    // From the settings: close to the tray, and how much a notification says.
    close_to_tray: AtomicBool,
    notify: Mutex<String>,
    // Calls: the Voice & Video settings, decoded video waiting for the window, the microphone test.
    voice: Mutex<call::VoiceSettings>,
    frames: Arc<call::Frames>,
    mic_test: Mutex<Option<discord_voice::audio::Audio>>,
    // The Privacy page (privacy.rs): its settings, the app lock (lock.rs), and the capture shield:
    // whether the window says a secret chat is on screen, and whether the shield is up.
    privacy: Mutex<privacy::Privacy>,
    secret_on_screen: AtomicBool,
    shield_up: Mutex<Option<bool>>,
}

fn profile_arg() -> String {
    std::env::args()
        .find_map(|a| a.strip_prefix("--profile=").map(str::to_owned))
        .filter(|p| !p.is_empty() && p.len() <= 32 && p.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
        .unwrap_or_else(|| "default".to_owned())
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

fn emit(app: &AppHandle, event: &str, payload: Value) {
    let _ = app.emit_to(MAIN, event, payload);
}

// --- Discord's records, cut down to what the UI uses ---------------------------------------------

fn user_view(u: &Value) -> Value {
    json!({
        "id": u["id"], "username": u["username"], "global_name": u["global_name"],
        "avatar": u["avatar"], "discriminator": u["discriminator"], "bot": u["bot"].as_bool().unwrap_or(false),
    })
}

fn dm_view(channel: &Value, users: &HashMap<String, Value>) -> Option<Value> {
    let kind = channel["type"].as_u64()?;
    if kind != 1 && kind != 3 {
        return None;
    }
    let recipients: Vec<Value> = match channel["recipients"].as_array() {
        Some(list) => list.iter().map(user_view).collect(),
        None => channel["recipient_ids"].as_array().map(|ids| ids.iter().filter_map(|id| users.get(id.as_str()?)).map(user_view).collect()).unwrap_or_default(),
    };
    Some(json!({
        "id": channel["id"], "type": kind, "name": channel["name"], "icon": channel["icon"], "owner_id": channel["owner_id"],
        "recipients": recipients, "last_message_id": channel["last_message_id"],
    }))
}

// Discord's relationship types: 1 friend, 2 blocked, 3 incoming request, 4 outgoing request.
fn rel_view(r: &Value, users: &HashMap<String, Value>) -> Option<Value> {
    let id = r["id"].as_str().filter(|id| is_id(id))?;
    let user = if r["user"].is_object() { &r["user"] } else { users.get(id)? };
    Some(json!({ "id": id, "type": r["type"], "nickname": r["nickname"], "user": user_view(user) }))
}

// READY's friends' presences, old shape (`presences`, user.id) or new (`merged_presences.friends`,
// user_id).
fn presences_of(data: &Value) -> HashMap<String, String> {
    let old = data["presences"].as_array().into_iter().flatten();
    let new = data["merged_presences"]["friends"].as_array().into_iter().flatten();
    old.chain(new).filter_map(presence).collect()
}

// READY's friends' custom statuses, from the same presences.
fn customs_of(data: &Value) -> HashMap<String, Value> {
    let old = data["presences"].as_array().into_iter().flatten();
    let new = data["merged_presences"]["friends"].as_array().into_iter().flatten();
    old.chain(new).filter_map(|p| {
        let id = p["user"]["id"].as_str().or(p["user_id"].as_str())?;
        let custom = people::custom_status(p);
        (!custom.is_null()).then(|| (id.to_owned(), custom))
    }).collect()
}

fn presence(p: &Value) -> Option<(String, String)> {
    let id = p["user"]["id"].as_str().or(p["user_id"].as_str())?;
    let status = p["status"].as_str().filter(|s| ["online", "idle", "dnd", "offline"].contains(s)).unwrap_or("offline");
    Some((id.to_owned(), status.to_owned()))
}

// READY's read state: a list, or { entries: [...] } in newer clients.
fn reads_of(data: &Value) -> HashMap<String, Value> {
    let entries = data["read_state"].as_array().or(data["read_state"]["entries"].as_array());
    entries
        .into_iter()
        .flatten()
        .filter_map(|e| Some((e["id"].as_str()?.to_owned(), json!({ "last": e["last_message_id"], "mentions": e["mention_count"].as_u64().unwrap_or(0) }))))
        .collect()
}

fn newer(a: &str, b: &str) -> bool {
    a.parse::<u64>().unwrap_or(0) > b.parse::<u64>().unwrap_or(0)
}

fn ready_view(live: &Live) -> Value {
    json!({
        "me": live.me, "dms": live.dms, "relationships": live.relationships, "presences": live.presences, "customs": live.customs, "reads": live.reads,
        "guilds": live.guild_views(), "muted": { "guilds": live.muted.0, "channels": live.muted.1 },
        "e2e": live.e2e_view(), "my_status": live.my_status, "voice": live.calls.view(),
        // Nitro (from the upload limit it allows): other servers' emoji may then be used anywhere.
        "nitro": live.upload_limit > media::upload_limit(None),
    })
}

fn sort_dms(dms: &mut [Value]) {
    let key = |d: &Value| d["last_message_id"].as_str().and_then(|s| s.parse::<u64>().ok()).unwrap_or(0);
    dms.sort_by_key(|d| std::cmp::Reverse(key(d)));
}

// A reaction's emoji as Discord's path wants it: a Unicode emoji, or a custom one as `name:id`.
// Anything else is refused, so nothing but an emoji ends up in the path.
fn emoji_path(emoji: &str) -> Option<String> {
    if emoji.is_empty() || emoji.len() > 64 {
        return None;
    }
    let ok = match emoji.split_once(':') {
        Some((name, id)) => (2..=32).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') && is_id(id),
        // Keycaps (1️⃣, #️⃣, *️⃣) are the only emoji with ASCII in them.
        None => !emoji.is_ascii() && emoji.bytes().all(|b| !b.is_ascii() || b.is_ascii_digit() || b == b'#' || b == b'*'),
    };
    ok.then(|| emoji.bytes().map(|b| if b.is_ascii_alphanumeric() { (b as char).to_string() } else { format!("%{b:02X}") }).collect())
}

// --- The session ---------------------------------------------------------------------------------

async fn start_session(app: AppHandle, token: String, save: bool) -> Result<Value, String> {
    let state = app.state::<App>();
    stop_session(&state);
    // A captcha Discord asks for is shown in its own window, then the request goes again.
    let solver_app = app.clone();
    let solver: rest::Solver = Arc::new(move |challenge| {
        let app = solver_app.clone();
        Box::pin(async move {
            let state = app.state::<App>();
            let (showing, dir) = (state.captcha.clone(), webview_dir(&app, &state.profile));
            captcha::solve(app.clone(), showing, dir, fingerprint::user_agent(), challenge).await
        })
    });
    let rest = Arc::new(rest::Rest::new(state.http.clone(), token.clone()).with_solver(solver));
    let me = match rest.get("/users/@me").await {
        Ok(me) => me,
        Err(rest::RestError::Unauthorized) => {
            auth::forget(&state.profile);
            return Err("Discord didn't accept that login.".into());
        }
        Err(e) => return Err(format!("Couldn't reach Discord ({e}).")),
    };
    if me["bot"].as_bool() == Some(true) {
        return Err("That's a bot account. LockCord signs in as a person.".into());
    }
    let (stop, stopped) = watch::channel(false);
    let (tx, mut rx) = mpsc::channel(512);
    let (outbound, outbound_rx) = mpsc::channel(32);
    let upload_limit = media::upload_limit(me["premium_type"].as_u64());
    let queue = deletion::Queue::start(rest.clone());
    let rules = store(&app, &state).map(|s| guard::load(&s.read_json("guard.json"))).unwrap_or_default();
    let guard_stop = stopped.clone();
    let e2e_state = load_e2e(&app, &state, me["id"].as_str().unwrap_or(""));
    *state.live.lock().unwrap() = Some(Live {
        rest, stop, me: user_view(&me), dms: Vec::new(), gateway: "connecting", saved: !save, own: HashSet::new(), upload_limit, uploads: HashSet::new(),
        relationships: Vec::new(), presences: HashMap::new(), customs: HashMap::new(), reads: HashMap::new(),
        guilds: Vec::new(), guild_order: Vec::new(), muted: (Vec::new(), Vec::new()), outbound, subscribed: None,
        queue, guard: rules, wiping: HashSet::new(), purge: None, e2e: e2e_state, my_status: people::status_view("online"), dm_ok: HashSet::new(),
        calls: call::Calls::default(),
    });
    emit(&app, "lc://session", json!({ "signedIn": true, "me": user_view(&me) }));

    tauri::async_runtime::spawn(gateway::run(token.clone(), tx, outbound_rx, stopped));
    tauri::async_runtime::spawn(guard_loop(app.clone(), guard_stop));
    let pump = app.clone();
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            on_gateway(&pump, &token, event);
        }
    });
    Ok(user_view(&me))
}

fn stop_session(state: &App) {
    if let Some(live) = state.live.lock().unwrap().take() {
        let _ = live.stop.send(true);
    }
}

fn on_gateway(app: &AppHandle, token: &str, event: gateway::Event) {
    let state = app.state::<App>();
    match event {
        gateway::Event::Status(status) => {
            if let Some(live) = state.live.lock().unwrap().as_mut() {
                live.gateway = status;
            }
            emit(app, "lc://gateway", json!({ "status": status }));
        }
        gateway::Event::Unauthorized => {
            auth::forget(&state.profile);
            stop_session(&state);
            emit(app, "lc://session", json!({ "signedIn": false, "reason": "Discord ended the session. Sign in again." }));
        }
        gateway::Event::Dispatch(name, data) => on_dispatch(app, &state, token, &name, data),
    }
}

fn on_dispatch(app: &AppHandle, state: &App, token: &str, name: &str, mut data: Value) {
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    match name {
        "READY" => {
            let users: HashMap<String, Value> = data["users"].as_array().map(|list| list.iter().filter_map(|u| Some((u["id"].as_str()?.to_owned(), u.clone()))).collect()).unwrap_or_default();
            let mut dms: Vec<Value> = data["private_channels"].as_array().map(|list| list.iter().filter_map(|c| dm_view(c, &users)).collect()).unwrap_or_default();
            sort_dms(&mut dms);
            live.dms = dms;
            live.relationships = data["relationships"].as_array().map(|list| list.iter().filter_map(|r| rel_view(r, &users)).collect()).unwrap_or_default();
            live.presences = presences_of(&data);
            live.customs = customs_of(&data);
            live.reads = reads_of(&data);
            if data["user"].is_object() {
                live.me = user_view(&data["user"]);
            }
            let me = live.me_id();
            let own = guilds::merged_own(&data, &me);
            live.guilds = data["guilds"].as_array().map(|list| list.iter().filter_map(|g| guilds::Guild::parse(g, &me, own.get(g["id"].as_str()?))).collect()).unwrap_or_default();
            // A fresh session: Discord dropped the old one's voice states (and so any call).
            live.calls.reset();
            for g in data["guilds"].as_array().into_iter().flatten() {
                if let Some(id) = g["id"].as_str() {
                    live.calls.load_guild(id, &g["voice_states"]);
                }
            }
            live.guild_order = guilds::order(&data);
            live.muted = guilds::muted(&data);
            live.my_status = people::ready_status(&data);
            // A fresh session has no subscriptions: the open channel's goes again.
            live.resubscribe();
            // Kept only once Discord's gateway accepted it.
            if !live.saved {
                live.saved = auth::save(&state.profile, token);
            }
            let payload = ready_view(live);
            drop(guard);
            emit(app, "lc://ready", payload);
        }
        "VOICE_STATE_UPDATE" | "VOICE_SERVER_UPDATE" | "CALL_CREATE" | "CALL_UPDATE" | "CALL_DELETE" | "STREAM_CREATE" | "STREAM_SERVER_UPDATE" | "STREAM_DELETE" => {
            on_voice_dispatch(app, state, live, name, &data);
        }
        "READY_SUPPLEMENTAL" => {
            let me = live.me_id();
            for (id, member) in guilds::merged_own(&data, &me) {
                if let Some(g) = live.guilds.iter_mut().find(|g| g.id == id) {
                    g.set_member(&member);
                }
            }
            let views = live.guild_views();
            drop(guard);
            emit(app, "lc://guilds", json!(views));
        }
        "GUILD_CREATE" | "GUILD_UPDATE" | "GUILD_DELETE" | "GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE" | "GUILD_ROLE_DELETE" | "GUILD_MEMBER_UPDATE" => {
            let me = live.me_id();
            let id = data["guild_id"].as_str().or(data["id"].as_str()).unwrap_or("").to_owned();
            let index = live.guilds.iter().position(|g| g.id == id);
            match (name, index) {
                ("GUILD_CREATE", _) => {
                    let Some(g) = guilds::Guild::parse(&data, &me, None) else { return };
                    live.calls.load_guild(&g.id, &data["voice_states"]);
                    match index {
                        Some(i) => live.guilds[i] = g,
                        None => live.guilds.push(g),
                    }
                }
                ("GUILD_DELETE", Some(i)) => {
                    // Unavailable is an outage; otherwise this account left or was removed.
                    if data["unavailable"].as_bool() == Some(true) {
                        live.guilds[i].unavailable = true;
                    } else {
                        live.guilds.remove(i);
                        drop(guard);
                        emit(app, "lc://guild-removed", json!({ "id": id }));
                        return;
                    }
                }
                ("GUILD_UPDATE", Some(i)) => {
                    let g = &mut live.guilds[i];
                    if let Some(n) = data["name"].as_str() {
                        g.name = n.chars().take(100).collect();
                    }
                    g.icon = data["icon"].clone();
                    if let Some(o) = data["owner_id"].as_str() {
                        g.owner_id = o.to_owned();
                    }
                    if let Some(roles) = data["roles"].as_array() {
                        g.roles = roles.clone();
                    }
                }
                ("GUILD_ROLE_CREATE" | "GUILD_ROLE_UPDATE", Some(i)) => {
                    let role = &data["role"];
                    let roles = &mut live.guilds[i].roles;
                    roles.retain(|r| r["id"] != role["id"]);
                    roles.push(role.clone());
                }
                ("GUILD_ROLE_DELETE", Some(i)) => {
                    live.guilds[i].roles.retain(|r| r["id"] != data["role_id"]);
                }
                ("GUILD_MEMBER_UPDATE", Some(i)) => {
                    if data["user"]["id"] != me.as_str() {
                        return;
                    }
                    live.guilds[i].set_member(&data);
                }
                _ => return,
            }
            // A server arriving (joined, or back from an outage) takes its place in the order.
            if name == "GUILD_CREATE" {
                let views = live.guild_views();
                drop(guard);
                emit(app, "lc://guilds", json!(views));
                return;
            }
            let Some(i) = live.guilds.iter().position(|g| g.id == id) else { return };
            let view = live.guild_view(i);
            drop(guard);
            emit(app, "lc://guild", view);
        }
        "CHANNEL_CREATE" | "CHANNEL_UPDATE" | "CHANNEL_DELETE" if data["guild_id"].is_string() => {
            let Some(i) = live.guilds.iter().position(|g| g.id == data["guild_id"].as_str().unwrap_or("")) else { return };
            let channels = &mut live.guilds[i].channels;
            channels.retain(|c| c["id"] != data["id"]);
            if name != "CHANNEL_DELETE" {
                channels.push(data.clone());
            }
            let view = live.guild_view(i);
            drop(guard);
            emit(app, "lc://guild", view);
        }
        "THREAD_CREATE" | "THREAD_UPDATE" | "THREAD_DELETE" | "THREAD_LIST_SYNC" | "THREAD_MEMBER_UPDATE" | "THREAD_MEMBERS_UPDATE" => {
            let Some(i) = live.guilds.iter().position(|g| data["guild_id"] == g.id.as_str()) else { return };
            let me = live.me_id();
            let g = &mut live.guilds[i];
            let mut changed: Vec<String> = Vec::new();
            match name {
                "THREAD_CREATE" | "THREAD_UPDATE" => {
                    g.upsert_thread(&data);
                    // A thread this account just made is one it's in.
                    if name == "THREAD_CREATE" && data["owner_id"] == me.as_str() {
                        g.joined.insert(data["id"].as_str().unwrap_or("").to_owned());
                    }
                    changed.push(data["id"].as_str().unwrap_or("").to_owned());
                }
                "THREAD_DELETE" => {
                    let id = data["id"].as_str().unwrap_or("").to_owned();
                    g.remove_thread(&id);
                    let view = live.guild_view(i);
                    drop(guard);
                    emit(app, "lc://thread-removed", json!({ "id": id }));
                    emit(app, "lc://guild", view);
                    return;
                }
                "THREAD_LIST_SYNC" => {
                    for t in data["threads"].as_array().into_iter().flatten() {
                        g.upsert_thread(t);
                        changed.push(t["id"].as_str().unwrap_or("").to_owned());
                    }
                    for m in data["members"].as_array().into_iter().flatten().filter(|m| m["user_id"] == me.as_str()) {
                        if let Some(id) = m["id"].as_str() {
                            g.joined.insert(id.to_owned());
                        }
                    }
                }
                "THREAD_MEMBER_UPDATE" => {
                    if let Some(id) = data["id"].as_str() {
                        g.joined.insert(id.to_owned());
                        changed.push(id.to_owned());
                    }
                }
                _ => {
                    let id = data["id"].as_str().unwrap_or("").to_owned();
                    if data["added_members"].as_array().is_some_and(|a| a.iter().any(|m| m["user_id"] == me.as_str())) {
                        g.joined.insert(id.clone());
                    }
                    if data["removed_member_ids"].as_array().is_some_and(|r| r.iter().any(|u| u == me.as_str())) {
                        g.joined.remove(&id);
                    }
                    if let Some(n) = data["member_count"].as_u64() {
                        if let Some(t) = g.threads.iter_mut().find(|t| t["id"] == id.as_str()) {
                            t["member_count"] = json!(n);
                        }
                    }
                    changed.push(id);
                }
            }
            let views: Vec<Value> = changed.iter().filter_map(|id| g.channel(id).and_then(|t| g.thread_view(&me, t))).collect();
            let view = live.guild_view(i);
            drop(guard);
            for v in views {
                emit(app, "lc://thread", v);
            }
            emit(app, "lc://guild", view);
        }
        "GUILD_EMOJIS_UPDATE" => {
            let Some(g) = live.guilds.iter_mut().find(|g| data["guild_id"] == g.id.as_str()) else { return };
            g.emojis = guilds::emojis_of(&data["emojis"]);
            let me = live.me_id();
            let view = live.guilds.iter().find(|g| data["guild_id"] == g.id.as_str()).map(|g| g.view(&me));
            drop(guard);
            if let Some(v) = view {
                emit(app, "lc://guild", v);
            }
        }
        // The answer to the composer's member search: only ours (its nonce), cut down for the window.
        "GUILD_MEMBERS_CHUNK" => {
            let Some(nonce) = data["nonce"].as_str().filter(|n| n.starts_with("lc-")) else { return };
            let members: Vec<Value> = data["members"].as_array().into_iter().flatten().take(10)
                .map(|m| json!({ "user": user_view(&m["user"]), "nick": m["nick"] })).collect();
            drop(guard);
            emit(app, "lc://members-chunk", json!({ "guild_id": data["guild_id"], "nonce": nonce, "members": members }));
        }
        "GUILD_MEMBER_LIST_UPDATE" => {
            // Only the open server's list is asked for.
            if live.subscribed.as_ref().map(|(g, _, _)| g.as_str()) != data["guild_id"].as_str() {
                return;
            }
            drop(guard);
            emit(app, "lc://member-list", data);
        }
        "CHANNEL_UPDATE" => {
            let Some(dm) = dm_view(&data, &HashMap::new()) else { return };
            let Some(slot) = live.dms.iter_mut().find(|d| d["id"] == dm["id"]) else { return };
            *slot = dm;
            let dms = live.dms.clone();
            drop(guard);
            emit(app, "lc://dms", json!(dms));
        }
        "CHANNEL_DELETE" => {
            let before = live.dms.len();
            live.dms.retain(|d| d["id"] != data["id"]);
            if live.dms.len() == before {
                return;
            }
            let dms = live.dms.clone();
            drop(guard);
            emit(app, "lc://dms", json!(dms));
        }
        "CHANNEL_RECIPIENT_ADD" | "CHANNEL_RECIPIENT_REMOVE" => {
            let Some(dm) = live.dms.iter_mut().find(|d| d["id"] == data["channel_id"]) else { return };
            let Some(list) = dm["recipients"].as_array_mut() else { return };
            list.retain(|u| u["id"] != data["user"]["id"]);
            if name == "CHANNEL_RECIPIENT_ADD" && data["user"].is_object() {
                list.push(user_view(&data["user"]));
            }
            let dms = live.dms.clone();
            drop(guard);
            emit(app, "lc://dms", json!(dms));
        }
        "RELATIONSHIP_ADD" | "RELATIONSHIP_UPDATE" | "RELATIONSHIP_REMOVE" => {
            let existing = live.relationships.iter().position(|r| r["id"] == data["id"]);
            match (name, existing) {
                ("RELATIONSHIP_REMOVE", Some(i)) => {
                    live.relationships.remove(i);
                }
                ("RELATIONSHIP_REMOVE", None) => return,
                (_, Some(i)) => {
                    // An update may carry only what changed.
                    let r = &mut live.relationships[i];
                    r["type"] = data["type"].clone();
                    if data.get("nickname").is_some() {
                        r["nickname"] = data["nickname"].clone();
                    }
                    if data["user"].is_object() {
                        r["user"] = user_view(&data["user"]);
                    }
                }
                (_, None) => {
                    let Some(r) = rel_view(&data, &HashMap::new()) else { return };
                    live.relationships.push(r);
                }
            }
            let list = live.relationships.clone();
            drop(guard);
            emit(app, "lc://relationships", json!(list));
        }
        "PRESENCE_UPDATE" => {
            // A server's presences come with its member list (servers, later).
            if !data["guild_id"].is_null() {
                return;
            }
            let Some((id, status)) = presence(&data) else { return };
            live.presences.insert(id.clone(), status.clone());
            let custom = people::custom_status(&data);
            if custom.is_null() { live.customs.remove(&id); } else { live.customs.insert(id.clone(), custom.clone()); }
            drop(guard);
            emit(app, "lc://presence", json!({ "id": id, "status": status, "custom": custom }));
        }
        // The status changed, here or on another of this account's clients.
        "USER_SETTINGS_PROTO_UPDATE" | "USER_SETTINGS_UPDATE" => {
            let next = if name == "USER_SETTINGS_PROTO_UPDATE" {
                data["settings"]["proto"].as_str().filter(|_| data["settings"]["type"] == 1).and_then(|p| people::status_of_proto(p, live.my_status["status"].as_str().unwrap_or("online")))
            } else {
                data["status"].as_str().map(|_| people::ready_status(&json!({ "user_settings": data })))
            };
            let Some(next) = next else { return };
            live.my_status = next.clone();
            drop(guard);
            emit(app, "lc://my-status", next);
        }
        "MESSAGE_ACK" => {
            let (Some(channel), Some(message)) = (data["channel_id"].as_str(), data["message_id"].as_str()) else { return };
            let read = json!({ "last": message, "mentions": 0 });
            live.reads.insert(channel.to_owned(), read.clone());
            drop(guard);
            emit(app, "lc://read", json!({ "id": channel, "read": read }));
        }
        "CHANNEL_CREATE" => {
            if let Some(dm) = dm_view(&data, &HashMap::new()) {
                live.dms.retain(|d| d["id"] != dm["id"]);
                live.dms.insert(0, dm);
                let dms = live.dms.clone();
                drop(guard);
                emit(app, "lc://dms", json!(dms));
            }
        }
        "MESSAGE_CREATE" | "MESSAGE_UPDATE" | "MESSAGE_DELETE" | "TYPING_START" | "MESSAGE_REACTION_ADD" | "MESSAGE_REACTION_REMOVE"
        | "MESSAGE_REACTION_REMOVE_ALL" | "MESSAGE_REACTION_REMOVE_EMOJI" | "CHANNEL_PINS_UPDATE" | "MESSAGE_POLL_VOTE_ADD" | "MESSAGE_POLL_VOTE_REMOVE" => {
            let channel = data["channel_id"].as_str().unwrap_or("").to_owned();
            let channel = channel.as_str();
            // Encrypted DMs: key messages are recorded (and answered), messages opened, here, before
            // Guard, the UI or anything else sees them.
            if name == "MESSAGE_CREATE" || name == "MESSAGE_UPDATE" {
                if let Some(seen) = live.e2e_key(&state.profile, &mut data) {
                    e2e_after_key(app, state, live, seen);
                }
                live.e2e_open(&mut data);
                e2e_save_seen(app, state, live);
            }
            let mut read = None;
            let Some(index) = live.dms.iter().position(|d| d["id"].as_str() == Some(channel)) else {
                // A server channel this account can see, or nothing.
                let Some(g) = live.guild_with(channel) else { return };
                if name == "MESSAGE_CREATE" {
                    live.note_own(&data);
                    guard_on_message(app, live, &data);
                    let me = live.me_id();
                    let mentioned = live.guilds[g].mentions_me(&me, &data);
                    live.guilds[g].touch(channel, &data);
                    let mine = data["author"]["id"] == me.as_str();
                    if mentioned && !mine {
                        notify_message(app, state, live, &data, Some(g));
                    }
                    if mine || mentioned {
                        let entry = live.reads.entry(channel.to_owned()).or_insert_with(|| json!({ "last": null, "mentions": 0 }));
                        if mine {
                            *entry = json!({ "last": data["id"], "mentions": 0 });
                        } else {
                            entry["mentions"] = json!(entry["mentions"].as_u64().unwrap_or(0) + 1);
                        }
                        read = Some(json!({ "id": channel, "read": entry.clone() }));
                    }
                } else if name == "MESSAGE_DELETE" {
                    if let Some(id) = data["id"].as_str() {
                        live.own.remove(id);
                    }
                }
                drop(guard);
                emit(app, "lc://event", json!({ "t": name, "d": data }));
                if let Some(read) = read {
                    emit(app, "lc://read", read);
                }
                return;
            };
            if name == "MESSAGE_CREATE" {
                live.note_own(&data);
                guard_on_message(app, live, &data);
                notify_message(app, state, live, &data, None);
                live.dms[index]["last_message_id"] = data["id"].clone();
                let dm = live.dms.remove(index);
                live.dms.insert(0, dm);
                // In a DM every message from someone else counts as a mention; one's own marks
                // the channel read, as Discord does.
                let mine = data["author"]["id"] == live.me["id"];
                let entry = live.reads.entry(channel.to_owned()).or_insert_with(|| json!({ "last": null, "mentions": 0 }));
                if mine {
                    *entry = json!({ "last": data["id"], "mentions": 0 });
                } else {
                    entry["mentions"] = json!(entry["mentions"].as_u64().unwrap_or(0) + 1);
                }
                read = Some(json!({ "id": channel, "read": entry.clone() }));
            } else if name == "MESSAGE_DELETE" {
                if let Some(id) = data["id"].as_str() {
                    live.own.remove(id);
                }
            }
            drop(guard);
            emit(app, "lc://event", json!({ "t": name, "d": data }));
            if let Some(read) = read {
                emit(app, "lc://read", read);
            }
        }
        _ => {}
    }
}

fn rest_of(state: &App) -> Result<Arc<rest::Rest>, String> {
    state.live.lock().unwrap().as_ref().map(|l| l.rest.clone()).ok_or_else(|| "Not signed in.".to_owned())
}

// --- Commands the UI may call -------------------------------------------------------------------

#[tauri::command]
fn status(state: State<App>) -> Value {
    let live = state.live.lock().unwrap();
    match live.as_ref() {
        Some(l) => {
            let mut view = ready_view(l);
            view["profile"] = json!(state.profile);
            view["signedIn"] = json!(true);
            view["gateway"] = json!(l.gateway);
            view
        }
        None => json!({ "profile": state.profile, "signedIn": false }),
    }
}

// Each profile's own WebView2 folder: WebView2 refuses a folder another process holds with other
// settings, so two profiles sharing one couldn't run side by side. Its windows are incognito, so
// it keeps next to nothing.
fn webview_dir(app: &AppHandle, profile: &str) -> std::path::PathBuf {
    let base = app.path().app_local_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("lockcord"));
    base.join("webview").join(profile)
}

// Async: building a window from a synchronous command deadlocks on Windows.
#[tauri::command]
async fn sign_in(app: AppHandle, state: State<'_, App>) -> Result<(), String> {
    let done = app.clone();
    let closed = app.clone();
    auth::open(
        &app,
        webview_dir(&app, &state.profile),
        move |token| {
            let app = done.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(reason) = start_session(app.clone(), token, true).await {
                    emit(&app, "lc://session", json!({ "signedIn": false, "reason": reason }));
                }
            });
        },
        move || emit(&closed, "lc://login-closed", json!({})),
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn sign_in_with_token(app: AppHandle, token: String) -> Result<Value, String> {
    let token = token.trim().trim_matches('"').to_owned();
    if !auth::plausible_token(&token) {
        return Err("That doesn't look like a session token.".into());
    }
    start_session(app, token, true).await
}

#[tauri::command]
fn sign_out(app: AppHandle, state: State<App>) {
    stop_session(&state);
    auth::forget(&state.profile);
    emit(&app, "lc://session", json!({ "signedIn": false }));
}

// 50 messages: the newest, or those before, after or around a message (a jump to an old one).
#[tauri::command]
async fn messages(app: AppHandle, state: State<'_, App>, channel_id: String, before: Option<String>, after: Option<String>, around: Option<String>) -> Result<Value, String> {
    let bad = |v: &Option<String>| v.as_deref().is_some_and(|b| !is_id(b));
    if !is_id(&channel_id) || bad(&before) || bad(&after) || bad(&around) || [&before, &after, &around].iter().filter(|v| v.is_some()).count() > 1 {
        return Err("bad id".into());
    }
    // A server channel without Read Message History has none to show.
    let blind = state.live.lock().unwrap().as_ref().is_some_and(|l| {
        let me = l.me_id();
        l.guild_with(&channel_id).is_some_and(|g| l.guilds[g].channel(&channel_id).is_some_and(|c| l.guilds[g].permissions(&me, c) & guilds::READ_MESSAGE_HISTORY == 0))
    });
    if blind {
        return Ok(json!([]));
    }
    let rest = rest_of(&state)?;
    let mut path = format!("/channels/{channel_id}/messages?limit=50");
    for (key, value) in [("before", before), ("after", after), ("around", around)] {
        if let Some(v) = value {
            path.push_str(&format!("&{key}={v}"));
        }
    }
    let mut list = rest.get(&path).await.map_err(|e| e.to_string())?;
    if let (Some(live), Some(messages)) = (state.live.lock().unwrap().as_mut(), list.as_array_mut()) {
        // A message that started a thread carries it: kept, so it can be opened.
        if let Some(g) = live.guild_with(&channel_id) {
            for m in messages.iter() {
                if m["thread"].is_object() {
                    live.guilds[g].upsert_thread(&m["thread"]);
                }
            }
        }
        // Oldest first, as they happened, so offers and answers pair up in order.
        for m in messages.iter_mut().rev() {
            live.note_own(m);
            if let Some(seen) = live.e2e_key(&state.profile, m) {
                e2e_after_key(&app, &state, live, seen);
            }
        }
        for m in messages.iter_mut() {
            live.e2e_open(m);
        }
        e2e_save_seen(&app, &state, live);
    }
    Ok(list)
}

// A reply is `reply_to`; `ping: false` replies without notifying its author, as Discord's
// "@ OFF" does. `attachments` are files `upload_attachment` put on Discord's bucket. In an
// encrypted DM the text is sealed here, always, unless `plain` is true for this one message.
#[tauri::command]
async fn send_message(state: State<'_, App>, channel_id: String, content: String, reply_to: Option<String>, ping: Option<bool>, attachments: Option<Vec<Upload>>, plain: Option<bool>) -> Result<Value, String> {
    let attachments = attachments.unwrap_or_default();
    let nothing = content.trim().is_empty() && attachments.is_empty();
    if !is_id(&channel_id) || nothing || content.chars().count() > MAX_CONTENT || attachments.len() > 10 || reply_to.as_deref().is_some_and(|r| !is_id(r)) {
        return Err("Nothing to send, or too long.".into());
    }
    // Only files this client uploaded, each sent once.
    let guild_id;
    let (clean_links, no_embeds) = {
        let p = state.privacy.lock().unwrap();
        (p.clean_links, p.no_embeds)
    };
    let mut content = if clean_links { clean::clean_links(&content) } else { content };
    {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        if !attachments.iter().all(|a| live.uploads.contains(&a.uploaded_filename) && media::clean_name(&a.filename).as_deref() == Some(a.filename.as_str())) {
            return Err("Upload the files again.".into());
        }
        // In a server, only where this account may send (and attach).
        guild_id = live.guild_with(&channel_id).map(|g| {
            let me = live.me_id();
            let perms = live.guilds[g].channel(&channel_id).map(|c| live.guilds[g].permissions(&me, c)).unwrap_or(0);
            (live.guilds[g].id.clone(), perms)
        });
        if let Some((_, perms)) = &guild_id {
            if perms & guilds::SEND_MESSAGES == 0 || (!attachments.is_empty() && perms & guilds::ATTACH_FILES == 0) {
                return Err("You can't send that in this channel.".into());
            }
        }
        if let Some(peer) = live.encrypted_peer(&channel_id).filter(|_| plain != Some(true)) {
            if !attachments.is_empty() {
                return Err("Files can't go encrypted yet. Send them unencrypted on purpose, or not at all.".into());
            }
            content = seal_for(live, &peer, &channel_id, &content)?;
        }
        for a in &attachments {
            live.uploads.remove(&a.uploaded_filename);
        }
    }
    let rest = rest_of(&state)?;
    let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let nonce = fingerprint::nonce(unix_ms, state.nonces.fetch_add(1, Ordering::Relaxed));
    // Flag 4 (SUPPRESS_EMBEDS): links go without previews, when that's on.
    let flags = if no_embeds { 4 } else { 0 };
    let mut body = json!({ "content": content, "nonce": nonce, "tts": false, "flags": flags, "mobile_network_type": "unknown" });
    if !attachments.is_empty() {
        body["attachments"] = attachments.iter().enumerate().map(|(i, a)| json!({ "id": i.to_string(), "filename": a.filename, "uploaded_filename": a.uploaded_filename })).collect();
    }
    if let Some(message_id) = reply_to {
        body["message_reference"] = json!({ "channel_id": channel_id, "message_id": message_id });
        if let Some((guild, _)) = &guild_id {
            body["message_reference"]["guild_id"] = json!(guild);
        }
        if ping == Some(false) {
            body["allowed_mentions"] = json!({ "parse": ["users", "roles", "everyone"], "replied_user": false });
        }
    }
    let mut sent = rest.post(&format!("/channels/{channel_id}/messages"), body).await.map_err(|e| e.to_string())?;
    if let Some(live) = state.live.lock().unwrap().as_mut() {
        live.note_own(&sent);
        live.e2e_open(&mut sent);
    }
    Ok(sent)
}

// A desktop notification for a new message: a DM, or a mention in a server. Never for your own,
// a muted chat, or the chat on screen while LockCord has focus. By default it says who, not what;
// an encrypted DM's says only that it came (its text would end up in Windows' notification
// history). Secret chats never reach here.
fn notify_message(app: &AppHandle, state: &App, live: &Live, m: &Value, guild: Option<usize>) {
    let mode = state.notify.lock().unwrap().clone();
    let channel = m["channel_id"].as_str().unwrap_or("");
    if mode == "off" || m["author"]["id"] == live.me["id"] || m.get("lc_key").is_some() {
        return;
    }
    let guild_id = guild.map(|g| live.guilds[g].id.clone());
    if live.muted.1.iter().any(|c| c == channel) || guild_id.as_ref().is_some_and(|g| live.muted.0.contains(g)) {
        return;
    }
    let window = app.get_webview_window(MAIN);
    let focused = window.as_ref().and_then(|w| w.is_focused().ok()).unwrap_or(false);
    if focused && state.viewing.lock().unwrap().as_deref() == Some(channel) {
        return;
    }
    let who = m["member"]["nick"].as_str().or(m["author"]["global_name"].as_str()).or(m["author"]["username"].as_str()).unwrap_or("Someone").to_owned();
    let title = match guild {
        Some(g) => {
            let name = live.guilds[g].channel(channel).and_then(|c| c["name"].as_str()).unwrap_or("channel");
            format!("{who} · #{name} ({})", live.guilds[g].name)
        }
        None => match live.dms.iter().find(|d| d["id"] == channel).filter(|d| d["type"] == 3) {
            Some(group) => format!("{who} · {}", group["name"].as_str().unwrap_or("group")),
            None => who,
        },
    };
    let body = if m["lc_e2e"] == true || m["lc_e2e_locked"] == true || m["lc_e2e_failed"] == true || m["lc_e2e_untrusted"] == true {
        "🔒 New encrypted message".to_owned()
    } else if mode == "text" {
        let text = m["content"].as_str().unwrap_or("");
        if text.is_empty() {
            if m["attachments"].as_array().is_some_and(|a| !a.is_empty()) { "Sent a file".to_owned() } else { "New message".to_owned() }
        } else {
            let short: String = text.chars().take(140).collect();
            if short.len() < text.len() { format!("{short}…") } else { short }
        }
    } else if guild.is_some() {
        "Mentioned you".to_owned()
    } else {
        "New message".to_owned()
    };
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
    if !focused {
        if let Some(w) = window {
            let _ = w.request_user_attention(Some(tauri::UserAttentionType::Informational));
        }
    }
}

// Sealed for an encrypted DM; an error, never plain text, when the keys aren't there.
fn seal_for(live: &Live, peer: &str, channel: &str, text: &str) -> Result<String, String> {
    let identity = live.e2e.identity.as_ref().ok_or("Your encryption key is missing. Send unencrypted on purpose, or turn encryption on again.")?;
    if !live.e2e.book.contacts.contains_key(peer) {
        return Err("The keys for this encrypted DM are missing. Turn encryption on again, or send unencrypted on purpose.".into());
    }
    live.e2e.book.seal(identity, peer, channel, &live.me_id(), text, deletion::now_ms())
}

fn check_own(state: &App, channel_id: &str, message_id: &str) -> Result<(), String> {
    let own = state.live.lock().unwrap().as_ref().is_some_and(|l| l.own.contains(message_id));
    if !is_id(channel_id) || !own {
        return Err("Only your own messages.".into());
    }
    Ok(())
}

// In an encrypted DM an edit is sealed too, unless `plain`.
#[tauri::command]
async fn edit_message(state: State<'_, App>, channel_id: String, message_id: String, content: String, plain: Option<bool>) -> Result<Value, String> {
    check_own(&state, &channel_id, &message_id)?;
    if content.trim().is_empty() || content.chars().count() > MAX_CONTENT {
        return Err("Nothing to save, or too long.".into());
    }
    let content = if state.privacy.lock().unwrap().clean_links { clean::clean_links(&content) } else { content };
    let content = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        match live.encrypted_peer(&channel_id).filter(|_| plain != Some(true)) {
            Some(peer) => seal_for(live, &peer, &channel_id, &content)?,
            None => content,
        }
    };
    let mut edited = rest_of(&state)?.patch(&format!("/channels/{channel_id}/messages/{message_id}"), json!({ "content": content })).await.map_err(|e| e.to_string())?;
    if let Some(live) = state.live.lock().unwrap().as_mut() {
        live.e2e_open(&mut edited);
    }
    Ok(edited)
}

#[tauri::command]
async fn delete_message(state: State<'_, App>, channel_id: String, message_id: String) -> Result<(), String> {
    check_own(&state, &channel_id, &message_id)?;
    let queue = queue_of(&state)?;
    match queue.delete(&channel_id, &message_id, None).await {
        Ok(deletion::Outcome::Deleted) => Ok(()),
        Ok(deletion::Outcome::Refused(e)) => Err(e),
        _ => Err("Stopped.".into()),
    }
}

fn queue_of(state: &App) -> Result<deletion::Queue, String> {
    state.live.lock().unwrap().as_ref().map(|l| l.queue.clone()).ok_or_else(|| "Not signed in.".to_owned())
}

// --- Encrypted DMs (e2e.rs) ---------------------------------------------------------------------

fn load_e2e(app: &AppHandle, state: &App, me: &str) -> E2eState {
    let pinned = e2e::load_pinned(&state.profile);
    let identity = e2e::load_identity(&state.profile);
    let (book, problem) = match (&identity, store(app, state)) {
        (Some(id), Ok(store)) => match e2e::Book::load(id, me, &store.read_json("contacts.json")) {
            Ok(book) => (book, None),
            Err(e) => (e2e::Book::default(), Some(e)),
        },
        (None, _) if !pinned.is_empty() => (e2e::Book::default(), Some(if cfg!(target_os = "macos") { "Your encryption key is missing from the macOS Keychain." } else { "Your encryption key is missing from Windows Credential Manager." }.to_owned())),
        _ => (e2e::Book::default(), None),
    };
    // The cleared "compared" marks are kept, so the notice comes once.
    if book.code_upgraded {
        if let (Some(id), Ok(store)) = (&identity, store(app, state)) {
            let _ = store.write_json("contacts.json", &book.to_json(id));
        }
    }
    let problem = problem.or_else(|| {
        let missing = pinned.iter().filter(|p| !book.contacts.contains_key(*p)).count();
        (missing > 0).then(|| format!("The keys for {missing} encrypted DM{} are missing; sending there is blocked until encryption is turned on again.", if missing == 1 { "" } else { "s" }))
    });
    E2eState { identity, book, pinned, answering: HashSet::new(), problem }
}

fn save_e2e(app: &AppHandle, state: &App, live: &Live) -> Result<(), String> {
    let identity = live.e2e.identity.as_ref().ok_or("No encryption key.")?;
    store(app, state)?.write_json("contacts.json", &live.e2e.book.to_json(identity))?;
    e2e::save_pinned(&state.profile, &live.e2e.pinned)
}

fn e2e_changed(app: &AppHandle, live: &Live) {
    emit(app, "lc://e2e", live.e2e_view());
}

// Messages seen in encrypted DMs (for spotting copies posted again) are kept on disk.
fn e2e_save_seen(app: &AppHandle, state: &App, live: &mut Live) {
    if live.e2e.book.seen_dirty && save_e2e(app, state, live).is_ok() {
        live.e2e.book.seen_dirty = false;
    }
}

// After a key message was recorded: kept on disk, the UI told, and an offer answered.
fn e2e_after_key(app: &AppHandle, state: &App, live: &mut Live, (peer, event, answer): (String, e2e::Event, bool)) {
    if let Err(e) = save_e2e(app, state, live) {
        live.e2e.problem = Some(format!("Couldn't save the encryption contacts: {e}"));
    }
    e2e_changed(app, live);
    if event == e2e::Event::Changed {
        emit(app, "lc://e2e-alert", json!({ "peer": peer }));
    }
    if answer {
        live.e2e.answering.insert(peer.clone());
        tauri::async_runtime::spawn(e2e_post_key(app.clone(), peer, e2e::Role::Answer));
    }
}

// Posts this side's key message in the DM with `peer` (an offer, or the answer to theirs) and
// records it from Discord's reply. Plain on purpose: it carries only a public key and a nonce.
async fn e2e_post_key(app: AppHandle, peer: String, role: e2e::Role) -> Result<(), String> {
    let state = app.state::<App>();
    let prepared = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        let channel = live.dms.iter().find(|d| d["type"] == 1 && d["recipients"][0]["id"] == peer.as_str()).and_then(|d| d["id"].as_str()).map(str::to_owned);
        match (channel, live.e2e.identity.as_ref()) {
            (Some(channel), Some(identity)) => e2e::random32().map(|nonce| (channel, e2e::key_message(identity, &nonce, role), live.rest.clone())),
            _ => Err("No DM or key for that.".to_owned()),
        }
    };
    let result = match prepared {
        Ok((channel, content, rest)) => {
            let unix_ms = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
            let nonce = fingerprint::nonce(unix_ms, state.nonces.fetch_add(1, Ordering::Relaxed));
            let body = json!({ "content": content, "nonce": nonce, "tts": false, "flags": 0, "mobile_network_type": "unknown" });
            rest.post(&format!("/channels/{channel}/messages"), body).await.map_err(|e| e.to_string())
        }
        Err(e) => Err(e),
    };
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return Ok(()) };
    live.e2e.answering.remove(&peer);
    let mut sent = result?;
    live.note_own(&sent);
    if let Some(seen) = live.e2e_key(&state.profile, &mut sent) {
        e2e_after_key(&app, &state, live, seen);
    }
    Ok(())
}

#[tauri::command]
fn e2e_status(state: State<App>) -> Value {
    state.live.lock().unwrap().as_ref().map(|l| l.e2e_view()).unwrap_or(Value::Null)
}

// Turning encryption on with someone (or renewing the keys): this side's offer, posted in the DM.
// Their LockCord asks them first (or answers by itself if encryption was already on).
#[tauri::command]
async fn e2e_enable(app: AppHandle, state: State<'_, App>, channel_id: String) -> Result<(), String> {
    let peer = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        let peer = live.dm_peer(&channel_id).ok_or("Encryption is for one-to-one DMs.")?;
        if live.e2e.identity.is_none() {
            live.e2e.identity = Some(e2e::identity(&state.profile)?);
        }
        if live.e2e.identity.as_ref().is_some_and(|id| live.e2e.book.status(id, &peer) == e2e::Status::Changed) {
            return Err("Their key changed: accept or refuse it first.".into());
        }
        peer
    };
    e2e_post_key(app, peer, e2e::Role::Offer).await
}

// The user accepts the other side's offer: this side's answer, posted in the DM.
#[tauri::command]
async fn e2e_accept_offer(app: AppHandle, state: State<'_, App>, peer_id: String) -> Result<(), String> {
    {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        let identity = live.e2e.identity.as_ref().ok_or("No encryption key.")?;
        if !live.e2e.book.contacts.get(&peer_id).is_some_and(|c| c.offer_waiting(identity.public.as_bytes())) {
            return Err("There's no offer to accept.".into());
        }
        if !live.e2e.answering.insert(peer_id.clone()) {
            return Ok(());
        }
    }
    e2e_post_key(app, peer_id, e2e::Role::Answer).await
}

// The user leaves the other side's offer unanswered: nothing is posted, and it isn't asked again.
#[tauri::command]
fn e2e_ignore_offer(app: AppHandle, state: State<App>, peer_id: String) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    if !live.e2e.book.ignore_offer(&peer_id) {
        return Err("There's no offer to ignore.".into());
    }
    save_e2e(&app, &state, live)?;
    e2e_changed(&app, live);
    Ok(())
}

// The user accepts their new key (after comparing the safety code, ideally).
#[tauri::command]
fn e2e_accept_change(app: AppHandle, state: State<App>, peer_id: String) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    if !live.e2e.book.accept_change(&peer_id) {
        return Err("Nothing to accept.".into());
    }
    let answer = live.e2e.identity.as_ref().is_some_and(|id| live.e2e.book.contacts.get(&peer_id).is_some_and(|c| c.needs_answer(id.public.as_bytes())));
    e2e_after_key(&app, &state, live, (peer_id, e2e::Event::Nothing, answer));
    Ok(())
}

#[tauri::command]
fn e2e_verify(app: AppHandle, state: State<App>, peer_id: String, verified: bool) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    live.e2e.book.set_verified(&peer_id, verified);
    save_e2e(&app, &state, live)?;
    e2e_changed(&app, live);
    Ok(())
}

// Forgetting a contact: its keys go, so its encrypted history can't be read again, and the DM is
// plain again.
#[tauri::command]
fn e2e_forget(app: AppHandle, state: State<App>, peer_id: String) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    live.e2e.book.forget(&peer_id);
    live.e2e.pinned.remove(&peer_id);
    if live.e2e.identity.is_some() {
        save_e2e(&app, &state, live)?;
    } else {
        e2e::save_pinned(&state.profile, &live.e2e.pinned)?;
    }
    live.e2e.problem = None;
    e2e_changed(&app, live);
    Ok(())
}

// --- Calls (call.rs) -----------------------------------------------------------------------------

fn call_changed(app: &AppHandle, live: &Live) {
    emit(app, "lc://call", live.calls.active_view());
}

fn rooms_changed(app: &AppHandle, live: &Live, channels: &[String]) {
    for c in channels {
        emit(app, "lc://voice-room", live.calls.room_view(c));
    }
}

fn voice_settings(state: &App) -> call::VoiceSettings {
    state.voice.lock().unwrap().clone()
}

// A channel this account may call in: a DM or group DM, or a server voice channel it can connect
// to. (guild, the other person in a 1:1 DM, may speak.)
fn callable(live: &Live, channel: &str) -> Result<(Option<String>, Option<String>, bool), String> {
    if let Some(dm) = live.dms.iter().find(|d| d["id"] == channel && (d["type"] == 1 || d["type"] == 3)) {
        let peer = (dm["type"] == 1).then(|| dm["recipients"][0]["id"].as_str().map(str::to_owned)).flatten();
        return Ok((None, peer, true));
    }
    let g = live.guild_with(channel).ok_or("Not a channel you can see.")?;
    let guild = &live.guilds[g];
    let c = guild.channel(channel).ok_or("Not a channel you can see.")?;
    match c["type"].as_u64() {
        Some(2) => {}
        Some(13) => return Err("Stage channels aren't supported.".into()),
        _ => return Err("That isn't a voice channel.".into()),
    }
    let p = guild.permissions(&live.me_id(), c);
    if p & guilds::CONNECT == 0 {
        return Err("You can't connect to that channel.".into());
    }
    Ok((Some(guild.id.clone()), None, p & guilds::SPEAK != 0))
}

// Mute as the call's controls and push-to-talk have it.
fn effective_mute(a: &call::Active, settings: &call::VoiceSettings) -> bool {
    a.muted || a.deafened || (settings.push_to_talk && !a.ptt_held)
}

fn send_voice_state(live: &Live) {
    if let Some(a) = &live.calls.active {
        let _ = live.outbound.try_send(call::voice_state(a.guild.as_deref(), Some(&a.channel), a.muted, a.deafened, a.camera));
    }
}

// Starts the call's media once Discord gave both its voice session and its voice server.
fn call_maybe_start(app: &AppHandle, state: &App, live: &mut Live) {
    let me = live.me_id();
    let settings = voice_settings(state);
    let Some(a) = live.calls.active.as_mut() else { return };
    if a.media.is_some() || a.session.is_none() || a.server.is_none() {
        return;
    }
    let request = a.request;
    let (status_app, audio_app, end_app) = (app.clone(), app.clone(), app.clone());
    let status = move |s: discord_voice::Status| {
        call_status(&status_app, request, s);
        Ok(())
    };
    let audio_status = move |r: Result<(), &'static str>| {
        if let Err(e) = r {
            call_problem(&audio_app, request, e);
        }
    };
    let ended = move |r: Result<(), &'static str>| call_ended(&end_app, request, r.err());
    match call::start_media(a, &me, &settings, state.frames.clone(), status, audio_status, ended) {
        Ok(media) => {
            let muted = effective_mute(a, &settings);
            media.set_controls(muted, a.deafened);
            a.media = Some(media);
            a.phase = "connecting";
        }
        Err(e) => {
            a.error = Some(e);
            a.phase = "failed";
        }
    }
    call_changed(app, live);
}

// The voice crate's progress for call `request`.
fn call_status(app: &AppHandle, request: u64, status: discord_voice::Status) {
    if let discord_voice::Status::Speaking(users) = &status {
        let ids: Vec<String> = users.iter().filter(|u| **u != 0).map(u64::to_string).collect();
        emit(app, "lc://call-speaking", json!(ids));
        return;
    }
    let state = app.state::<App>();
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    let mut ring = None;
    {
        let Some(a) = live.calls.active.as_mut().filter(|a| a.request == request) else { return };
        match &status {
            discord_voice::Status::TransportReady => {
                if std::mem::take(&mut a.ring) {
                    ring = Some(a.channel.clone());
                }
            }
            discord_voice::Status::CameraAvailable(on) => a.camera_available = *on,
            discord_voice::Status::Ready { privacy_code } => {
                a.privacy = Some(privacy_code.chars().take(256).collect());
                a.error = None;
            }
            discord_voice::Status::WaitingForPeer => a.privacy = None,
            _ => {}
        }
        if let Some(phase) = call::phase_of(&status) {
            a.phase = phase;
            if let Some(m) = &a.media {
                m.set_ready(matches!(phase, "connected" | "waiting"));
            }
        }
    }
    if let Some(channel) = ring {
        let rest = live.rest.clone();
        tauri::async_runtime::spawn(async move {
            let _ = rest.post(&format!("/channels/{channel}/call/ring"), json!({ "recipients": null })).await;
        });
    }
    call_changed(app, live);
}

// Something wrong that doesn't end the call (a microphone that won't open).
fn call_problem(app: &AppHandle, request: u64, problem: &str) {
    let state = app.state::<App>();
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    if let Some(a) = live.calls.active.as_mut().filter(|a| a.request == request) {
        a.error = Some(problem.to_owned());
        call_changed(app, live);
    }
}

// The voice transport of call `request` ended: the call is left (Discord told, if it wasn't it
// that ended it).
fn call_ended(app: &AppHandle, request: u64, error: Option<&'static str>) {
    let state = app.state::<App>();
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    if !live.calls.active.as_ref().is_some_and(|a| a.request == request) {
        return;
    }
    call_leave_now(app, &state, live);
    let reason = match error {
        Some(e) if e == discord_voice::DISCONNECTED_BY_DISCORD => "Discord ended the call.".to_owned(),
        Some(e) => format!("The call ended: {e}"),
        None => "The call ended.".to_owned(),
    };
    emit(app, "lc://call-ended", json!({ "reason": reason }));
}

// Leaves the call this device is in: streams stopped, Discord told, media closed.
fn call_leave_now(app: &AppHandle, state: &App, live: &mut Live) {
    let Some(a) = live.calls.active.take() else { return };
    for s in [&a.share, &a.watch].into_iter().flatten() {
        let _ = live.outbound.try_send(json!({ "op": 19, "d": { "stream_key": s.key } }));
    }
    let _ = live.outbound.try_send(call::voice_state(a.guild.as_deref(), None, false, false, false));
    drop(a);
    state.frames.clear();
    call_changed(app, live);
}

fn on_voice_dispatch(app: &AppHandle, state: &App, live: &mut Live, name: &str, data: &Value) {
    let me = live.me_id();
    match name {
        "VOICE_STATE_UPDATE" => {
            let changed = live.calls.apply_state(data);
            rooms_changed(app, live, &changed);
            if data["user_id"] != me.as_str() {
                return;
            }
            let Some(a) = live.calls.active.as_mut() else { return };
            let channel = data["channel_id"].as_str();
            let session = data["session_id"].as_str().map(str::to_owned);
            if channel == Some(a.channel.as_str()) {
                // Until media starts, the newest state of this account in the call is the one.
                if a.media.is_none() {
                    a.session = session;
                    call_maybe_start(app, state, live);
                }
            } else if a.media.is_some() && session == a.session {
                // This session left the channel: moved or disconnected by someone, or Discord.
                call_leave_now(app, state, live);
                emit(app, "lc://call-ended", json!({ "reason": if channel.is_some() { "You were moved to another channel. Join it again to talk there." } else { "You were disconnected from the call." } }));
            }
        }
        "VOICE_SERVER_UPDATE" => {
            let Some(a) = live.calls.active.as_mut() else { return };
            let ours = match &a.guild {
                Some(g) => data["guild_id"] == g.as_str(),
                None => data["guild_id"].is_null() && (data["channel_id"].is_null() || data["channel_id"] == a.channel.as_str()),
            };
            let (Some(token), Some(endpoint)) = (data["token"].as_str(), data["endpoint"].as_str()) else { return };
            if !ours || token.len() > 2048 || endpoint.len() > 256 {
                return;
            }
            // A new voice server for a running call: its media starts over there.
            if a.media.take().is_some() {
                a.privacy = None;
            }
            a.server = Some((token.to_owned(), endpoint.to_owned()));
            call_maybe_start(app, state, live);
        }
        "CALL_CREATE" | "CALL_UPDATE" | "CALL_DELETE" => {
            let channel = data["channel_id"].as_str().unwrap_or("").to_owned();
            let was_rung = live.calls.dm_calls.get(&channel).is_some_and(|c| c["ringing"].as_array().is_some_and(|r| r.iter().any(|u| u == me.as_str())));
            let rung = live.calls.apply_dm_call(name, data, &me);
            emit(app, "lc://dm-call", json!({ "channel": channel, "call": live.calls.dm_calls.get(&channel) }));
            rooms_changed(app, live, std::slice::from_ref(&channel));
            let in_it = live.calls.active.as_ref().is_some_and(|a| a.channel == channel);
            if rung && !was_rung && !in_it {
                notify_call(app, state, live, &channel);
            }
        }
        "STREAM_CREATE" | "STREAM_SERVER_UPDATE" | "STREAM_DELETE" => on_stream_dispatch(app, state, live, name, data),
        _ => {}
    }
}

// Someone is calling: a notification and the taskbar flash (the window rings and shows the card).
fn notify_call(app: &AppHandle, state: &App, live: &Live, channel: &str) {
    emit(app, "lc://ring", json!({ "channel": channel }));
    if *state.notify.lock().unwrap() == "off" || live.muted.1.iter().any(|c| c == channel) {
        return;
    }
    let dm = live.dms.iter().find(|d| d["id"] == channel);
    let who = dm.map(|d| if d["type"] == 3 { d["name"].as_str().unwrap_or("A group").to_owned() } else {
        let r = &d["recipients"][0];
        r["global_name"].as_str().or(r["username"].as_str()).unwrap_or("Someone").to_owned()
    }).unwrap_or_else(|| "Someone".to_owned());
    let title = format!("{who} is calling");
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body("Answer in LockCord").show();
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.request_user_attention(Some(tauri::UserAttentionType::Critical));
    }
}

// Go Live: your stream's or a watched stream's connection, from Discord.
fn on_stream_dispatch(app: &AppHandle, state: &App, live: &mut Live, name: &str, data: &Value) {
    let me = live.me_id();
    let Some(key) = data["stream_key"].as_str().filter(|k| k.len() <= 128) else { return };
    let Some(a) = live.calls.active.as_mut() else { return };
    let mine = a.share.as_ref().is_some_and(|s| s.key == key);
    let watched = a.watch.as_ref().is_some_and(|s| s.key == key);
    if !mine && !watched {
        return;
    }
    if name == "STREAM_DELETE" {
        let reason = call::stream_end_reason(data);
        if mine {
            a.share = None;
        } else if let Some(w) = a.watch.take() {
            state.frames.drop_key(&format!("stream:{}", w.streamer.clone().unwrap_or_default()));
        }
        call_changed(app, live);
        emit(app, "lc://call-notice", json!({ "text": reason }));
        return;
    }
    let s = if mine { a.share.as_mut() } else { a.watch.as_mut() };
    let Some(s) = s else { return };
    match name {
        "STREAM_CREATE" => s.rtc = s.rtc.or_else(|| call::stream_rtc(data)),
        _ => {
            if let (Some(token), Some(endpoint)) = (data["token"].as_str().filter(|t| t.len() <= 2048), data["endpoint"].as_str().filter(|e| e.len() <= 256)) {
                s.server = Some((token.to_owned(), endpoint.to_owned()));
            }
        }
    }
    if s.task.is_some() || s.rtc.is_none() || s.server.is_none() {
        return;
    }
    let Some(media) = a.media.as_ref() else { return };
    let identity = media.identity.clone();
    let watching = !mine;
    let Some(s) = (if mine { a.share.as_ref() } else { a.watch.as_ref() }) else { return };
    let request = s.request;
    let creds = match call::stream_credentials(s, a, &me) {
        Ok(c) => c,
        Err(_) => return,
    };
    let stream_audio = media.stream_audio();
    let (status_app, end_app) = (app.clone(), app.clone());
    let status = move |st: discord_voice::Status| {
        stream_status(&status_app, request, watching, st);
        Ok(())
    };
    let s = if mine { a.share.as_mut() } else { a.watch.as_mut() };
    let Some(s) = s else { return };
    if mine {
        let Some(settings) = s.settings else { return };
        match discord_voice::screen::Worker::start(settings, || {}) {
            Ok((worker, video)) => {
                s.worker = Some(worker);
                s.task = Some(tauri::async_runtime::spawn(async move {
                    let r = discord_voice::run_stream(creds, identity, video, status).await;
                    stream_ended(&end_app, request, false, r.err());
                }));
            }
            Err(e) => {
                let key = s.key.clone();
                a.share = None;
                let _ = live.outbound.try_send(json!({ "op": 19, "d": { "stream_key": key } }));
                emit(app, "lc://call-notice", json!({ "text": format!("Couldn't share: {e}") }));
            }
        }
    } else {
        let sink = call::stream_sink(state.frames.clone(), s.streamer.clone().unwrap_or_default());
        s.task = Some(tauri::async_runtime::spawn(async move {
            let r = discord_voice::watch_stream(creds, identity, sink, Some(stream_audio), status).await;
            stream_ended(&end_app, request, true, r.err());
        }));
    }
    call_changed(app, live);
}

fn stream_status(app: &AppHandle, request: u64, watching: bool, status: discord_voice::Status) {
    let Some(phase) = call::phase_of(&status) else { return };
    let state = app.state::<App>();
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    let Some(a) = live.calls.active.as_mut() else { return };
    let s = if watching { a.watch.as_mut() } else { a.share.as_mut() };
    if let Some(s) = s.filter(|s| s.request == request) {
        s.phase = phase;
        call_changed(app, live);
    }
}

fn stream_ended(app: &AppHandle, request: u64, watching: bool, error: Option<&'static str>) {
    let state = app.state::<App>();
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    let Some(a) = live.calls.active.as_mut() else { return };
    let slot = if watching { &mut a.watch } else { &mut a.share };
    if !slot.as_ref().is_some_and(|s| s.request == request) {
        return;
    }
    let s = slot.take();
    if let Some(s) = &s {
        let _ = live.outbound.try_send(json!({ "op": 19, "d": { "stream_key": s.key } }));
        if watching {
            state.frames.drop_key(&format!("stream:{}", s.streamer.clone().unwrap_or_default()));
        }
    }
    call_changed(app, live);
    let what = if watching { "Watching" } else { "Your screen share" };
    emit(app, "lc://call-notice", json!({ "text": match error { Some(e) => format!("{what} stopped: {e}"), None => format!("{what} stopped.") } }));
}

// Joins a call: a DM's (ringing the others if nobody is in it yet) or a server voice channel.
// One call at a time: the window asks before switching.
#[tauri::command]
fn call_join(app: AppHandle, state: State<App>, channel_id: String) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    if live.calls.active.as_ref().is_some_and(|a| a.channel == channel_id) {
        return Ok(());
    }
    let (guild, peer, _speak) = callable(live, &channel_id)?;
    call_leave_now(&app, &state, live);
    let me = live.me_id();
    let ring = guild.is_none() && !live.calls.rooms.get(&channel_id).is_some_and(|r| r.keys().any(|u| *u != me));
    let request = live.calls.request();
    live.calls.active = Some(call::Active {
        channel: channel_id, guild, peer, request, ring, session: None, server: None, phase: "joining", privacy: None,
        muted: false, deafened: false, camera: false, camera_available: false, ptt_held: false, error: None,
        media: None, share: None, watch: None, volumes: HashMap::new(),
    });
    send_voice_state(live);
    call_changed(&app, live);
    Ok(())
}

#[tauri::command]
fn call_leave(app: AppHandle, state: State<App>) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    call_leave_now(&app, &state, live);
    Ok(())
}

// Declining a DM call that rings this account: the ringing stops here (the call goes on).
#[tauri::command]
async fn call_decline(state: State<'_, App>, channel_id: String) -> Result<(), String> {
    let (rest, me) = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        if !live.dms.iter().any(|d| d["id"] == channel_id.as_str()) {
            return Err("Not a DM.".into());
        }
        (live.rest.clone(), live.me_id())
    };
    rest.post(&format!("/channels/{channel_id}/call/stop-ringing"), json!({ "recipients": [me] })).await.map(|_| ()).map_err(|e| e.to_string())
}

// Asks Discord for a DM's call, if there is one (as opening a DM does in Discord's app).
#[tauri::command]
fn call_peek(state: State<App>, channel_id: String) -> Result<(), String> {
    let guard = state.live.lock().unwrap();
    let live = guard.as_ref().ok_or("Not signed in.")?;
    if live.dms.iter().any(|d| d["id"] == channel_id.as_str() && (d["type"] == 1 || d["type"] == 3)) {
        let _ = live.outbound.try_send(json!({ "op": 13, "d": { "channel_id": channel_id } }));
    }
    Ok(())
}

#[tauri::command]
fn call_controls(app: AppHandle, state: State<App>, muted: bool, deafened: bool) -> Result<(), String> {
    let settings = voice_settings(&state);
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let a = live.calls.active.as_mut().ok_or("You're not in a call.")?;
    a.muted = muted;
    a.deafened = deafened;
    if let Some(m) = &a.media {
        m.set_controls(effective_mute(a, &settings), deafened);
    }
    send_voice_state(live);
    call_changed(&app, live);
    Ok(())
}

// Push-to-talk, held or let go (the window sends it while it has focus).
#[tauri::command]
fn call_ptt(state: State<App>, held: bool) -> Result<(), String> {
    let settings = voice_settings(&state);
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let a = live.calls.active.as_mut().ok_or("You're not in a call.")?;
    a.ptt_held = held;
    if let Some(m) = &a.media {
        m.set_controls(effective_mute(a, &settings), a.deafened);
    }
    Ok(())
}

#[tauri::command]
fn call_camera(app: AppHandle, state: State<App>, on: bool) -> Result<(), String> {
    let settings = voice_settings(&state);
    let frames = state.frames.clone();
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let a = live.calls.active.as_mut().ok_or("You're not in a call.")?;
    let m = a.media.as_mut().ok_or("The call isn't connected yet.")?;
    if on {
        if !a.camera_available {
            return Err("Video isn't available in this call.".into());
        }
        let wake = app.clone();
        m.camera_on(settings.camera.clone(), frames, Arc::new(move || {
            // Called for every picture too: never waits for the lock, and only acts once the camera
            // stopped by itself (unplugged, in use elsewhere), turning the button off.
            let state = wake.state::<App>();
            let Ok(mut guard) = state.live.try_lock() else { return };
            let Some(live) = guard.as_mut() else { return };
            if let Some(a) = live.calls.active.as_mut() {
                if let Some(e) = a.media.as_ref().and_then(|m| m.camera_error()) {
                    a.error = Some(e.to_owned());
                    a.camera = false;
                    if let Some(m) = a.media.as_mut() {
                        m.camera_off();
                    }
                    send_voice_state(live);
                    call_changed(&wake, live);
                }
            }
        }))?;
    } else {
        m.camera_off();
        state.frames.drop_key("me");
    }
    a.camera = on;
    send_voice_state(live);
    call_changed(&app, live);
    Ok(())
}

#[tauri::command]
fn call_volume(app: AppHandle, state: State<App>, user_id: String, percent: u16) -> Result<(), String> {
    let settings = voice_settings(&state);
    let id: u64 = user_id.parse().map_err(|_| "bad user")?;
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let a = live.calls.active.as_mut().ok_or("You're not in a call.")?;
    a.volumes.insert(id, percent.min(200));
    if let Some(m) = &a.media {
        m.apply_settings(&settings, &a.volumes);
    }
    call_changed(&app, live);
    Ok(())
}

#[tauri::command]
fn call_share_sources() -> Result<Value, String> {
    call::share_sources()
}

// Starts sharing a screen or window in the call (Go Live).
#[tauri::command]
fn call_share(app: AppHandle, state: State<App>, kind: String, id: String, height: u32, fps: u32, audio: bool) -> Result<(), String> {
    let settings = call::share_settings(&kind, &id, height, fps, audio)?;
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let me = live.me_id();
    let request = live.calls.request();
    let a = live.calls.active.as_mut().ok_or("You're not in a call.")?;
    if a.media.is_none() || !matches!(a.phase, "connected" | "waiting") {
        return Err("The call isn't connected yet.".into());
    }
    if a.share.is_some() {
        return Err("You're already sharing.".into());
    }
    let key = call::stream_key(a.guild.as_deref(), &a.channel, &me);
    a.share = Some(call::Stream { key, request, rtc: None, server: None, task: None, worker: None, settings: Some(settings), streamer: None, phase: "connecting" });
    let frame = json!({ "op": 18, "d": { "type": if a.guild.is_some() { "guild" } else { "call" }, "guild_id": a.guild, "channel_id": a.channel, "preferred_region": null } });
    let _ = live.outbound.try_send(frame);
    call_changed(&app, live);
    Ok(())
}

#[tauri::command]
fn call_share_stop(app: AppHandle, state: State<App>) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let Some(s) = live.calls.active.as_mut().and_then(|a| a.share.take()) else { return Ok(()) };
    let _ = live.outbound.try_send(json!({ "op": 19, "d": { "stream_key": s.key } }));
    call_changed(&app, live);
    Ok(())
}

// Watches someone's stream in the call this device is in.
#[tauri::command]
fn call_watch(app: AppHandle, state: State<App>, user_id: String) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let me = live.me_id();
    if user_id == me || !is_id(&user_id) {
        return Err("bad user".into());
    }
    let request = live.calls.request();
    let a = live.calls.active.as_mut().ok_or("You're not in a call.")?;
    if a.media.is_none() {
        return Err("The call isn't connected yet.".into());
    }
    if let Some(old) = a.watch.take() {
        let _ = live.outbound.try_send(json!({ "op": 19, "d": { "stream_key": old.key } }));
        state.frames.drop_key(&format!("stream:{}", old.streamer.clone().unwrap_or_default()));
    }
    let a = live.calls.active.as_mut().ok_or("You're not in a call.")?;
    let key = call::stream_key(a.guild.as_deref(), &a.channel, &user_id);
    a.watch = Some(call::Stream { key: key.clone(), request, rtc: None, server: None, task: None, worker: None, settings: None, streamer: Some(user_id), phase: "connecting" });
    let _ = live.outbound.try_send(json!({ "op": 20, "d": { "stream_key": key } }));
    call_changed(&app, live);
    Ok(())
}

#[tauri::command]
fn call_watch_stop(app: AppHandle, state: State<App>) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let Some(w) = live.calls.active.as_mut().and_then(|a| a.watch.take()) else { return Ok(()) };
    let _ = live.outbound.try_send(json!({ "op": 19, "d": { "stream_key": w.key } }));
    state.frames.drop_key(&format!("stream:{}", w.streamer.clone().unwrap_or_default()));
    call_changed(&app, live);
    Ok(())
}

// A video tile's newest picture, if newer than `since` (call.rs Frames::take); empty if not.
#[tauri::command]
fn call_frame(state: State<App>, key: String, since: u32, max_width: u32) -> tauri::ipc::Response {
    tauri::ipc::Response::new(state.frames.take(&key, since, max_width))
}

// Microphones, speakers and cameras (listing opens none of them).
#[tauri::command]
async fn voice_devices() -> Value {
    tauri::async_runtime::spawn_blocking(call::devices).await.unwrap_or(Value::Null)
}

// The microphone test in Voice & Video settings: on, off, and its level (dBFS).
#[tauri::command]
fn voice_test(state: State<App>, on: bool) -> Result<(), String> {
    let mut test = state.mic_test.lock().unwrap();
    *test = None;
    if on {
        if state.live.lock().unwrap().as_ref().is_some_and(|l| l.calls.active.is_some()) {
            return Err("Leave the call to test your microphone.".into());
        }
        *test = Some(call::mic_test(&voice_settings(&state))?);
    }
    Ok(())
}

#[tauri::command]
fn voice_test_level(state: State<App>) -> Option<f32> {
    state.mic_test.lock().unwrap().as_ref().map(|a| a.preview_level_db())
}

// The composer's @mention search in a server (op 8). Nothing is asked for an encrypted DM or a
// secret chat: those never reach a server, and the window doesn't call this there.
#[tauri::command]
fn member_search(state: State<App>, guild_id: String, query: String, nonce: String) -> Result<(), String> {
    let query: String = query.chars().take(32).collect();
    if query.is_empty() || query.chars().any(char::is_control) || !nonce.starts_with("lc-") || nonce.len() > 32 {
        return Err("bad search".into());
    }
    let guard = state.live.lock().unwrap();
    let live = guard.as_ref().ok_or("Not signed in.")?;
    if !live.guilds.iter().any(|g| g.id == guild_id && !g.unavailable) {
        return Err("Not a server you're in.".into());
    }
    let _ = live.outbound.try_send(guilds::member_search(&guild_id, &query, &nonce));
    Ok(())
}

// --- Updates (update.rs) -----------------------------------------------------------------------------

// update.json: when it last checked, the version the user put off, and the newer release it knows.
fn update_state(app: &AppHandle, state: &App) -> Value {
    store(app, state).map(|s| s.read_json("update.json")).unwrap_or(Value::Null)
}

fn known_release(saved: &Value) -> Option<update::Release> {
    let k = &saved["known"];
    let digest = k["digest"].as_str().and_then(|hex| {
        let mut out = [0u8; 32];
        (hex.len() == 64).then_some(())?;
        for (i, b) in out.iter_mut().enumerate() {
            *b = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
        }
        Some(out)
    });
    let r = update::Release {
        version: k["version"].as_str()?.to_owned(),
        notes: k["notes"].as_str().unwrap_or("").to_owned(),
        page: k["page"].as_str()?.to_owned(),
        asset: k["asset"].as_str().filter(|a| update::allowed_asset_url(a))?.to_owned(),
        size: k["size"].as_u64().unwrap_or(0),
        digest,
    };
    update::newer(env!("CARGO_PKG_VERSION"), &r.version).then_some(r)
}

fn release_json(r: &update::Release) -> Value {
    json!({ "version": r.version, "notes": r.notes, "page": r.page, "asset": r.asset, "size": r.size,
        "digest": r.digest.map(|d| d.iter().map(|b| format!("{b:02x}")).collect::<String>()) })
}

// Whether there's a newer LockCord: asked of GitHub when `force` (Check now), or on start when
// the check is on and the last one is older than a day; otherwise what was found last time.
#[tauri::command]
async fn update_check(app: AppHandle, state: State<'_, App>, force: bool) -> Result<Value, String> {
    let current = env!("CARGO_PKG_VERSION");
    let enabled = store(&app, &state)?.settings()["checkUpdates"] != false;
    let mut saved = update_state(&app, &state);
    let last = saved["lastCheck"].as_u64().unwrap_or(0);
    let due = enabled && deletion::now_ms().saturating_sub(last) > update::CHECK_EVERY_MS;
    let mut problem = None;
    if force || due {
        match update::check(&state.http, current).await {
            Ok(found) => {
                saved["lastCheck"] = json!(deletion::now_ms());
                saved["known"] = found.as_ref().map(release_json).unwrap_or(Value::Null);
                let _ = store(&app, &state)?.write_json("update.json", &saved);
            }
            Err(e) => problem = Some(e),
        }
    }
    let release = known_release(&saved);
    // Put off with "Later": not shown again on its own until a newer one (Check now still shows it).
    let hidden = !force && release.as_ref().is_some_and(|r| saved["dismissed"] == r.version.as_str());
    Ok(json!({
        "current": current, "enabled": enabled, "lastCheck": saved["lastCheck"], "installed": update::installed(),
        "release": release.filter(|_| !hidden).map(|r| r.view()), "problem": problem,
    }))
}

#[tauri::command]
fn update_dismiss(app: AppHandle, state: State<App>, version: String) -> Result<(), String> {
    let mut saved = update_state(&app, &state);
    saved["dismissed"] = json!(version.chars().take(32).collect::<String>());
    store(&app, &state)?.write_json("update.json", &saved)
}

// Installing the newer release: an installed LockCord downloads its installer, checks it against
// GitHub's checksum, starts it and closes; a portable copy opens the release page instead.
#[tauri::command]
async fn update_install(app: AppHandle, state: State<'_, App>) -> Result<String, String> {
    let release = known_release(&update_state(&app, &state)).ok_or("There's no newer version to install.")?;
    if !update::installed() || release.digest.is_none() {
        app.opener().open_url(&release.page, None::<&str>).map_err(|e| e.to_string())?;
        return Ok("opened".into());
    }
    let progress = app.clone();
    let path = update::download(&release, env!("CARGO_PKG_VERSION"), move |done, total| {
        emit(&progress, "lc://update-progress", json!({ "done": done, "total": total }));
    }).await?;
    let digest = release.digest.ok_or("no checksum")?;
    if !update::still_matches(&path, &digest) {
        let _ = std::fs::remove_file(&path);
        return Err("The installer changed after it was downloaded, so it wasn't started.".into());
    }
    update::start_installer(&path)?;
    // The installer replaces this exe: LockCord gets out of its way (a call is left first).
    if let Some(live) = state.live.lock().unwrap().as_mut() {
        call_leave_now(&app, &state, live);
    }
    let quit = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(400)).await;
        quit.exit(0);
    });
    Ok("started".into())
}

// --- Translation (translate.rs) -----------------------------------------------------------------

// A channel whose text may go to Google: a DM or a server channel this account can see. (Secret
// chats never reach here; encrypted DMs are refused in e2e.rs's part.)
fn translatable(state: &App, channel_id: &str) -> Result<(), String> {
    let guard = state.live.lock().unwrap();
    let live = guard.as_ref().ok_or("Not signed in.")?;
    if !(live.dms.iter().any(|d| d["id"] == channel_id) || live.guild_with(channel_id).is_some()) {
        return Err("Not a channel you can see.".into());
    }
    if live.encrypted_peer(channel_id).is_some() {
        return Err("Encrypted DMs are never translated.".into());
    }
    Ok(())
}

#[tauri::command]
async fn translate_texts(state: State<'_, App>, channel_id: String, to: String, texts: Vec<String>) -> Result<Value, String> {
    translatable(&state, &channel_id)?;
    if !translate::valid_lang(&to) || to == "auto" || texts.len() > 50 {
        return Err("bad translation request".into());
    }
    let out = translate::translate_many(&state.http, "auto", &to, &texts).await?;
    Ok(json!(out.iter().map(|t| json!({ "text": t.text, "detected": t.detected })).collect::<Vec<_>>()))
}

#[tauri::command]
async fn translate_outgoing(state: State<'_, App>, channel_id: String, to: String, text: String) -> Result<String, String> {
    translatable(&state, &channel_id)?;
    if !translate::valid_lang(&to) || to == "auto" || text.chars().count() > MAX_CONTENT {
        return Err("bad translation request".into());
    }
    let out = translate::translate_many(&state.http, "auto", &to, &[text]).await?;
    Ok(out.into_iter().next().map(|t| t.text).unwrap_or_default())
}

// --- Purge (deletion.rs) ----------------------------------------------------------------------------

fn purge_event(app: &AppHandle, view: Value) {
    emit(app, "lc://purge", view);
}

// Scans the chosen channels (DMs and server channels this account can see, up to 200) for your
// messages that match; the result waits for purge_run, or purge_stop.
#[tauri::command]
fn purge_scan(app: AppHandle, state: State<App>, channels: Vec<Value>, filters: Value) -> Result<(), String> {
    let filters = deletion::Filters::from(&filters);
    let cancel = Arc::new(AtomicBool::new(false));
    let (rest, me, picked) = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        if live.purge.is_some() {
            return Err("A purge is already going. Stop it first.".into());
        }
        let readable = |id: &str| live.dms.iter().any(|d| d["id"] == id) || live.guild_with(id).is_some();
        let picked: Vec<(String, String)> = channels
            .iter()
            .filter_map(|c| Some((c["id"].as_str()?.to_owned(), c["name"].as_str().unwrap_or("").chars().take(100).collect::<String>())))
            .filter(|(id, _)| is_id(id) && readable(id))
            .take(200)
            .collect();
        if picked.is_empty() {
            return Err("Pick at least one channel.".into());
        }
        live.purge = Some(Purge { cancel: cancel.clone(), found: Vec::new() });
        (live.rest.clone(), live.me_id(), picked)
    };
    tauri::async_runtime::spawn(async move {
        let min_id = filters.min_id(deletion::now_ms());
        let mut found: Vec<(String, String)> = Vec::new();
        let mut unread = Vec::new();
        for (id, name) in &picked {
            if cancel.load(Ordering::Relaxed) {
                break;
            }
            let before = found.len();
            purge_event(&app, json!({ "state": "scanning", "found": before, "channel": name }));
            let progress = |n: usize| purge_event(&app, json!({ "state": "scanning", "found": before + n, "channel": name }));
            match deletion::collect(&rest, id, &me, &filters, min_id, &cancel, progress, |m| e2e_open_for(&app, m)).await {
                Ok(ids) => found.extend(ids.into_iter().map(|m| (id.clone(), m))),
                Err(e) => unread.push(format!("{name} ({e})")),
            }
        }
        let state = app.state::<App>();
        let mut guard = state.live.lock().unwrap();
        let Some(live) = guard.as_mut() else { return };
        if cancel.load(Ordering::Relaxed) || found.is_empty() {
            live.purge = None;
            drop(guard);
            let message = if cancel.load(Ordering::Relaxed) { "Stopped during the scan. Nothing was deleted." } else { "No messages matched. Nothing was deleted." };
            purge_event(&app, json!({ "state": if cancel.load(Ordering::Relaxed) { "stopped" } else { "done" }, "message": message, "unread": unread }));
            return;
        }
        let channels: HashSet<&String> = found.iter().map(|(c, _)| c).collect();
        let view = json!({ "state": "review", "total": found.len(), "channels": channels.len(), "unread": unread });
        if let Some(p) = live.purge.as_mut() {
            p.found = found;
        }
        drop(guard);
        purge_event(&app, view);
    });
    Ok(())
}

// Deletes what the scan found, through the queue.
#[tauri::command]
fn purge_run(app: AppHandle, state: State<App>) -> Result<(), String> {
    let (queue, cancel, found) = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        let p = live.purge.as_mut().filter(|p| !p.found.is_empty()).ok_or("Nothing to delete: scan first.")?;
        (live.queue.clone(), p.cancel.clone(), std::mem::take(&mut p.found))
    };
    tauri::async_runtime::spawn(async move {
        let total = found.len();
        let (mut deleted, mut failed) = (0, 0);
        for (channel, id) in &found {
            match queue.delete(channel, id, Some(cancel.clone())).await {
                Ok(deletion::Outcome::Deleted) => deleted += 1,
                Ok(deletion::Outcome::Refused(_)) => failed += 1,
                _ => break,
            }
            purge_event(&app, json!({ "state": "deleting", "deleted": deleted, "failed": failed, "total": total }));
        }
        let stopped = cancel.load(Ordering::Relaxed);
        if let Some(live) = app.state::<App>().live.lock().unwrap().as_mut() {
            live.purge = None;
        }
        let mut message = format!("{} Deleted {deleted} message{}.", if stopped { "Stopped." } else { "Done." }, if deleted == 1 { "" } else { "s" });
        if failed > 0 {
            message.push_str(&format!(" {failed} couldn't be deleted."));
        }
        purge_event(&app, json!({ "state": if stopped { "stopped" } else { "done" }, "deleted": deleted, "failed": failed, "total": total, "message": message }));
    });
    Ok(())
}

#[tauri::command]
fn purge_stop(app: AppHandle, state: State<App>) {
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    if let Some(p) = live.purge.take() {
        p.cancel.store(true, Ordering::Relaxed);
        // A review that's cancelled has nothing running to say so.
        if !p.found.is_empty() {
            drop(guard);
            purge_event(&app, json!({ "state": "stopped", "message": "Cancelled. Nothing was deleted." }));
        }
    }
}

// --- Guard (guard.rs) ------------------------------------------------------------------------------

fn save_guard(app: &AppHandle, state: &App, rules: &[guard::Rule]) -> Result<(), String> {
    store(app, state)?.write_json("guard.json", &guard::save(rules))
}

#[tauri::command]
fn guard_list(state: State<App>) -> Value {
    let rules = state.live.lock().unwrap().as_ref().map(|l| l.guard.clone()).unwrap_or_default();
    json!(rules.iter().map(guard::Rule::to_json).collect::<Vec<_>>())
}

// A new or changed rule for a channel this account can see; armed now.
#[tauri::command]
fn guard_set(app: AppHandle, state: State<App>, rule: Value) -> Result<Value, String> {
    let rule = guard::Rule::from(&rule, deletion::now_ms()).ok_or("A rule needs a trigger word or a time limit.")?;
    let rules = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        if !(live.dms.iter().any(|d| d["id"] == rule.channel.as_str()) || live.guild_with(&rule.channel).is_some()) {
            return Err("Not a channel you can see.".into());
        }
        live.guard.retain(|r| r.channel != rule.channel);
        live.guard.push(rule.clone());
        live.guard.clone()
    };
    save_guard(&app, &state, &rules)?;
    Ok(rule.to_json())
}

#[tauri::command]
fn guard_remove(app: AppHandle, state: State<App>, channel_id: String) -> Result<(), String> {
    let rules = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        live.guard.retain(|r| r.channel != channel_id);
        live.guard.clone()
    };
    save_guard(&app, &state, &rules)
}

// Your own new message in a guarded channel: the trigger wipes the channel, the time limit
// schedules its delete.
fn guard_on_message(app: &AppHandle, live: &mut Live, m: &Value) {
    let me = live.me_id();
    let channel = m["channel_id"].as_str().unwrap_or("");
    let Some(rule) = live.guard.iter().find(|r| r.channel == channel).cloned() else { return };
    if rule.is_trigger(m, &me) {
        tauri::async_runtime::spawn(guard_wipe(app.clone(), rule));
    } else if let Some(at) = rule.expires_at(m, &me) {
        let queue = live.queue.clone();
        let id = m["id"].as_str().unwrap_or("").to_owned();
        let app = app.clone();
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(at.saturating_sub(deletion::now_ms()))).await;
            // Only if the rule still stands.
            let standing = app.state::<App>().live.lock().unwrap().as_ref().is_some_and(|l| l.guard.iter().any(|r| r.channel == rule.channel && r.armed_at == rule.armed_at));
            if standing {
                let _ = queue.delete(&rule.channel, &id, None).await;
            }
        });
    }
}

// The trigger fired: all of your messages in the channel go.
async fn guard_wipe(app: AppHandle, rule: guard::Rule) {
    let (rest, queue, me) = {
        let state = app.state::<App>();
        let mut guard = state.live.lock().unwrap();
        let Some(live) = guard.as_mut() else { return };
        if !live.wiping.insert(rule.channel.clone()) {
            return;
        }
        (live.rest.clone(), live.queue.clone(), live.me_id())
    };
    let never = AtomicBool::new(false);
    let ids = deletion::collect(&rest, &rule.channel, &me, &deletion::Filters::all(), None, &never, |_| {}, |_| {}).await.unwrap_or_default();
    let mut deleted = 0;
    for id in &ids {
        if let Ok(deletion::Outcome::Deleted) = queue.delete(&rule.channel, id, None).await {
            deleted += 1;
        }
    }
    if let Some(live) = app.state::<App>().live.lock().unwrap().as_mut() {
        live.wiping.remove(&rule.channel);
    }
    let name = if rule.name.is_empty() { "a channel".to_owned() } else { rule.name.clone() };
    emit(&app, "lc://guard", json!({ "text": format!("Guard wiped {deleted} message{} in {name}.", if deleted == 1 { "" } else { "s" }) }));
}

// An encrypted DM's message fetched outside the usual paths (Purge, Guard's sweep), opened.
fn e2e_open_for(app: &AppHandle, m: &mut Value) {
    if let Some(live) = app.state::<App>().live.lock().unwrap().as_mut() {
        live.e2e_open(m);
    }
}

// Every minute (first after a few seconds): what was sent from elsewhere while LockCord wasn't
// watching. A trigger among a channel's newest messages wipes it; messages past the time limit go.
async fn guard_loop(app: AppHandle, mut stop: watch::Receiver<bool>) {
    let mut wait = std::time::Duration::from_secs(10);
    loop {
        tokio::select! {
            _ = tokio::time::sleep(wait) => {}
            _ = stop.changed() => return,
        }
        wait = std::time::Duration::from_secs(60);
        let snapshot = app.state::<App>().live.lock().unwrap().as_ref().map(|l| (l.rest.clone(), l.queue.clone(), l.me_id(), l.guard.clone()));
        let Some((rest, queue, me, rules)) = snapshot else { return };
        for rule in rules {
            if *stop.borrow() {
                return;
            }
            if !rule.trigger.is_empty() {
                let mut newest = rest.get(&format!("/channels/{}/messages?limit={}", rule.channel, guard::TRIGGER_LOOKBACK)).await.unwrap_or(Value::Null);
                // A trigger word in an encrypted DM is only found in the opened text.
                for m in newest.as_array_mut().into_iter().flatten() {
                    e2e_open_for(&app, m);
                }
                if newest.as_array().is_some_and(|list| list.iter().any(|m| rule.is_trigger(m, &me))) {
                    guard_wipe(app.clone(), rule.clone()).await;
                    continue;
                }
            }
            let Some((newest, oldest)) = rule.expiry_window(deletion::now_ms()) else { continue };
            let mut before = newest.to_string();
            'pages: loop {
                let page = rest.get(&format!("/channels/{}/messages?limit={}&before={before}", rule.channel, deletion::PAGE)).await.unwrap_or(Value::Null);
                let list = page.as_array().cloned().unwrap_or_default();
                for m in &list {
                    let id = m["id"].as_str().unwrap_or("");
                    if id.parse::<u64>().unwrap_or(0) < oldest {
                        break 'pages;
                    }
                    if rule.expires_at(m, &me).is_some() && deletion::Filters::all().matches(m, &me) {
                        let _ = queue.delete(&rule.channel, id, None).await;
                    }
                }
                if list.len() < deletion::PAGE {
                    break;
                }
                before = list.last().and_then(|m| m["id"].as_str()).unwrap_or("").to_owned();
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
}

// Adds or takes back this account's reaction.
#[tauri::command]
async fn react(state: State<'_, App>, channel_id: String, message_id: String, emoji: String, add: bool) -> Result<(), String> {
    let Some(emoji) = emoji_path(&emoji).filter(|_| is_id(&channel_id) && is_id(&message_id)) else {
        return Err("bad reaction".into());
    };
    let rest = rest_of(&state)?;
    let path = format!("/channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me");
    let done = if add { rest.put(&path).await } else { rest.delete(&path).await };
    done.map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
async fn typing(state: State<'_, App>, channel_id: String) -> Result<(), String> {
    if !is_id(&channel_id) {
        return Err("bad id".into());
    }
    // Ghost mode: nobody sees you typing.
    if state.privacy.lock().unwrap().ghost_typing(&channel_id) {
        return Ok(());
    }
    rest_of(&state)?.post(&format!("/channels/{channel_id}/typing"), json!({})).await.map(|_| ()).map_err(|e| e.to_string())
}

// One file for a Discord message, as raw bytes from the UI (the UI never has a path): headers
// `lc-channel` and `lc-name` (URI-encoded). Uploaded to Discord's bucket the web client's way;
// the answer goes into `send_message`'s `attachments`.
#[tauri::command]
async fn upload_attachment(state: State<'_, App>, request: tauri::ipc::Request<'_>) -> Result<Value, String> {
    let header = |name: &str| request.headers().get(name).and_then(|v| v.to_str().ok()).map(str::to_owned);
    let channel_id = header("lc-channel").filter(|c| is_id(c)).ok_or("bad channel")?;
    let filename = header("lc-name").and_then(|n| media::requested_url(&n)).and_then(|n| media::clean_name(&n)).ok_or("bad file name")?;
    let tauri::ipc::InvokeBody::Raw(bytes) = request.body() else { return Err("no file".into()) };
    let (clean_uploads, generic) = {
        let p = state.privacy.lock().unwrap();
        (p.clean_uploads, p.generic_names)
    };
    // Photo metadata (GPS, camera, time) is taken out before anything leaves.
    let bytes = if clean_uploads { clean::strip_metadata(bytes.clone())? } else { bytes.clone() };
    let filename = if generic { clean::generic_name(&filename) } else { filename };
    let limit = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        // A file for an encrypted DM would sit on Discord's bucket in the clear: only on purpose.
        if live.encrypted_peer(&channel_id).is_some() && header("lc-plain").as_deref() != Some("1") {
            return Err("Files can't go encrypted yet. Send them unencrypted on purpose, or not at all.".into());
        }
        live.upload_limit
    };
    if bytes.is_empty() || bytes.len() > limit {
        return Err(format!("Files can be up to {} MB here.", limit / (1024 * 1024)));
    }

    let rest = rest_of(&state)?;
    let slot = json!({ "files": [{ "filename": filename, "file_size": bytes.len(), "id": "0", "is_clip": false }] });
    let answer = rest.post(&format!("/channels/{channel_id}/attachments"), slot).await.map_err(|e| e.to_string())?;
    let target = &answer["attachments"][0];
    let upload_url = target["upload_url"].as_str().and_then(media::upload_allowed).ok_or("Discord gave no upload address")?;
    let uploaded = target["upload_filename"].as_str().filter(|n| !n.is_empty() && n.len() < 512).ok_or("Discord gave no upload name")?.to_owned();

    // Discord's bucket, not Discord's API: no token goes with it.
    netlog::note(&upload_url.to_string());
    let response = state
        .http
        .put(upload_url)
        .header("User-Agent", fingerprint::user_agent())
        .header("Content-Type", "application/octet-stream")
        .body(bytes)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err(format!("Upload failed ({}).", response.status()));
    }
    if let Some(live) = state.live.lock().unwrap().as_mut() {
        live.uploads.insert(uploaded.clone());
    }
    Ok(json!({ "filename": filename, "uploaded_filename": uploaded }))
}

#[derive(serde::Deserialize)]
struct Upload {
    filename: String,
    uploaded_filename: String,
}

// Opening a server channel: the op 37 the web client sends, for its typing, activity and member
// list (ranges: the list's 100-row windows in view, guilds.rs `member_ranges`). Only for a
// channel this account can see, and only when it changes.
#[tauri::command]
fn subscribe(state: State<App>, guild_id: String, channel_id: String, ranges: Option<Vec<[u32; 2]>>) -> Result<(), String> {
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let ok = live.guild_with(&channel_id).is_some_and(|g| live.guilds[g].id == guild_id);
    if !ok {
        return Err("Not a channel you can see.".into());
    }
    let want = Some((guild_id, channel_id, guilds::member_ranges(&ranges.unwrap_or_default())));
    if live.subscribed != want {
        live.subscribed = want;
        live.resubscribe();
    }
    Ok(())
}

// --- Channels: pins, threads, forum posts, polls (phase 5, part B) ------------------------------

// What this account may do in a conversation: None for a DM (everything a DM allows), the
// permissions in a server channel or thread; an error for anything it can't see.
fn conv_perms(live: &Live, channel_id: &str) -> Result<Option<u64>, String> {
    if live.dms.iter().any(|d| d["id"] == channel_id) {
        return Ok(None);
    }
    let g = live.guild_with(channel_id).ok_or("Not a channel you can see.")?;
    let me = live.me_id();
    Ok(live.guilds[g].channel(channel_id).map(|c| live.guilds[g].permissions(&me, c)))
}

// A channel's pinned messages, newest pin first (the web client's newer endpoint, or the older one).
#[tauri::command]
async fn pins(state: State<'_, App>, channel_id: String) -> Result<Value, String> {
    if !is_id(&channel_id) {
        return Err("bad id".into());
    }
    {
        let guard = state.live.lock().unwrap();
        let perms = conv_perms(guard.as_ref().ok_or("Not signed in.")?, &channel_id)?;
        if perms.is_some_and(|p| p & guilds::READ_MESSAGE_HISTORY == 0) {
            return Ok(json!([]));
        }
    }
    let rest = rest_of(&state)?;
    let mut list: Vec<Value> = match rest.get(&format!("/channels/{channel_id}/messages/pins?limit=50")).await {
        Ok(page) => page["items"].as_array().into_iter().flatten().filter_map(|i| {
            let mut m = i["message"].clone();
            m.is_object().then(|| {
                m["pinned_at"] = i["pinned_at"].clone();
                m
            })
        }).collect(),
        Err(rest::RestError::Status(404, _)) => rest.get(&format!("/channels/{channel_id}/pins")).await.map_err(|e| e.to_string())?.as_array().cloned().unwrap_or_default(),
        Err(e) => return Err(e.to_string()),
    };
    if let Some(live) = state.live.lock().unwrap().as_mut() {
        for m in list.iter_mut() {
            live.note_own(m);
            live.e2e_open(m);
        }
    }
    Ok(json!(list))
}

// Pinning or unpinning: anyone in a DM; in a server, with Manage Messages or Pin Messages.
#[tauri::command]
async fn pin(state: State<'_, App>, channel_id: String, message_id: String, on: bool) -> Result<(), String> {
    if !is_id(&channel_id) || !is_id(&message_id) {
        return Err("bad id".into());
    }
    {
        let guard = state.live.lock().unwrap();
        let perms = conv_perms(guard.as_ref().ok_or("Not signed in.")?, &channel_id)?;
        if perms.is_some_and(|p| p & (guilds::MANAGE_MESSAGES | guilds::PIN_MESSAGES) == 0) {
            return Err("You can't pin messages here.".into());
        }
    }
    let rest = rest_of(&state)?;
    let method = if on { reqwest::Method::PUT } else { reqwest::Method::DELETE };
    match rest.request(method.clone(), &format!("/channels/{channel_id}/messages/pins/{message_id}"), None).await {
        Err(rest::RestError::Status(404, _)) => rest.request(method, &format!("/channels/{channel_id}/pins/{message_id}"), None).await.map(|_| ()).map_err(|e| e.to_string()),
        done => done.map(|_| ()).map_err(|e| e.to_string()),
    }
}

// A channel's threads: the active ones known here, and a page of archived ones (public, and the
// private ones this account is in), each kept so it can be opened.
#[tauri::command]
async fn threads(state: State<'_, App>, channel_id: String, before: Option<String>) -> Result<Value, String> {
    if !is_id(&channel_id) || before.as_deref().is_some_and(|b| b.len() > 40 || !b.bytes().all(|c| c.is_ascii_alphanumeric() || b":.+-".contains(&c))) {
        return Err("bad id".into());
    }
    let active = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        let g = live.guild_with(&channel_id).ok_or("Not a channel you can see.")?;
        live.guilds[g].threads_of(&live.me_id(), &channel_id).into_iter().filter(|t| t["archived"] != true).collect::<Vec<_>>()
    };
    let rest = rest_of(&state)?;
    let mut path = format!("/channels/{channel_id}/threads/archived/public?limit=25");
    if let Some(b) = &before {
        path.push_str(&format!("&before={}", percent_encoding::utf8_percent_encode(b, percent_encoding::NON_ALPHANUMERIC)));
    }
    let page = rest.get(&path).await.map_err(|e| e.to_string())?;
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let g = live.guild_with(&channel_id).ok_or("Not a channel you can see.")?;
    let me = live.me_id();
    let mut archived = Vec::new();
    for t in page["threads"].as_array().into_iter().flatten() {
        live.guilds[g].upsert_thread(t);
        if let Some(v) = t["id"].as_str().and_then(|id| live.guilds[g].channel(id)).and_then(|c| live.guilds[g].thread_view(&me, c)) {
            archived.push(v);
        }
    }
    for m in page["members"].as_array().into_iter().flatten().filter(|m| m["user_id"] == me.as_str()) {
        if let Some(id) = m["id"].as_str() {
            live.guilds[g].joined.insert(id.to_owned());
        }
    }
    Ok(json!({ "active": active, "archived": archived, "has_more": page["has_more"].as_bool().unwrap_or(false) }))
}

// One thread as the UI opens it: known here already, or fetched (a "started a thread" message
// names only its id). Only in a server this account is in, under a channel it can see.
#[tauri::command]
async fn thread_open(state: State<'_, App>, thread_id: String) -> Result<Value, String> {
    if !is_id(&thread_id) {
        return Err("bad id".into());
    }
    let known = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        live.guild_with(&thread_id).and_then(|g| live.guilds[g].channel(&thread_id).and_then(|t| live.guilds[g].thread_view(&live.me_id(), t)))
    };
    if let Some(view) = known {
        return Ok(view);
    }
    let t = rest_of(&state)?.get(&format!("/channels/{thread_id}")).await.map_err(|e| e.to_string())?;
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let me = live.me_id();
    let g = live.guilds.iter().position(|g| t["guild_id"] == g.id.as_str()).filter(|_| guilds::is_thread(&t) && t["id"] == thread_id.as_str()).ok_or("Not a thread you can see.")?;
    live.guilds[g].upsert_thread(&t);
    live.guilds[g].channel(&thread_id).and_then(|c| live.guilds[g].thread_view(&me, c)).ok_or_else(|| "Not a thread you can see.".to_owned())
}

// Joining (following) or leaving a thread.
#[tauri::command]
async fn thread_join(state: State<'_, App>, thread_id: String, join: bool) -> Result<(), String> {
    let ok = is_id(&thread_id) && state.live.lock().unwrap().as_ref().is_some_and(|l| l.guild_with(&thread_id).is_some_and(|g| l.guilds[g].channel(&thread_id).is_some_and(guilds::is_thread)));
    if !ok {
        return Err("Not a thread you can see.".into());
    }
    let rest = rest_of(&state)?;
    let path = format!("/channels/{thread_id}/thread-members/@me");
    if join { rest.put(&path).await } else { rest.delete(&path).await }.map(|_| ()).map_err(|e| e.to_string())
}

// A forum's (or media channel's) posts, newest activity first, 25 at a time, with each post's
// first message: the search the web client's forum view makes, or else the archived list.
#[tauri::command]
async fn forum_posts(state: State<'_, App>, channel_id: String, offset: u32) -> Result<Value, String> {
    let ok = is_id(&channel_id) && offset <= 10_000 && state.live.lock().unwrap().as_ref().is_some_and(|l| {
        l.guild_with(&channel_id).is_some_and(|g| l.guilds[g].channel(&channel_id).is_some_and(|c| c["type"] == 15 || c["type"] == 16))
    });
    if !ok {
        return Err("Not a forum you can see.".into());
    }
    let rest = rest_of(&state)?;
    let search = format!("/channels/{channel_id}/threads/search?archived=true&sort_by=last_message_time&sort_order=desc&limit=25&tag_setting=match_some&offset={offset}");
    let page = match rest.get(&search).await {
        Ok(page) if page["threads"].is_array() => page,
        _ if offset == 0 => rest.get(&format!("/channels/{channel_id}/threads/archived/public?limit=25")).await.map_err(|e| e.to_string())?,
        Ok(_) => json!({ "threads": [] }),
        Err(e) => return Err(e.to_string()),
    };
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let g = live.guild_with(&channel_id).ok_or("Not a forum you can see.")?;
    let me = live.me_id();
    // The active posts known from the gateway first, on the first page.
    let mut posts: Vec<Value> = if offset == 0 { live.guilds[g].threads_of(&me, &channel_id).into_iter().filter(|t| t["archived"] != true).collect() } else { Vec::new() };
    for t in page["threads"].as_array().into_iter().flatten() {
        live.guilds[g].upsert_thread(t);
        let id = t["id"].as_str().unwrap_or("");
        if posts.iter().any(|p| p["id"] == id) {
            continue;
        }
        if let Some(v) = live.guilds[g].channel(id).and_then(|c| live.guilds[g].thread_view(&me, c)) {
            posts.push(v);
        }
    }
    for m in page["members"].as_array().into_iter().flatten().filter(|m| m["user_id"] == me.as_str()) {
        if let Some(id) = m["id"].as_str() {
            live.guilds[g].joined.insert(id.to_owned());
        }
    }
    // A post's first message has the post's id.
    let firsts: HashMap<String, Value> = page["first_messages"].as_array().into_iter().flatten().filter_map(|m| Some((m["id"].as_str()?.to_owned(), m.clone()))).collect();
    for p in posts.iter_mut() {
        if let Some(m) = p["id"].as_str().and_then(|id| firsts.get(id)) {
            p["first"] = json!({ "content": m["content"], "author": user_view(&m["author"]), "attachments": m["attachments"], "embeds": m["embeds"] });
        }
    }
    Ok(json!({ "posts": posts, "has_more": page["has_more"].as_bool().unwrap_or(false) }))
}

// A vote in a poll (several where it allows), or none to take the vote back.
#[tauri::command]
async fn poll_vote(state: State<'_, App>, channel_id: String, message_id: String, answers: Vec<u32>) -> Result<(), String> {
    if !is_id(&channel_id) || !is_id(&message_id) || answers.len() > 10 || answers.iter().any(|a| *a == 0 || *a > 100) {
        return Err("bad vote".into());
    }
    {
        let guard = state.live.lock().unwrap();
        conv_perms(guard.as_ref().ok_or("Not signed in.")?, &channel_id)?;
    }
    let ids: Vec<String> = answers.iter().map(u32::to_string).collect();
    rest_of(&state)?
        .request(reqwest::Method::PUT, &format!("/channels/{channel_id}/polls/{message_id}/answers/@me"), Some(json!({ "answer_ids": ids })))
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

// --- Search (phase 5, part C) ---------------------------------------------------------------------

// Searching a server or a DM the way the web client does, 25 at a time. Never an encrypted DM:
// Discord only has ciphertext there. Results from channels this account can't see are dropped.
#[tauri::command]
async fn search(state: State<'_, App>, guild_id: Option<String>, channel_id: Option<String>, query: Value, offset: u32) -> Result<Value, String> {
    let path = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        let base = match (&guild_id, &channel_id) {
            (Some(g), _) if is_id(g) && live.guilds.iter().any(|x| &x.id == g) => format!("/guilds/{g}/messages/search"),
            (None, Some(c)) if is_id(c) && live.dms.iter().any(|d| d["id"] == c.as_str()) => {
                if live.encrypted_peer(c).is_some() {
                    return Err("This DM is encrypted: Discord only has ciphertext, so it can't be searched.".into());
                }
                format!("/channels/{c}/messages/search")
            }
            _ => return Err("Nothing to search there.".into()),
        };
        let params = search::params(&query, offset, |c| guild_id.as_ref().is_some_and(|g| live.guild_with(c).is_some_and(|i| &live.guilds[i].id == g)))?;
        format!("{base}?{params}")
    };
    let rest = rest_of(&state)?;
    // Discord answers 202 while it builds the index: wait as long as it says, twice at most.
    let mut page = Value::Null;
    for attempt in 0..3 {
        page = rest.get(&path).await.map_err(|e| e.to_string())?;
        if page["messages"].is_array() {
            break;
        }
        if attempt == 2 {
            return Err("Discord is still indexing this for search. Try again in a moment.".into());
        }
        let wait = page["retry_after"].as_f64().unwrap_or(2.0).clamp(0.5, 8.0);
        tokio::time::sleep(std::time::Duration::from_secs_f64(wait)).await;
    }
    let mut guard = state.live.lock().unwrap();
    let live = guard.as_mut().ok_or("Not signed in.")?;
    let mut out = Vec::new();
    for group in page["messages"].as_array().into_iter().flatten() {
        let Some(m) = group.as_array().and_then(|g| g.iter().find(|m| m["hit"].as_bool() != Some(false)).or(g.first())) else { continue };
        let mut m = m.clone();
        let channel = m["channel_id"].as_str().unwrap_or("").to_owned();
        let seen = live.dms.iter().any(|d| d["id"] == channel.as_str()) || live.guild_with(&channel).is_some();
        if !seen {
            continue;
        }
        live.note_own(&m);
        live.e2e_open(&mut m);
        out.push(m);
    }
    let total = page["total_results"].as_u64().unwrap_or(out.len() as u64);
    Ok(json!({ "messages": out, "total": total, "has_more": u64::from(offset) + 25 < total }))
}

// Marks a channel read up to a message, as viewing it does in Discord.
#[tauri::command]
async fn ack(app: AppHandle, state: State<'_, App>, channel_id: String, message_id: String) -> Result<(), String> {
    if !is_id(&channel_id) || !is_id(&message_id) {
        return Err("bad id".into());
    }
    let already = state.live.lock().unwrap().as_ref().and_then(|l| l.reads.get(&channel_id)?.get("last")?.as_str().map(|last| !newer(&message_id, last)));
    if already == Some(true) {
        return Ok(());
    }
    // Ghost mode: read here, but Discord isn't told (other devices keep it unread).
    if !state.privacy.lock().unwrap().ghost_read(&channel_id) {
        rest_of(&state)?.post(&format!("/channels/{channel_id}/messages/{message_id}/ack"), json!({ "token": null })).await.map_err(|e| e.to_string())?;
    }
    let read = json!({ "last": message_id, "mentions": 0 });
    if let Some(live) = state.live.lock().unwrap().as_mut() {
        live.reads.insert(channel_id.clone(), read.clone());
    }
    emit(&app, "lc://read", json!({ "id": channel_id, "read": read }));
    Ok(())
}

// The X-Context-Properties the web client sends with these calls: base64 of where in the app the
// action came from.
fn context(location: &str) -> String {
    use base64::Engine;
    let props = if location.is_empty() { json!({}) } else { json!({ "location": location }) };
    base64::engine::general_purpose::STANDARD.encode(props.to_string())
}

// Accepting an incoming request, or removing a friend, a request either way, or a block; from a
// profile, also sending a request ("add") or blocking ("block") someone not on the list yet.
#[tauri::command]
async fn relationship(state: State<'_, App>, user_id: String, action: String) -> Result<(), String> {
    if !is_id(&user_id) {
        return Err("bad id".into());
    }
    let (kind, me) = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        (live.relationships.iter().find(|r| r["id"] == user_id.as_str()).and_then(|r| r["type"].as_u64()), live.me_id())
    };
    if user_id == me {
        return Err("That's you.".into());
    }
    let rest = rest_of(&state)?;
    let path = format!("/users/@me/relationships/{user_id}");
    let from_profile = context("User Profile");
    let props = context("Friends");
    let headers = [("X-Context-Properties", props.as_str())];
    let profile_headers = [("X-Context-Properties", from_profile.as_str())];
    let done = match (action.as_str(), kind) {
        ("accept", Some(3)) => rest.request_with(reqwest::Method::PUT, &path, Some(json!({})), &headers).await,
        ("remove", Some(_)) => rest.request_with(reqwest::Method::DELETE, &path, None, &headers).await,
        ("add", None) => rest.request_with(reqwest::Method::PUT, &path, Some(json!({})), &profile_headers).await,
        ("block", k) if k != Some(2) => rest.request_with(reqwest::Method::PUT, &path, Some(json!({ "type": 2 })), &profile_headers).await,
        _ => return Err("Not possible here.".into()),
    };
    done.map(|_| ()).map_err(|e| e.to_string())
}

// A friend request by username, as the Friends view's "Add Friend" sends it. A captcha, if Discord
// wants one, shows in its own window (captcha.rs).
#[tauri::command]
async fn add_friend(state: State<'_, App>, username: String) -> Result<(), String> {
    let name = username.trim().trim_start_matches('@').to_lowercase();
    if !people::username_ok(&name) {
        return Err("That isn't a Discord username (2-32 letters, numbers, _ and .).".into());
    }
    let props = context("Add Friend");
    rest_of(&state)?
        .request_with(reqwest::Method::POST, "/users/@me/relationships", Some(json!({ "username": name, "discriminator": null })), &[("X-Context-Properties", props.as_str())])
        .await
        .map(|_| ())
        .map_err(|e| match e {
            rest::RestError::Status(400, _) => "Hm, didn't work. Double-check the username: they may not exist, already be your friend, or not accept requests.".to_owned(),
            other => other.to_string(),
        })
}

// A user's profile, for the popout: from a server (its nickname, roles and join date) or not.
// Someone who turns out to share a server may be messaged from it (open_dm).
#[tauri::command]
async fn user_profile(state: State<'_, App>, user_id: String, guild_id: Option<String>) -> Result<Value, String> {
    if !is_id(&user_id) || guild_id.as_deref().is_some_and(|g| !is_id(g)) {
        return Err("bad id".into());
    }
    let me = {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        if guild_id.as_ref().is_some_and(|g| !live.guilds.iter().any(|x| &x.id == g)) {
            return Err("Not a server you're in.".into());
        }
        live.me_id()
    };
    let mut path = format!("/users/{user_id}/profile?");
    if user_id != me {
        path.push_str("with_mutual_guilds=true&with_mutual_friends=true&with_mutual_friends_count=true&");
    }
    if let Some(g) = &guild_id {
        path.push_str(&format!("guild_id={g}"));
    }
    let body = rest_of(&state)?.get(path.trim_end_matches(['?', '&'])).await.map_err(|e| e.to_string())?;
    let view = people::profile_view(&body);
    if view["mutual_guilds"].as_array().is_some_and(|m| !m.is_empty()) {
        if let Some(live) = state.live.lock().unwrap().as_mut() {
            if live.dm_ok.len() > 5000 {
                live.dm_ok.clear();
            }
            live.dm_ok.insert(user_id);
        }
    }
    Ok(view)
}

// --- GIFs for the picker (gifs.rs) --------------------------------------------------------------

// Trending: GIF categories and a preview for the "Trending GIFs" tile.
#[tauri::command]
async fn gif_trending(state: State<'_, App>) -> Result<Value, String> {
    let body = rest_of(&state)?.get(&format!("/gifs/trending?{}", gifs::query(None, &fingerprint::discord_locale()))).await.map_err(|e| e.to_string())?;
    Ok(gifs::trending_view(&body))
}

// Trending GIFs themselves, or a search (a category's tile searches for its name).
#[tauri::command]
async fn gif_search(state: State<'_, App>, q: Option<String>) -> Result<Value, String> {
    let q = q.map(|q| q.trim().chars().take(100).collect::<String>()).filter(|q| !q.is_empty());
    let locale = fingerprint::discord_locale();
    let path = match &q {
        Some(q) => format!("/gifs/search?{}", gifs::query(Some(q), &locale)),
        None => format!("/gifs/trending-gifs?{}&limit={}", gifs::query(None, &locale), gifs::MAX),
    };
    let body = rest_of(&state)?.get(&path).await.map_err(|e| e.to_string())?;
    Ok(gifs::gifs_view(&body))
}

// The account's favourite GIFs, read from its frecency settings (never written here).
#[tauri::command]
async fn gif_favorites(state: State<'_, App>) -> Result<Value, String> {
    let body = rest_of(&state)?.get("/users/@me/settings-proto/2").await.map_err(|e| e.to_string())?;
    Ok(gifs::favorites(body["settings"].as_str().unwrap_or("")))
}

// This account's status (online, idle, dnd, invisible), saved in Discord's settings as the web
// client does, then told to this session's gateway at once.
#[tauri::command]
async fn set_status(app: AppHandle, state: State<'_, App>, status: String) -> Result<Value, String> {
    if !people::STATUSES.contains(&status.as_str()) {
        return Err("Not a status.".into());
    }
    let rest = rest_of(&state)?;
    // Twice at most: a change made on another client meanwhile makes the first one out of date.
    let mut saved = false;
    for _ in 0..2 {
        let current = rest.get("/users/@me/settings-proto/1").await.map_err(|e| e.to_string())?;
        let body = people::status_patch(current["settings"].as_str().unwrap_or(""), &status)?;
        let answer = rest.patch("/users/@me/settings-proto/1", body).await.map_err(|e| e.to_string())?;
        if answer["out_of_date"].as_bool() != Some(true) {
            saved = true;
            break;
        }
    }
    if !saved {
        return Err("Your settings changed on another device meanwhile. Try again.".into());
    }
    let view = people::status_view(&status);
    if let Some(live) = state.live.lock().unwrap().as_mut() {
        live.my_status = view.clone();
        let _ = live.outbound.try_send(people::presence_frame(&status));
    }
    emit(&app, "lc://my-status", view.clone());
    Ok(view)
}

// A new group DM with up to nine friends.
#[tauri::command]
async fn create_group(app: AppHandle, state: State<'_, App>, user_ids: Vec<String>) -> Result<String, String> {
    if user_ids.is_empty() || user_ids.len() > 9 || user_ids.iter().any(|u| !is_id(u)) {
        return Err("Pick one to nine friends.".into());
    }
    {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        if !user_ids.iter().all(|u| live.relationships.iter().any(|r| r["id"] == u.as_str() && r["type"] == 1)) {
            return Err("Only friends can be added to a group.".into());
        }
    }
    let props = context("");
    let channel = rest_of(&state)?
        .request_with(reqwest::Method::POST, "/users/@me/channels", Some(json!({ "recipients": user_ids })), &[("X-Context-Properties", props.as_str())])
        .await
        .map_err(|e| e.to_string())?;
    let dm = dm_view(&channel, &HashMap::new()).ok_or("Discord didn't make the group.")?;
    let id = dm["id"].as_str().unwrap_or_default().to_owned();
    let dms = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        live.dms.retain(|d| d["id"] != dm["id"]);
        live.dms.insert(0, dm);
        live.dms.clone()
    };
    emit(&app, "lc://dms", json!(dms));
    Ok(id)
}

// A friend added to a group DM this account is in (ten people at most).
#[tauri::command]
async fn group_add(state: State<'_, App>, channel_id: String, user_id: String) -> Result<(), String> {
    if !is_id(&channel_id) || !is_id(&user_id) {
        return Err("bad id".into());
    }
    {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        let group = live.dms.iter().find(|d| d["id"] == channel_id.as_str() && d["type"] == 3).ok_or("Not a group you're in.")?;
        let members = group["recipients"].as_array().map(Vec::len).unwrap_or(0);
        if members + 1 >= 10 {
            return Err("A group holds ten people at most.".into());
        }
        if !live.relationships.iter().any(|r| r["id"] == user_id.as_str() && r["type"] == 1) {
            return Err("Only friends can be added to a group.".into());
        }
    }
    rest_of(&state)?.put(&format!("/channels/{channel_id}/recipients/{user_id}")).await.map(|_| ()).map_err(|e| e.to_string())
}

// Leaving a group DM; Discord's CHANNEL_DELETE then takes it off the list.
#[tauri::command]
async fn group_leave(app: AppHandle, state: State<'_, App>, channel_id: String) -> Result<(), String> {
    let ok = is_id(&channel_id) && state.live.lock().unwrap().as_ref().is_some_and(|l| l.dms.iter().any(|d| d["id"] == channel_id.as_str() && d["type"] == 3));
    if !ok {
        return Err("Not a group you're in.".into());
    }
    rest_of(&state)?.delete(&format!("/channels/{channel_id}?silent=false")).await.map_err(|e| e.to_string())?;
    let dms = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        live.dms.retain(|d| d["id"] != channel_id.as_str());
        live.dms.clone()
    };
    emit(&app, "lc://dms", json!(dms));
    Ok(())
}

// The DM with a friend: the one already open, or a new one.
#[tauri::command]
async fn open_dm(app: AppHandle, state: State<'_, App>, user_id: String) -> Result<String, String> {
    {
        let guard = state.live.lock().unwrap();
        let live = guard.as_ref().ok_or("Not signed in.")?;
        let existing = live.dms.iter().find(|d| d["type"] == 1 && d["recipients"].as_array().is_some_and(|r| r.len() == 1 && r[0]["id"] == user_id.as_str()));
        if let Some(id) = existing.and_then(|d| d["id"].as_str()) {
            return Ok(id.to_owned());
        }
        let friend = live.relationships.iter().any(|r| r["id"] == user_id.as_str() && r["type"] == 1);
        if !friend && !live.dm_ok.contains(&user_id) {
            return Err("Only with friends, or someone whose profile shows a server you share.".into());
        }
    }
    let props = context("");
    let channel = rest_of(&state)?
        .request_with(reqwest::Method::POST, "/users/@me/channels", Some(json!({ "recipients": [user_id] })), &[("X-Context-Properties", props.as_str())])
        .await
        .map_err(|e| e.to_string())?;
    let dm = dm_view(&channel, &HashMap::new()).ok_or("Discord didn't open a DM.")?;
    let id = dm["id"].as_str().unwrap_or_default().to_owned();
    let dms = {
        let mut guard = state.live.lock().unwrap();
        let live = guard.as_mut().ok_or("Not signed in.")?;
        live.dms.retain(|d| d["id"] != dm["id"]);
        live.dms.insert(0, dm);
        live.dms.clone()
    };
    emit(&app, "lc://dms", json!(dms));
    Ok(id)
}

// --- Settings and themes (settings.rs) -----------------------------------------------------------

fn store(app: &AppHandle, state: &App) -> Result<settings::Store, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    Ok(settings::Store::new(&dir, &state.profile))
}

// What the window needs to dress itself: the settings (the theme they name is built in), and
// whether it started in safe mode (the default theme, whatever the settings say).
#[tauri::command]
fn settings_get(app: AppHandle, state: State<App>) -> Result<Value, String> {
    Ok(json!({ "settings": store(&app, &state)?.settings(), "safeMode": state.safe_mode, "debug": cfg!(debug_assertions) }))
}

#[tauri::command]
fn settings_save(app: AppHandle, state: State<App>, settings: Value) -> Result<Value, String> {
    let saved = store(&app, &state)?.save_settings(&settings)?;
    apply_desktop(&app, &state, &saved);
    // Starting with Windows: the plugin's Run entry, named per profile.
    use tauri_plugin_autostart::ManagerExt;
    let autolaunch = app.autolaunch();
    let on = saved["autostart"] == true;
    if autolaunch.is_enabled().unwrap_or(false) != on {
        let done = if on { autolaunch.enable() } else { autolaunch.disable() };
        done.map_err(|e| format!("Starting with Windows couldn't be changed: {e}"))?;
    }
    Ok(saved)
}

fn apply_desktop(app: &AppHandle, state: &App, settings: &Value) {
    apply_shortcuts(app, settings);
    *state.privacy.lock().unwrap() = privacy::Privacy::from(&settings["privacy"]);
    refresh_shield(app, state);
    state.close_to_tray.store(settings["closeToTray"] != false, Ordering::Relaxed);
    *state.notify.lock().unwrap() = settings["notify"].as_str().unwrap_or("name").to_owned();
    // Voice & Video: kept for the next call, and applied to the one going on.
    let voice = call::VoiceSettings::from(&settings["voice"]);
    if let Some(a) = state.live.lock().unwrap().as_ref().and_then(|l| l.calls.active.as_ref()) {
        if let Some(m) = &a.media {
            m.apply_settings(&voice, &a.volumes);
            m.set_controls(effective_mute(a, &voice), a.deafened);
        }
    }
    *state.voice.lock().unwrap() = voice;
}

// Global mute and deafen keys (Voice & Video) and the panic key (Privacy): off unless set, and
// they work while LockCord is in the background too. Re-registered whenever the settings change.
fn apply_shortcuts(app: &AppHandle, settings: &Value) {
    use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
    let shortcuts = app.global_shortcut();
    let _ = shortcuts.unregister_all();
    if let Some(keys) = settings["privacy"]["panicKey"].as_str().filter(|k| !k.is_empty()) {
        let _ = shortcuts.on_shortcut(keys, move |app, _, event| {
            if event.state == ShortcutState::Pressed {
                panic(app);
            }
        });
    }
    let voice = &settings["voice"];
    for (key, deafen) in [("muteKey", false), ("deafenKey", true)] {
        let Some(keys) = voice[key].as_str().filter(|k| !k.is_empty()) else { continue };
        let _ = shortcuts.on_shortcut(keys, move |app, _, event| {
            if event.state == ShortcutState::Pressed {
                toggle_call_control(app, deafen);
            }
        });
    }
}

// The mute or deafen key: flips it in the call this device is in.
fn toggle_call_control(app: &AppHandle, deafen: bool) {
    let state = app.state::<App>();
    let settings = voice_settings(&state);
    let mut guard = state.live.lock().unwrap();
    let Some(live) = guard.as_mut() else { return };
    let Some(a) = live.calls.active.as_mut() else { return };
    if deafen {
        a.deafened = !a.deafened;
    } else {
        a.muted = !a.muted;
        if !a.muted {
            a.deafened = false;
        }
    }
    if let Some(m) = &a.media {
        m.set_controls(effective_mute(a, &settings), a.deafened);
    }
    send_voice_state(live);
    call_changed(app, live);
}

// The channel on screen (notifications skip it while the window has focus).
#[tauri::command]
fn viewing(app: AppHandle, state: State<App>, channel_id: Option<String>) {
    *state.viewing.lock().unwrap() = channel_id.filter(|c| is_id(c));
    refresh_shield(&app, &state);
}

// --- Privacy (privacy.rs, lock.rs) --------------------------------------------------------------------

// The window says whether a live secret chat is on screen (it runs there); Rust knows the rest.
#[tauri::command]
fn shield_hint(app: AppHandle, state: State<App>, secret: bool) {
    state.secret_on_screen.store(secret, Ordering::SeqCst);
    refresh_shield(&app, &state);
}

// The capture shield up or down, as the setting and the chat on screen say. Rust decides whether
// the chat is an encrypted DM; a secret chat it learns from the window.
fn refresh_shield(app: &AppHandle, state: &App) {
    let encrypted = {
        let viewing = state.viewing.lock().unwrap().clone();
        let guard = state.live.lock().unwrap();
        viewing.zip(guard.as_ref()).is_some_and(|(c, live)| live.encrypted_peer(&c).is_some())
    };
    let sensitive = encrypted || state.secret_on_screen.load(Ordering::SeqCst);
    let on = state.privacy.lock().unwrap().shield_on(sensitive);
    let mut up = state.shield_up.lock().unwrap();
    if *up == Some(on) {
        return;
    }
    if let Some(w) = app.get_webview_window(MAIN) {
        match privacy::set_capture_shield(&w, on) {
            Ok(()) => *up = Some(on),
            Err(e) => eprintln!("[shield] {e}"),
        }
    }
}

// Copying text: from a secret chat or an encrypted DM it's kept out of clipboard history and the
// cloud clipboard, and cleared after a while unless something else was copied since.
#[tauri::command]
async fn copy_text(state: State<'_, App>, text: String, sensitive: bool) -> Result<(), String> {
    if text.len() > 64 * 1024 {
        return Err("Too long to copy.".into());
    }
    let careful = sensitive && state.privacy.lock().unwrap().clipboard;
    let sequence = tauri::async_runtime::spawn_blocking(move || privacy::copy_text(&text, careful)).await.map_err(|e| e.to_string())??;
    if careful {
        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_secs(privacy::CLIPBOARD_CLEAR_SECS)).await;
            let _ = tauri::async_runtime::spawn_blocking(move || privacy::clear_clipboard_if(sequence)).await;
        });
    }
    Ok(())
}

// What LockCord sent where this session (netlog.rs): hosts and counts, nothing more.
#[tauri::command]
fn net_log() -> Value {
    netlog::view()
}

// Debug builds only: drops Discord's live connection as a lost network would; it should resume.
#[tauri::command]
fn debug_drop_gateway(state: State<App>) -> Result<(), String> {
    if !cfg!(debug_assertions) {
        return Err("Only in debug builds.".into());
    }
    let guard = state.live.lock().unwrap();
    let live = guard.as_ref().ok_or("Not signed in.")?;
    live.outbound.try_send(json!({ "lc_debug_drop": true })).map_err(|e| e.to_string())
}

// The panic key: LockCord hides in the tray at once, and the microphone goes quiet in a call.
fn panic(app: &AppHandle) {
    let state = app.state::<App>();
    if let Some(w) = app.get_webview_window(MAIN) {
        let _ = w.hide();
    }
    let settings = voice_settings(&state);
    let mut guard = state.live.lock().unwrap();
    if let Some(live) = guard.as_mut() {
        if let Some(a) = live.calls.active.as_mut().filter(|a| !a.muted) {
            a.muted = true;
            if let Some(m) = &a.media {
                m.set_controls(effective_mute(a, &settings), a.deafened);
            }
            send_voice_state(live);
            call_changed(app, live);
        }
    }
}

// --- Saving files (desktop.rs) ----------------------------------------------------------------------

fn save_dir(app: &AppHandle, state: &App) -> Option<std::path::PathBuf> {
    let stored = store(app, state).ok()?.read_json("save.json")["dir"].as_str().map(std::path::PathBuf::from);
    stored.filter(|p| p.is_absolute()).or_else(|| desktop::default_save_dir(app))
}

fn folder_name(dir: &std::path::Path) -> String {
    dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| dir.to_string_lossy().into_owned())
}

#[tauri::command]
fn save_folder(app: AppHandle, state: State<App>) -> Value {
    json!({ "folder": save_dir(&app, &state).map(|d| folder_name(&d)) })
}

// The folder files are saved to, picked in the native dialog (the window never sees the path).
#[tauri::command]
async fn pick_save_folder(app: AppHandle, state: State<'_, App>) -> Result<Value, String> {
    use tauri_plugin_dialog::DialogExt;
    let Some(dir) = app.dialog().file().set_title("Save LockCord files to").blocking_pick_folder().and_then(|p| p.into_path().ok()) else {
        return Ok(Value::Null);
    };
    store(&app, &state)?.write_json("save.json", &json!({ "dir": dir.to_string_lossy() }))?;
    Ok(json!({ "folder": folder_name(&dir) }))
}

// A message's files, saved: from Discord's hosts only, under safe names, marked as from the internet.
#[tauri::command]
async fn save_media(app: AppHandle, state: State<'_, App>, files: Vec<Value>) -> Result<Value, String> {
    if files.is_empty() || files.len() > 10 {
        return Err("Nothing to save.".into());
    }
    let dir = save_dir(&app, &state).ok_or("No folder to save to.")?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let mut saved = 0;
    let mut failed = Vec::new();
    for f in &files {
        let url = f["url"].as_str().unwrap_or("");
        let name = desktop::safe_name(f["filename"].as_str().unwrap_or("file"));
        let result = async {
            let bytes = media::download(&state.media_http, url, desktop::MAX_SAVE).await?;
            let (path, mut file) = desktop::reserve(&dir, &name)?;
            use std::io::Write;
            file.write_all(&bytes).map_err(|e| e.to_string())?;
            drop(file);
            desktop::mark_from_internet(&path);
            Ok::<(), String>(())
        }
        .await;
        match result {
            Ok(()) => saved += 1,
            Err(e) => failed.push(format!("{name}: {e}")),
        }
    }
    Ok(json!({ "saved": saved, "failed": failed, "folder": folder_name(&dir) }))
}

// `lccaptcha:` for the captcha window only: its page, with a CSP of its own (captcha.rs).
fn serve_captcha(ctx: tauri::UriSchemeContext<'_, tauri::Wry>, _request: tauri::http::Request<Vec<u8>>) -> tauri::http::Response<Vec<u8>> {
    let deny = |status: u16| tauri::http::Response::builder().status(status).body(Vec::new()).unwrap_or_default();
    if ctx.webview_label() != captcha::LABEL {
        return deny(403);
    }
    let state = ctx.app_handle().state::<App>();
    let current = state.captcha.current.lock().unwrap().clone();
    let Some((challenge, key, nonce)) = current else { return deny(404) };
    tauri::http::Response::builder()
        .status(200)
        .header("Content-Type", "text/html; charset=utf-8")
        .header("Content-Security-Policy", captcha::csp(&nonce))
        .header("Cache-Control", "no-store")
        .body(captcha::page(&challenge, &key, &nonce).into_bytes())
        .unwrap_or_default()
}

#[tauri::command]
fn open_link(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = url::Url::parse(&url).map_err(|_| "bad link".to_owned())?;
    if parsed.scheme() != "https" && parsed.scheme() != "http" {
        return Err("only web links open".into());
    }
    app.opener().open_url(parsed.as_str(), None::<&str>).map_err(|e| e.to_string())
}

// `lcmedia:` (media.rs), for LockCord's own window only: the login window, which shows
// discord.com, gets nothing from it.
fn serve_media(ctx: tauri::UriSchemeContext<'_, tauri::Wry>, request: tauri::http::Request<Vec<u8>>, responder: tauri::UriSchemeResponder) {
    let reply = |status: u16, kind: &str, body: Vec<u8>| {
        let mut response = tauri::http::Response::builder().status(status).header("Content-Type", kind);
        if status == 200 {
            response = response.header("Cache-Control", "private, max-age=3600");
        }
        response.body(body).unwrap_or_default()
    };
    if ctx.webview_label() != MAIN {
        responder.respond(reply(403, "text/plain", Vec::new()));
        return;
    }
    let app = ctx.app_handle().clone();
    let url = media::requested_url(request.uri().path());
    tauri::async_runtime::spawn(async move {
        let state = app.state::<App>();
        let fetched = match url {
            Some(url) => media::fetch(&state.media_http, &state.media, &url).await,
            None => Err("bad media path".into()),
        };
        responder.respond(match fetched {
            Ok((kind, bytes)) => reply(200, kind, bytes.to_vec()),
            Err(_) => reply(404, "text/plain", Vec::new()),
        });
    });
}

// Only LockCord's own window may call its commands. The capability (build.rs's manifest) already
// says so; this holds even if that list is ever got wrong.
fn only_main(handler: impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static) -> impl Fn(tauri::ipc::Invoke) -> bool + Send + Sync + 'static {
    move |invoke| {
        if !may_invoke(invoke.message.webview_ref().label()) {
            invoke.resolver.reject("Not allowed from this window.");
            return true;
        }
        handler(invoke)
    }
}

fn may_invoke(label: &str) -> bool {
    label == MAIN
}

fn main() {
    let profile = profile_arg();
    let http = reqwest::Client::builder().use_rustls_tls().gzip(true).build().expect("http client");
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        // Started with Windows: with this profile, and marked so "start hidden" can apply.
        .plugin(tauri_plugin_autostart::Builder::new()
            .args(["--autostart".to_owned(), format!("--profile={profile}")])
            .app_name(if profile == "default" { "LockCord".to_owned() } else { format!("LockCord ({profile})") })
            .build())
        .manage(App { profile: profile.clone(), safe_mode: std::env::args().any(|a| a == "--safe-mode"), http, live: Mutex::new(None), media: Mutex::new(media::Cache::default()), nonces: AtomicU64::new(0), captcha: Arc::new(captcha::Showing::default()),
            media_http: media::client(), viewing: Mutex::new(None), close_to_tray: AtomicBool::new(true), notify: Mutex::new("name".into()),
            voice: Mutex::new(call::VoiceSettings::from(&Value::Null)), frames: Arc::new(call::Frames::default()), mic_test: Mutex::new(None),
            privacy: Mutex::new(privacy::Privacy::default()), secret_on_screen: AtomicBool::new(false), shield_up: Mutex::new(None) })
        .register_asynchronous_uri_scheme_protocol(media::SCHEME, serve_media)
        .register_uri_scheme_protocol(captcha::SCHEME, serve_captcha)
        .invoke_handler(only_main(tauri::generate_handler![status, sign_in, sign_in_with_token, sign_out, messages, send_message, edit_message, delete_message, react, typing, upload_attachment, ack, relationship, open_dm, subscribe, settings_get, settings_save, purge_scan, purge_run, purge_stop, guard_list, guard_set, guard_remove, translate_texts, translate_outgoing, e2e_status, e2e_enable, e2e_accept_change, e2e_verify, e2e_forget, viewing, save_folder, pick_save_folder, save_media, open_link, add_friend, user_profile, set_status, create_group, group_add, group_leave, pins, pin, threads, thread_join, forum_posts, poll_vote, search, thread_open, e2e_accept_offer, e2e_ignore_offer, call_join, call_leave, call_decline, call_peek, call_controls, call_ptt, call_camera, call_volume, call_share_sources, call_share, call_share_stop, call_watch, call_watch_stop, call_frame, voice_devices, voice_test, voice_test_level, update_check, update_dismiss, update_install, member_search, gif_trending, gif_search, gif_favorites, shield_hint, copy_text, net_log, debug_drop_gateway]))
        .setup(move |app| {
            if let Ok(version) = tauri::webview_version() {
                fingerprint::set_engine_version(&version);
            }
            let title = if profile == "default" { "LockCord".to_owned() } else { format!("LockCord ({profile})") };
            let settings = store(app.handle(), &app.state::<App>()).map(|s| s.settings()).unwrap_or(Value::Null);
            // The app lock is gone (1.6.0): an older version's PIN hash isn't left in the vault.
            vault::delete(&format!("app-lock:{profile}"));
            apply_desktop(app.handle(), &app.state::<App>(), &settings);
            // Started with Windows and set to start hidden: only the tray icon shows.
            let hidden = std::env::args().any(|a| a == "--autostart") && settings["startHidden"] == true;
            // LockCord's own UI keeps nothing in the browser: its state is in Rust.
            let window = WebviewWindowBuilder::new(app, MAIN, WebviewUrl::App("index.html".into()))
                .title(&title)
                .inner_size(1240.0, 800.0)
                .min_inner_size(900.0, 560.0)
                .incognito(true)
                .data_directory(webview_dir(app.handle(), &profile))
                .visible(!hidden)
                // Dropped files reach the page as the browser's File objects, without their paths.
                .disable_drag_drop_handler();
            // LockCord draws its own title bar (titlebar.js), so themes can dress it; on macOS the
            // system's traffic-light buttons sit in it (the page leaves room for them).
            #[cfg(not(target_os = "macos"))]
            let window = window.decorations(false);
            #[cfg(target_os = "macos")]
            let window = window
                .title_bar_style(tauri::TitleBarStyle::Overlay)
                .hidden_title(true)
                .traffic_light_position(tauri::LogicalPosition::new(14.0, 13.0));
            window.build()?;
            // Closing hides to the tray when that's on; Quit is in the tray's menu.
            let closing = app.handle().clone();
            if let Some(window) = app.get_webview_window(MAIN) {
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        if closing.state::<App>().close_to_tray.load(Ordering::Relaxed) {
                            api.prevent_close();
                            if let Some(w) = closing.get_webview_window(MAIN) {
                                let _ = w.hide();
                            }
                        }
                    }
                });
            }
            desktop::tray(app.handle(), MAIN)?;

            let handle = app.handle().clone();
            let profile = profile.clone();
            tauri::async_runtime::spawn(async move {
                fingerprint::refresh_build_number(&handle.state::<App>().http).await;
                if let Some(token) = auth::load(&profile) {
                    if let Err(reason) = start_session(handle.clone(), token, false).await {
                        emit(&handle, "lc://session", json!({ "signedIn": false, "reason": reason }));
                    }
                } else {
                    emit(&handle, "lc://session", json!({ "signedIn": false }));
                }
            });
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("LockCord failed to start")
        .run(|_app, _event| {
            // macOS: clicking the Dock icon brings back a window that was closed to the tray.
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = _event {
                desktop::show(_app, MAIN);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_main_window_may_invoke() {
        assert!(may_invoke(MAIN));
        for other in [captcha::LABEL, "login", "", "main2", "Main"] {
            assert!(!may_invoke(other), "{other}");
        }
    }

    #[test]
    fn reaction_emoji_are_checked_and_encoded() {
        assert_eq!(emoji_path("👍").as_deref(), Some("%F0%9F%91%8D"));
        assert_eq!(emoji_path("1️⃣").as_deref(), Some("1%EF%B8%8F%E2%83%A3"));
        assert_eq!(emoji_path("blob_wave:123456789").as_deref(), Some("blob%5Fwave%3A123456789"));
        for bad in ["", "abc", "👍/../messages", "👍?x=1", "a:123", "blob:12x", "../@me", "%F0"] {
            assert_eq!(emoji_path(bad), None, "{bad}");
        }
    }

    #[test]
    fn ready_presences_reads_and_friends_in_either_shape() {
        let old = json!({
            "presences": [{ "user": { "id": "5" }, "status": "idle" }, { "user": { "id": "6" }, "status": "weird" }],
            "read_state": [{ "id": "100", "last_message_id": "900", "mention_count": 2 }],
        });
        let new = json!({
            "merged_presences": { "friends": [{ "user_id": "7", "status": "dnd" }] },
            "read_state": { "entries": [{ "id": "101", "last_message_id": "901" }] },
        });
        assert_eq!(presences_of(&old).get("5").map(String::as_str), Some("idle"));
        assert_eq!(presences_of(&old).get("6").map(String::as_str), Some("offline"));
        assert_eq!(presences_of(&new).get("7").map(String::as_str), Some("dnd"));
        assert_eq!(reads_of(&old)["100"], json!({ "last": "900", "mentions": 2 }));
        assert_eq!(reads_of(&new)["101"], json!({ "last": "901", "mentions": 0 }));

        let users = HashMap::from([("8".to_owned(), json!({ "id": "8", "username": "eight" }))]);
        assert_eq!(rel_view(&json!({ "id": "8", "type": 1 }), &users).unwrap()["user"]["username"], "eight");
        assert_eq!(rel_view(&json!({ "id": "9", "type": 3, "user": { "id": "9", "username": "nine" } }), &users).unwrap()["type"], 3);
        assert!(rel_view(&json!({ "id": "10", "type": 1 }), &users).is_none());
        assert!(newer("1000", "999") && !newer("999", "1000"));
    }

    // A Live for tests: me (100), a DM (555) with 200 (Bob), encryption on with him if `with_keys`;
    // and Bob's own book, to seal as him.
    fn e2e_live(with_keys: bool) -> (Live, e2e::Book, e2e::Identity) {
        let (stop, _) = watch::channel(false);
        let rest = Arc::new(rest::Rest::new(reqwest::Client::new(), String::new()));
        let mine = e2e::Identity::for_tests([1u8; 32]);
        let bob = e2e::Identity::for_tests([2u8; 32]);
        let ann = |id: &str, who: &e2e::Identity, n: u8, role| e2e::Announcement { id: id.into(), key: *who.public.as_bytes(), nonce: [n; 32], role, at: 1 };
        let (offer, answer) = (ann("10", &mine, 7, e2e::Role::Offer), ann("11", &bob, 9, e2e::Role::Answer));
        let mut book = e2e::Book::default();
        if with_keys {
            book.record(&mine, "100", "200", offer.clone(), true);
            book.record(&mine, "100", "200", answer.clone(), false);
        }
        let mut theirs = e2e::Book::default();
        theirs.record(&bob, "200", "100", offer, false);
        theirs.record(&bob, "200", "100", answer, true);
        let live = Live {
            rest, stop, me: json!({ "id": "100" }), dms: vec![json!({ "id": "555", "type": 1, "recipients": [{ "id": "200" }] })], gateway: "connected", saved: true,
            own: HashSet::new(), upload_limit: 0, uploads: HashSet::new(), relationships: Vec::new(), presences: HashMap::new(), customs: HashMap::new(), reads: HashMap::new(),
            guilds: Vec::new(), guild_order: Vec::new(), muted: (Vec::new(), Vec::new()), outbound: mpsc::channel(1).0, subscribed: None,
            queue: deletion::Queue::start(Arc::new(rest::Rest::new(reqwest::Client::new(), String::new()))), guard: Vec::new(), wiping: HashSet::new(), purge: None,
            e2e: E2eState { identity: Some(mine), book, pinned: HashSet::from(["200".to_owned()]), answering: HashSet::new(), problem: None },
            my_status: Value::Null, dm_ok: HashSet::new(), calls: call::Calls::default(),
        };
        (live, theirs, bob)
    }

    #[tokio::test]
    async fn encrypted_dms_open_only_where_they_belong() {
        let (mut live, theirs, bob) = e2e_live(true);
        let id = deletion::snowflake_at(deletion::now_ms()).to_string();
        let sealed = theirs.seal(&bob, "100", "555", "200", "meet at 6", deletion::now_ms()).unwrap();
        let mut m = json!({ "id": id, "channel_id": "555", "author": { "id": "200" }, "content": sealed });
        live.e2e_open(&mut m);
        assert_eq!((m["content"].as_str(), m["lc_e2e"].as_bool()), (Some("meet at 6"), Some(true)));

        // The same ciphertext posted elsewhere, or claimed by a third person: never readable.
        let mut moved = json!({ "id": id, "channel_id": "556", "author": { "id": "200" }, "content": sealed });
        live.e2e_open(&mut moved);
        assert_eq!((moved["content"].as_str(), moved["lc_e2e_locked"].as_bool()), (Some(""), Some(true)));
        let mut stranger = json!({ "id": id, "channel_id": "555", "author": { "id": "300" }, "content": sealed });
        live.e2e_open(&mut stranger);
        assert_eq!(stranger["content"], "");

        // Forwards and reply previews don't show ciphertext either.
        let mut fwd = json!({ "id": id, "channel_id": "557", "author": { "id": "200" }, "content": "", "message_snapshots": [{ "message": { "content": sealed } }],
            "referenced_message": { "id": id, "channel_id": "555", "author": { "id": "200" }, "content": sealed } });
        live.e2e_open(&mut fwd);
        assert_eq!(fwd["message_snapshots"][0]["message"]["content"], "");
        assert_eq!(fwd["referenced_message"]["content"], "meet at 6");

        assert_eq!(live.encrypted_peer("555").as_deref(), Some("200"));
        assert_eq!(live.encrypted_peer("556"), None);
        assert!(seal_for(&live, "200", "555", "hi").unwrap().starts_with("LC1:"));
    }

    #[tokio::test]
    async fn a_pinned_dm_without_keys_refuses_rather_than_going_plain() {
        let (live, _, _) = e2e_live(false);
        assert_eq!(live.encrypted_peer("555").as_deref(), Some("200"), "pinned in Credential Manager");
        assert!(seal_for(&live, "200", "555", "hi").is_err());
    }

    #[tokio::test]
    async fn only_this_accounts_messages_count_as_its_own() {
        let (stop, _) = watch::channel(false);
        let rest = Arc::new(rest::Rest::new(reqwest::Client::new(), String::new()));
        let mut live = Live {
            rest, stop, me: json!({ "id": "1" }), dms: Vec::new(), gateway: "connected", saved: true, own: HashSet::new(), upload_limit: 0, uploads: HashSet::new(),
            relationships: Vec::new(), presences: HashMap::new(), customs: HashMap::new(), reads: HashMap::new(),
            guilds: Vec::new(), guild_order: Vec::new(), muted: (Vec::new(), Vec::new()), outbound: mpsc::channel(1).0, subscribed: None,
            queue: deletion::Queue::start(Arc::new(rest::Rest::new(reqwest::Client::new(), String::new()))), guard: Vec::new(), wiping: HashSet::new(), purge: None, e2e: E2eState::default(),
            my_status: Value::Null, dm_ok: HashSet::new(), calls: call::Calls::default(),
        };
        live.note_own(&json!({ "id": "10", "author": { "id": "1" } }));
        live.note_own(&json!({ "id": "11", "author": { "id": "2" } }));
        assert!(live.own.contains("10"));
        assert!(!live.own.contains("11"));
    }
}
