//! Encrypted DMs between LockCord users: the everyday DM, end-to-end encrypted. The messages stay
//! in the DM on Discord as ciphertext, noise to everyone but the two LockCords that hold the keys,
//! and are readable there later (unlike a secret chat, which is live and peer to peer).
//!
//!   identity    one X25519 key pair per profile; the secret lives only in Windows Credential
//!               Manager, on this machine, DPAPI-wrapped (vault.rs, like the session), and never
//!               reaches the window. There's no forward secrecy: the message keys come from the
//!               two identities, so whoever got one later could read that DM's history.
//!   key message LockCords swap public keys through the DM itself: "🔑 LockCord key · LCK1:<o|a>:
//!               <public key>:<nonce>". Your click posts an offer (o); the other side answers (a)
//!               only after its user accepts, so a fake offer learns nothing. A renewal from
//!               someone encryption is already on with is answered by itself. Someone on Discord's
//!               own app only ever sees a key message after you chose to turn encryption on.
//!   epochs      every pair of announcements (one of yours, one of theirs) is an epoch: key =
//!               HKDF-SHA256(X25519(mine, theirs), salt = both nonces, info = version + both user
//!               ids), id = a hash of the pair, so both sides always agree on it, even when both
//!               offered at once. A new offer ("renew") is a new epoch; old ones stay readable.
//!   trust       the first key seen from someone is trusted (as Signal does). If their key ever
//!               changes, sending there stops and messages under the new key stay unread until
//!               the user accepts it: whoever swapped a key can neither read nor impersonate. The
//!               safety code depends only on the two identity keys, so it's compared once. It is
//!               60 digits: nothing commits a side to its key first, so a short code could be
//!               matched by someone generating keys in between.
//!   message     "LC1:<epoch>:<base64url(nonce ‖ AES-256-GCM)>". The authenticated data binds the
//!               channel, the sender and the epoch; the frame carries the send time (an old message
//!               posted again is flagged, and so is a second Discord message with the same
//!               ciphertext) and is padded to a size bucket (lengths don't leak).
//!   contacts    contacts.json in the profile folder (announcements, the trusted key; none of it
//!               secret) with an HMAC keyed from the identity, so tampering is noticed. Who has
//!               encryption on is also in Credential Manager, so a lost file never means plain text.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use serde_json::{Value, json};
use sha2::{Digest, Sha256, Sha512};
use std::collections::{HashMap, HashSet};
use x25519_dalek::{PublicKey, StaticSecret};
use zeroize::Zeroizing;

pub const PREFIX: &str = "LC1:";
const KEY_LEAD: &str = "🔑 LockCord key · ";
const KEY_PREFIX: &str = "LCK1:";
const INFO: &str = "LockCord DM v1";
const FRAME_VERSION: u8 = 1;
// [version][u64 sent-at ms][u32 length][text][zero padding]
const HEADER: usize = 1 + 8 + 4;
// The largest bucket keeps the whole message under Discord's 2,000 characters after base64.
const BUCKETS: [usize; 5] = [64, 128, 256, 512, 1024];
pub const MAX_TEXT_BYTES: usize = 1024 - HEADER;
/// How far a message's send time may be from when Discord says it was posted.
const CLOCK_SLACK_MS: u64 = 10 * 60 * 1000;
const MAX_ANNOUNCEMENTS: usize = 32;
// Key messages left alone (ignored offers, a forgotten contact's) and messages seen, per contact.
const MAX_IGNORED: usize = 128;
const MAX_SEEN: usize = 512;
// contacts.json's safety-code version: comparisons made with the old 6-digit code don't count.
const CODE_VERSION: u64 = 2;

#[derive(Debug, PartialEq)]
pub enum Opened {
    Text(String),
    /// Decrypted, but its send time is far from when it was posted: an old message posted again.
    Stale(String),
}

/// A sealed message's identity: its epoch and GCM nonce. The same pair under another Discord
/// message is a copy posted again.
type Seen = (u32, [u8; 12], String);

#[derive(Debug, PartialEq)]
pub enum E2eError {
    NotOurs,
    NoKey,
    Broken,
    /// Under a key of theirs that isn't the trusted one: not shown until it's accepted.
    Untrusted,
}

// --- Identity -----------------------------------------------------------------------------------

pub struct Identity {
    secret: StaticSecret,
    pub public: PublicKey,
}

impl Identity {
    fn from_bytes(bytes: [u8; 32]) -> Identity {
        let secret = StaticSecret::from(bytes);
        let public = PublicKey::from(&secret);
        Identity { secret, public }
    }

    #[cfg(test)]
    pub fn for_tests(bytes: [u8; 32]) -> Identity {
        Identity::from_bytes(bytes)
    }

    // The contacts file's MAC key: HKDF of the identity, so only this profile can sign it.
    fn mac_key(&self) -> Zeroizing<[u8; 32]> {
        let hk = Hkdf::<Sha256>::new(None, self.secret.as_bytes());
        let mut key = Zeroizing::new([0u8; 32]);
        hk.expand(b"LockCord contacts MAC v1", key.as_mut()).expect("32 bytes");
        key
    }
}

fn vault_name(profile: &str, what: &str) -> String {
    format!("{what}:{profile}")
}

/// The profile's identity, if it has one.
pub fn load_identity(profile: &str) -> Option<Identity> {
    let stored = crate::vault::get(&vault_name(profile, "e2e-identity"))?;
    let bytes = Zeroizing::new(STANDARD.decode(stored.as_bytes()).ok()?);
    let array: [u8; 32] = bytes.as_slice().try_into().ok()?;
    Some(Identity::from_bytes(array))
}

/// The profile's identity, made and stored the first time it's needed.
pub fn identity(profile: &str) -> Result<Identity, String> {
    if let Some(id) = load_identity(profile) {
        return Ok(id);
    }
    let mut bytes = Zeroizing::new([0u8; 32]);
    getrandom::fill(bytes.as_mut()).map_err(|e| e.to_string())?;
    crate::vault::set(&vault_name(profile, "e2e-identity"), &Zeroizing::new(STANDARD.encode(bytes.as_ref())))?;
    // Read back, so what's used is what was stored.
    load_identity(profile).ok_or_else(|| "Couldn't keep the encryption key in Credential Manager.".to_owned())
}

/// The peers encryption is on with, kept in Credential Manager too: if contacts.json goes missing
/// or is tampered with, these DMs still never go out in plain text.
pub fn load_pinned(profile: &str) -> HashSet<String> {
    crate::vault::get(&vault_name(profile, "e2e-pinned"))
        .map(|list| list.split(',').filter(|p| is_id(p)).map(str::to_owned).collect())
        .unwrap_or_default()
}

pub fn save_pinned(profile: &str, pinned: &HashSet<String>) -> Result<(), String> {
    let name = vault_name(profile, "e2e-pinned");
    if pinned.is_empty() {
        crate::vault::delete(&name);
        return Ok(());
    }
    let mut list: Vec<&str> = pinned.iter().map(String::as_str).collect();
    list.sort();
    crate::vault::set(&name, &list.join(","))
}

pub fn random32() -> Result<[u8; 32], String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b)
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

// --- Key messages -----------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Role {
    Offer,
    Answer,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Announcement {
    /// The Discord message it came in.
    pub id: String,
    pub key: [u8; 32],
    pub nonce: [u8; 32],
    pub role: Role,
    pub at: u64,
}

/// The message LockCord posts in a DM to share this side's key.
pub fn key_message(identity: &Identity, nonce: &[u8; 32], role: Role) -> String {
    let r = if role == Role::Offer { "o" } else { "a" };
    format!("{KEY_LEAD}{KEY_PREFIX}{r}:{}:{}", URL_SAFE_NO_PAD.encode(identity.public.as_bytes()), URL_SAFE_NO_PAD.encode(nonce))
}

/// A key message, read back: exactly LockCord's own shape, nothing more.
pub fn parse_key_message(content: &str) -> Option<(Role, [u8; 32], [u8; 32])> {
    let rest = content.strip_prefix(KEY_LEAD)?.strip_prefix(KEY_PREFIX)?;
    let mut parts = rest.split(':');
    let role = match parts.next()? {
        "o" => Role::Offer,
        "a" => Role::Answer,
        _ => return None,
    };
    let key: [u8; 32] = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?.try_into().ok()?;
    let nonce: [u8; 32] = URL_SAFE_NO_PAD.decode(parts.next()?).ok()?.try_into().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((role, key, nonce))
}

// --- Contacts -----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    /// Nothing announced either way.
    Off,
    /// One side announced; the other hasn't answered yet. Plain text still goes out.
    Waiting,
    /// Both announced, with the trusted key: everything is sealed.
    On,
    /// Their newest key isn't the trusted one: nothing is sent until the user accepts it.
    Changed,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Contact {
    pub peer: String,
    pub mine: Vec<Announcement>,
    pub theirs: Vec<Announcement>,
    /// Their key, trusted on first sight; changes only when the user accepts a new one.
    pub trusted: Option<[u8; 32]>,
    /// The user compared the safety code with them.
    pub verified: bool,
    /// Key messages never recorded again: offers the user ignored, a forgotten contact's.
    pub ignored: Vec<String>,
    /// Sealed messages seen in the DM, oldest message id per (epoch, nonce).
    pub seen: Vec<Seen>,
}

fn newest<'a>(list: impl Iterator<Item = &'a Announcement>) -> Option<&'a Announcement> {
    list.max_by_key(|a| (a.at, a.id.parse::<u64>().unwrap_or(0)))
}

impl Contact {
    fn new(peer: &str) -> Contact {
        Contact { peer: peer.to_owned(), mine: Vec::new(), theirs: Vec::new(), trusted: None, verified: false, ignored: Vec::new(), seen: Vec::new() }
    }

    /// Your newest announcement made with your current key.
    pub fn latest_mine(&self, my_key: &[u8; 32]) -> Option<&Announcement> {
        newest(self.mine.iter().filter(|a| &a.key == my_key))
    }

    pub fn latest_theirs(&self) -> Option<&Announcement> {
        newest(self.theirs.iter())
    }

    pub fn status(&self, my_key: &[u8; 32]) -> Status {
        let theirs = self.latest_theirs();
        if theirs.is_some_and(|t| self.trusted != Some(t.key)) {
            return Status::Changed;
        }
        match (self.latest_mine(my_key), theirs) {
            (Some(_), Some(_)) => Status::On,
            (None, None) => Status::Off,
            _ => Status::Waiting,
        }
    }

    /// Their newest announcement is an offer this side hasn't answered (and their key is trusted).
    pub fn offer_waiting(&self, my_key: &[u8; 32]) -> bool {
        let Some(t) = self.latest_theirs() else { return false };
        let order = |a: &Announcement| (a.at, a.id.parse::<u64>().unwrap_or(0));
        t.role == Role::Offer && self.trusted == Some(t.key) && self.latest_mine(my_key).is_none_or(|m| order(m) < order(t))
    }

    /// An offer to answer without asking: only from someone this side already had encryption on
    /// with (a renewal). A first offer waits for the user (`offer_waiting`).
    pub fn needs_answer(&self, my_key: &[u8; 32]) -> bool {
        self.offer_waiting(my_key) && self.latest_mine(my_key).is_some()
    }

    /// This side has announced to them: encryption was chosen here, so the DM is pinned.
    pub fn chosen(&self, my_key: &[u8; 32]) -> bool {
        self.trusted.is_some() && self.latest_mine(my_key).is_some()
    }

    fn chosen_any(&self) -> bool {
        !self.mine.is_empty()
    }
}

/// What a new announcement did.
#[derive(Debug, PartialEq)]
pub enum Event {
    Nothing,
    /// The first key seen from them: trusted, encryption can start.
    Trusted,
    /// A key of theirs that isn't the trusted one.
    Changed,
}

/// The contacts, with every epoch's key derived once.
#[derive(Default)]
pub struct Book {
    pub contacts: HashMap<String, Contact>,
    // (peer, epoch) -> (key, their key in that epoch)
    keys: HashMap<(String, u32), (Zeroizing<[u8; 32]>, [u8; 32])>,
    /// Loaded from a file made with the old 6-digit codes: the "compared" marks were cleared.
    pub code_upgraded: bool,
    /// Seen messages changed since the last save.
    pub seen_dirty: bool,
}

fn encode_ann(a: &Announcement) -> Value {
    json!({ "id": a.id, "key": STANDARD.encode(a.key), "nonce": STANDARD.encode(a.nonce), "role": if a.role == Role::Offer { "o" } else { "a" }, "at": a.at })
}

fn decode_ann(v: &Value) -> Option<Announcement> {
    Some(Announcement {
        id: v["id"].as_str().filter(|i| is_id(i))?.to_owned(),
        key: STANDARD.decode(v["key"].as_str()?).ok()?.try_into().ok()?,
        nonce: STANDARD.decode(v["nonce"].as_str()?).ok()?.try_into().ok()?,
        role: if v["role"] == "o" { Role::Offer } else { Role::Answer },
        at: v["at"].as_u64().unwrap_or(0),
    })
}

fn contacts_body(contacts: &HashMap<String, Contact>) -> Value {
    let mut peers: Vec<&Contact> = contacts.values().collect();
    peers.sort_by(|a, b| a.peer.cmp(&b.peer));
    json!(peers.iter().map(|c| json!({
        "peer": c.peer, "trusted": c.trusted.map(|k| STANDARD.encode(k)), "verified": c.verified,
        "mine": c.mine.iter().map(encode_ann).collect::<Vec<_>>(), "theirs": c.theirs.iter().map(encode_ann).collect::<Vec<_>>(),
        "ignored": c.ignored,
        "seen": c.seen.iter().map(|(e, n, id)| json!([e, STANDARD.encode(n), id])).collect::<Vec<_>>(),
    })).collect::<Vec<_>>())
}

fn decode_seen(v: &Value) -> Option<Seen> {
    let epoch = u32::try_from(v[0].as_u64()?).ok()?;
    let nonce = STANDARD.decode(v[1].as_str()?).ok()?.try_into().ok()?;
    Some((epoch, nonce, v[2].as_str().filter(|i| is_id(i))?.to_owned()))
}

fn mac_of(identity: &Identity, body: &Value) -> Hmac<Sha256> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(identity.mac_key().as_ref()).expect("any key length");
    m.update(body.to_string().as_bytes());
    m
}

// The pair in the order both sides use: the smaller user id's first.
fn ordered<T>(me: &str, mine: T, peer: &str, theirs: T) -> (T, T) {
    if me.parse::<u64>().unwrap_or(0) <= peer.parse::<u64>().unwrap_or(0) { (mine, theirs) } else { (theirs, mine) }
}

/// An epoch's id: a hash of the pair of announcements, the same on both sides.
pub fn epoch_id(me: &str, mine: &Announcement, peer: &str, theirs: &Announcement) -> u32 {
    let (a, b) = ordered(me, mine, peer, theirs);
    let digest = Sha256::new().chain_update(b"LockCord epoch v1").chain_update(a.key).chain_update(a.nonce).chain_update(b.key).chain_update(b.nonce).finalize();
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

pub fn derive_key(identity: &Identity, peer_key: &[u8; 32], my_nonce: &[u8; 32], their_nonce: &[u8; 32], me: &str, peer: &str) -> Option<Zeroizing<[u8; 32]>> {
    if me == peer {
        return None;
    }
    let shared = identity.secret.diffie_hellman(&PublicKey::from(*peer_key));
    if !shared.was_contributory() {
        return None;
    }
    let (lo_nonce, hi_nonce) = ordered(me, my_nonce, peer, their_nonce);
    let (lo, hi) = ordered(me, me, peer, peer);
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(lo_nonce);
    salt[32..].copy_from_slice(hi_nonce);
    let info = format!("{INFO}|{lo}|{hi}");
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    hk.expand(info.as_bytes(), key.as_mut()).ok()?;
    Some(key)
}

/// The code both people can compare: the two identity keys only, so it stays the same when the
/// keys are renewed. 60 digits in twelve groups of five (about 199 bits, like Signal's safety
/// numbers): far too many to match by generating keys. "12345 67890 …".
pub fn safety_code(mine: &[u8; 32], theirs: &[u8; 32]) -> String {
    let (a, b) = if mine <= theirs { (mine, theirs) } else { (theirs, mine) };
    let digest = Sha512::new().chain_update(b"LockCord safety code v2").chain_update(a).chain_update(b).finalize();
    let groups: Vec<String> = digest[..60]
        .chunks(5)
        .map(|c| format!("{:05}", c.iter().fold(0u64, |n, b| n << 8 | u64::from(*b)) % 100_000))
        .collect();
    groups.join(" ")
}

impl Book {
    /// From contacts.json: refused whole if its MAC doesn't match this identity.
    pub fn load(identity: &Identity, me: &str, raw: &Value) -> Result<Book, String> {
        let mut book = Book::default();
        if raw.is_null() {
            return Ok(book);
        }
        let body = &raw["contacts"];
        let expected = raw["mac"].as_str().and_then(|m| STANDARD.decode(m).ok()).unwrap_or_default();
        mac_of(identity, body).verify_slice(&expected).map_err(|_| "The encrypted-DM contacts file was changed outside LockCord, so it isn't trusted.".to_owned())?;
        for c in body.as_array().into_iter().flatten() {
            let Some(peer) = c["peer"].as_str().filter(|p| is_id(p)) else { continue };
            let list = |k: &str| c[k].as_array().into_iter().flatten().filter_map(decode_ann).take(MAX_ANNOUNCEMENTS).collect::<Vec<_>>();
            let trusted = c["trusted"].as_str().and_then(|k| STANDARD.decode(k).ok()).and_then(|b| b.try_into().ok());
            let ignored = c["ignored"].as_array().into_iter().flatten().filter_map(|i| i.as_str().filter(|i| is_id(i))).take(MAX_IGNORED).map(str::to_owned).collect();
            let seen = c["seen"].as_array().into_iter().flatten().filter_map(decode_seen).take(MAX_SEEN).collect();
            let contact = Contact { peer: peer.to_owned(), mine: list("mine"), theirs: list("theirs"), trusted, verified: c["verified"] == true, ignored, seen };
            book.contacts.insert(peer.to_owned(), contact);
        }
        // Compared with the old 6-digit code: that comparison could have been faked, so it's
        // asked again. (The marker is outside the MAC: removing it only clears marks.)
        if raw["code"].as_u64() != Some(CODE_VERSION) {
            for c in book.contacts.values_mut().filter(|c| c.verified) {
                c.verified = false;
                book.code_upgraded = true;
            }
        }
        let peers: Vec<String> = book.contacts.keys().cloned().collect();
        for peer in peers {
            book.derive(identity, me, &peer);
        }
        Ok(book)
    }

    pub fn to_json(&self, identity: &Identity) -> Value {
        let body = contacts_body(&self.contacts);
        json!({ "version": 2, "code": CODE_VERSION, "mac": STANDARD.encode(mac_of(identity, &body).finalize().into_bytes()), "contacts": body })
    }

    // Every pair of announcements made with this identity's key is an epoch.
    fn derive(&mut self, identity: &Identity, me: &str, peer: &str) {
        self.keys.retain(|(p, _), _| p != peer);
        let Some(c) = self.contacts.get(peer) else { return };
        for m in c.mine.iter().filter(|m| &m.key == identity.public.as_bytes()) {
            for t in &c.theirs {
                if let Some(key) = derive_key(identity, &t.key, &m.nonce, &t.nonce, me, peer) {
                    self.keys.insert((peer.to_owned(), epoch_id(me, m, peer, t)), (key, t.key));
                }
            }
        }
    }

    /// A key message seen in the DM with `peer` (yours if `from_me`).
    pub fn record(&mut self, identity: &Identity, me: &str, peer: &str, ann: Announcement, from_me: bool) -> Event {
        let contact = self.contacts.entry(peer.to_owned()).or_insert_with(|| Contact::new(peer));
        let list = if from_me { &mut contact.mine } else { &mut contact.theirs };
        if list.iter().any(|a| a.id == ann.id) || contact.ignored.contains(&ann.id) {
            return Event::Nothing;
        }
        let event = if from_me {
            Event::Nothing
        } else {
            match contact.trusted {
                None => {
                    contact.trusted = Some(ann.key);
                    Event::Trusted
                }
                Some(k) if k != ann.key => Event::Changed,
                _ => Event::Nothing,
            }
        };
        let list = if from_me { &mut contact.mine } else { &mut contact.theirs };
        list.push(ann);
        if list.len() > MAX_ANNOUNCEMENTS {
            list.sort_by_key(|a| a.at);
            list.remove(0);
        }
        self.derive(identity, me, peer);
        event
    }

    /// The user accepts their new key (after comparing the safety code, ideally).
    pub fn accept_change(&mut self, peer: &str) -> bool {
        let Some(c) = self.contacts.get_mut(peer) else { return false };
        let Some(k) = c.latest_theirs().map(|t| t.key) else { return false };
        c.trusted = Some(k);
        c.verified = false;
        true
    }

    pub fn set_verified(&mut self, peer: &str, verified: bool) {
        if let Some(c) = self.contacts.get_mut(peer) {
            c.verified = verified;
        }
    }

    /// Their waiting offer, left unanswered for good.
    pub fn ignore_offer(&mut self, peer: &str) -> bool {
        let Some(c) = self.contacts.get_mut(peer) else { return false };
        if c.latest_theirs().is_none_or(|t| t.role != Role::Offer) || c.chosen_any() {
            return false;
        }
        // Never chosen here: none of their key messages count, and their key isn't kept as
        // trusted (a later offer with another key is a first offer, not a "changed key").
        for id in c.theirs.drain(..).map(|t| t.id).collect::<Vec<_>>() {
            push_ignored(c, id);
        }
        c.trusted = None;
        true
    }

    /// Keys and announcements go; the key messages already in the DM are remembered as ignored,
    /// so loading its history doesn't turn encryption on again.
    pub fn forget(&mut self, peer: &str) {
        if let Some(c) = self.contacts.get_mut(peer) {
            let ids: Vec<String> = c.mine.iter().chain(&c.theirs).map(|a| a.id.clone()).collect();
            let ignored = std::mem::take(&mut c.ignored);
            *c = Contact::new(peer);
            c.ignored = ignored;
            for id in ids {
                push_ignored(c, id);
            }
        }
        self.keys.retain(|(p, _), _| p != peer);
    }

    /// After a message opened: true if another Discord message carried the same ciphertext
    /// first (an older id), so this one is a copy posted again. Edits keep their id, so they pass.
    pub fn replayed(&mut self, peer: &str, content: &str, message_id: &str) -> bool {
        let Some((epoch, data)) = parse(content) else { return false };
        let Some(nonce) = URL_SAFE_NO_PAD.decode(data).ok().and_then(|b| b.get(..12).and_then(|n| <[u8; 12]>::try_from(n).ok())) else { return false };
        let (Some(c), true) = (self.contacts.get_mut(peer), is_id(message_id)) else { return false };
        let id = message_id.parse::<u64>().unwrap_or(0);
        match c.seen.iter_mut().find(|(e, n, _)| *e == epoch && *n == nonce) {
            Some((_, _, first)) => {
                let first_id = first.parse::<u64>().unwrap_or(0);
                if id < first_id {
                    // Seen out of order: this one is the original.
                    *first = message_id.to_owned();
                    self.seen_dirty = true;
                }
                id > first_id
            }
            None => {
                c.seen.push((epoch, nonce, message_id.to_owned()));
                if c.seen.len() > MAX_SEEN {
                    c.seen.remove(0);
                }
                self.seen_dirty = true;
                false
            }
        }
    }

    pub fn status(&self, identity: &Identity, peer: &str) -> Status {
        self.contacts.get(peer).map(|c| c.status(identity.public.as_bytes())).unwrap_or(Status::Off)
    }

    /// Encrypts with the newest pair of announcements; only when the status is On.
    pub fn seal(&self, identity: &Identity, peer: &str, channel: &str, me: &str, text: &str, now: u64) -> Result<String, String> {
        let c = self.contacts.get(peer).ok_or("Encryption isn't on with them.")?;
        match c.status(identity.public.as_bytes()) {
            Status::On => {}
            Status::Changed => return Err("Their key changed. Check it in the DM's header before anything more is sent.".into()),
            _ => return Err("Encryption isn't on with them yet.".into()),
        }
        let (Some(m), Some(t)) = (c.latest_mine(identity.public.as_bytes()), c.latest_theirs()) else { return Err("Encryption isn't on with them yet.".into()) };
        let epoch = epoch_id(me, m, peer, t);
        let (key, _) = self.keys.get(&(peer.to_owned(), epoch)).ok_or("That contact's key is missing.")?;
        seal(key, epoch, channel, me, text, now)
    }

    /// Decrypts an LC1 message in the DM with `peer`, sent by `sender` (you or them).
    pub fn open(&self, peer: &str, channel: &str, sender: &str, content: &str, posted_at: u64) -> Result<Opened, E2eError> {
        let (epoch, _) = parse(content).ok_or(E2eError::NotOurs)?;
        let (key, their_key) = self.keys.get(&(peer.to_owned(), epoch)).ok_or(E2eError::NoKey)?;
        if self.contacts.get(peer).and_then(|c| c.trusted) != Some(*their_key) {
            return Err(E2eError::Untrusted);
        }
        open(key, channel, sender, content, posted_at)
    }

    /// For the UI: never any key material.
    pub fn view(&self, identity: &Identity, peer: &str) -> Option<Value> {
        let c = self.contacts.get(peer)?;
        let my_key = identity.public.as_bytes();
        let status = match c.status(my_key) {
            Status::Off => "off",
            // Only they announced: an offer waiting for the user, or one already left alone.
            Status::Waiting if c.latest_mine(my_key).is_none() => if c.offer_waiting(my_key) { "offered" } else { "off" },
            Status::Waiting => "waiting",
            Status::On => "on",
            Status::Changed => "changed",
        };
        let theirs = c.trusted.or(c.latest_theirs().map(|t| t.key));
        let since = c.mine.iter().chain(&c.theirs).map(|a| a.at).min().unwrap_or(0);
        Some(json!({
            "peer": peer, "status": status, "verified": c.verified, "since": since,
            "safety": theirs.map(|k| safety_code(identity.public.as_bytes(), &k)),
            "newSafety": c.latest_theirs().filter(|t| Some(t.key) != c.trusted).map(|t| safety_code(identity.public.as_bytes(), &t.key)),
        }))
    }
}

fn push_ignored(c: &mut Contact, id: String) {
    if !c.ignored.contains(&id) {
        c.ignored.push(id);
        if c.ignored.len() > MAX_IGNORED {
            c.ignored.remove(0);
        }
    }
}

// --- Messages -------------------------------------------------------------------------------------

fn aad(channel: &str, sender: &str, epoch: u32) -> String {
    format!("LC1|{channel}|{sender}|{epoch:08x}")
}

fn frame(text: &str, now: u64) -> Result<Zeroizing<Vec<u8>>, String> {
    let bytes = text.as_bytes();
    if bytes.len() > MAX_TEXT_BYTES {
        return Err(format!("Encrypted messages can be up to about {MAX_TEXT_BYTES} characters (fewer with emoji); split it up."));
    }
    let size = BUCKETS.iter().copied().find(|b| *b >= HEADER + bytes.len()).unwrap_or(BUCKETS[BUCKETS.len() - 1]);
    let mut out = Zeroizing::new(vec![0u8; size]);
    out[0] = FRAME_VERSION;
    out[1..9].copy_from_slice(&now.to_be_bytes());
    out[9..13].copy_from_slice(&(bytes.len() as u32).to_be_bytes());
    out[HEADER..HEADER + bytes.len()].copy_from_slice(bytes);
    Ok(out)
}

fn unframe(bytes: &[u8]) -> Option<(u64, String)> {
    if bytes.len() < HEADER || bytes[0] != FRAME_VERSION || !BUCKETS.contains(&bytes.len()) {
        return None;
    }
    let at = u64::from_be_bytes(bytes[1..9].try_into().ok()?);
    let len = u32::from_be_bytes(bytes[9..13].try_into().ok()?) as usize;
    let text = bytes.get(HEADER..HEADER + len)?;
    // The padding is zeros; anything else is a broken frame.
    if bytes[HEADER + len..].iter().any(|b| *b != 0) {
        return None;
    }
    Some((at, String::from_utf8(text.to_vec()).ok()?))
}

/// "LC1:<epoch, 8 hex>:<data>" split up, or None if it isn't one.
pub fn parse(content: &str) -> Option<(u32, &str)> {
    let rest = content.strip_prefix(PREFIX)?;
    let (epoch, data) = rest.split_once(':')?;
    if epoch.len() != 8 || data.is_empty() || data.len() > 2000 {
        return None;
    }
    Some((u32::from_str_radix(epoch, 16).ok()?, data))
}

pub fn is_lc1(content: &str) -> bool {
    parse(content).is_some()
}

pub fn seal(key: &[u8; 32], epoch: u32, channel: &str, sender: &str, text: &str, now: u64) -> Result<String, String> {
    let plain = frame(text, now)?;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let associated = aad(channel, sender, epoch);
    let sealed = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: &plain, aad: associated.as_bytes() }).map_err(|_| "Encryption failed.".to_owned())?;
    let mut data = Vec::with_capacity(12 + sealed.len());
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&sealed);
    Ok(format!("{PREFIX}{epoch:08x}:{}", URL_SAFE_NO_PAD.encode(data)))
}

pub fn open(key: &[u8; 32], channel: &str, sender: &str, content: &str, posted_at: u64) -> Result<Opened, E2eError> {
    let (epoch, data) = parse(content).ok_or(E2eError::NotOurs)?;
    let bytes = URL_SAFE_NO_PAD.decode(data).map_err(|_| E2eError::Broken)?;
    if bytes.len() < 12 + 16 {
        return Err(E2eError::Broken);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| E2eError::Broken)?;
    let associated = aad(channel, sender, epoch);
    let plain = Zeroizing::new(cipher.decrypt(Nonce::from_slice(&bytes[..12]), Payload { msg: &bytes[12..], aad: associated.as_bytes() }).map_err(|_| E2eError::Broken)?);
    let (sent_at, text) = unframe(&plain).ok_or(E2eError::Broken)?;
    Ok(if sent_at.abs_diff(posted_at) > CLOCK_SLACK_MS { Opened::Stale(text) } else { Opened::Text(text) })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(id: &str, who: &Identity, nonce: u8, role: Role, at: u64) -> Announcement {
        Announcement { id: id.into(), key: *who.public.as_bytes(), nonce: [nonce; 32], role, at }
    }

    // Alice (100) offers, Bob (200) answers: both books see both announcements.
    fn established() -> (Identity, Identity, Book, Book) {
        let alice = Identity::from_bytes([1u8; 32]);
        let bob = Identity::from_bytes([2u8; 32]);
        let (mut a, mut b) = (Book::default(), Book::default());
        let offer = ann("10", &alice, 7, Role::Offer, 1);
        let answer = ann("11", &bob, 9, Role::Answer, 2);
        assert_eq!(a.record(&alice, "100", "200", offer.clone(), true), Event::Nothing);
        assert_eq!(b.record(&bob, "200", "100", offer, false), Event::Trusted);
        assert!(b.contacts["100"].offer_waiting(bob.public.as_bytes()), "a first offer waits for Bob");
        assert!(!b.contacts["100"].needs_answer(bob.public.as_bytes()), "and is never answered by itself");
        assert_eq!(b.view(&bob, "100").unwrap()["status"], "offered");
        b.record(&bob, "200", "100", answer.clone(), true);
        assert!(!b.contacts["100"].offer_waiting(bob.public.as_bytes()));
        assert_eq!(a.record(&alice, "100", "200", answer, false), Event::Trusted);
        (alice, bob, a, b)
    }

    #[test]
    fn key_messages_round_trip_and_nothing_else_parses() {
        let alice = Identity::from_bytes([1u8; 32]);
        let text = key_message(&alice, &[7u8; 32], Role::Offer);
        assert!(text.starts_with("🔑 LockCord key · LCK1:o:"));
        assert_eq!(parse_key_message(&text), Some((Role::Offer, *alice.public.as_bytes(), [7u8; 32])));
        assert_eq!(parse_key_message(&text.replace("LCK1:o:", "LCK1:x:")), None);
        assert_eq!(parse_key_message(&(text.clone() + ":extra")), None);
        assert_eq!(parse_key_message("LCK1:o:abc:def"), None);
    }

    #[test]
    fn both_sides_agree_and_read_each_other() {
        let (alice, bob, a, b) = established();
        assert_eq!(a.status(&alice, "200"), Status::On);
        assert_eq!(b.status(&bob, "100"), Status::On);
        let now = 1_700_000_000_000;
        let sealed = a.seal(&alice, "200", "555", "100", "hei Bob 👋", now).unwrap();
        assert!(sealed.starts_with("LC1:") && !sealed.contains("hei"));
        assert_eq!(b.open("100", "555", "100", &sealed, now + 500), Ok(Opened::Text("hei Bob 👋".into())));
        assert_eq!(a.open("200", "555", "100", &sealed, now), Ok(Opened::Text("hei Bob 👋".into())), "your own, read back");
        let reply = b.seal(&bob, "100", "555", "200", "moi", now).unwrap();
        assert_eq!(a.open("200", "555", "200", &reply, now), Ok(Opened::Text("moi".into())));
        assert_eq!(a.view(&alice, "200").unwrap()["safety"], b.view(&bob, "100").unwrap()["safety"]);
    }

    #[test]
    fn offering_at_once_still_meets_and_renewing_keeps_history() {
        let (alice, bob, mut a, mut b) = established();
        let old = a.seal(&alice, "200", "555", "100", "old", 5).unwrap();
        // Both renew at once: each sees the other's offer.
        let ao = ann("20", &alice, 3, Role::Offer, 10);
        let bo = ann("21", &bob, 4, Role::Offer, 10);
        a.record(&alice, "100", "200", ao.clone(), true);
        a.record(&alice, "100", "200", bo.clone(), false);
        b.record(&bob, "200", "100", bo, true);
        b.record(&bob, "200", "100", ao, false);
        let new = a.seal(&alice, "200", "555", "100", "new", 11).unwrap();
        assert_eq!(b.open("100", "555", "100", &new, 11), Ok(Opened::Text("new".into())), "the same newest pair on both sides");
        assert_eq!(b.open("100", "555", "100", &old, 5), Ok(Opened::Text("old".into())), "old epochs still read");
        let renewal = ann("22", &bob, 6, Role::Offer, 12);
        a.record(&alice, "100", "200", renewal, false);
        assert!(a.contacts["200"].needs_answer(alice.public.as_bytes()), "a renewal is answered by itself");
    }

    #[test]
    fn a_changed_key_stops_sending_and_reading_until_accepted() {
        let (alice, _, mut a, _) = established();
        let mallory = Identity::from_bytes([9u8; 32]);
        assert_eq!(a.record(&alice, "100", "200", ann("30", &mallory, 5, Role::Offer, 20), false), Event::Changed);
        assert_eq!(a.status(&alice, "200"), Status::Changed);
        assert!(a.seal(&alice, "200", "555", "100", "hi", 21).is_err(), "nothing goes out");
        assert!(!a.contacts["200"].needs_answer(alice.public.as_bytes()), "no automatic answer to a changed key");
        // Mallory, who swapped the key, writes as Bob: not shown.
        let mut m = Book::default();
        let alice_answer = ann("32", &alice, 7, Role::Answer, 22);
        m.record(&mallory, "200", "100", ann("30", &mallory, 5, Role::Offer, 20), true);
        m.record(&mallory, "200", "100", alice_answer.clone(), false);
        let forged = m.seal(&mallory, "100", "555", "200", "send me your password", 22).unwrap();
        a.record(&alice, "100", "200", alice_answer, true);
        assert_eq!(a.open("200", "555", "200", &forged, 22), Err(E2eError::Untrusted));
        assert!(a.view(&alice, "200").unwrap()["newSafety"].is_string());
        assert!(a.accept_change("200"));
        assert_eq!(a.status(&alice, "200"), Status::On);
    }

    #[test]
    fn tampering_replay_and_impersonation_fail() {
        let (alice, _, a, b) = established();
        let now = 1_700_000_000_000;
        let sealed = a.seal(&alice, "200", "555", "100", "secret", now).unwrap();
        assert_eq!(b.open("100", "556", "100", &sealed, now), Err(E2eError::Broken), "another DM");
        assert_eq!(b.open("100", "555", "200", &sealed, now), Err(E2eError::Broken), "claimed to be from Bob");
        let mut chars: Vec<char> = sealed.chars().collect();
        let i = chars.len() - 5;
        chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
        assert_eq!(b.open("100", "555", "100", &chars.into_iter().collect::<String>(), now), Err(E2eError::Broken));
        assert_eq!(b.open("100", "555", "100", &sealed, now + 3_600_000), Ok(Opened::Stale("secret".into())), "posted again later");
        assert_eq!(b.open("100", "555", "100", &format!("LC1:deadbeef:{}", &sealed[13..]), now), Err(E2eError::NoKey));
        assert_eq!(b.open("100", "555", "100", "hello", now), Err(E2eError::NotOurs));
    }

    #[test]
    fn a_ciphertext_posted_again_is_flagged_but_an_edit_is_not() {
        let (alice, _, a, mut b) = established();
        let sealed = a.seal(&alice, "200", "555", "100", "pay me", 50).unwrap();
        assert!(!b.replayed("100", &sealed, "900"));
        assert!(!b.replayed("100", &sealed, "900"), "the same message again (an edit, a reload)");
        assert!(b.replayed("100", &sealed, "901"), "the same ciphertext under another message");
        assert!(!b.replayed("100", &sealed, "899"), "an older id is the original");
        assert!(b.replayed("100", &sealed, "900"));
        let saved = b.to_json(&Identity::from_bytes([2u8; 32]));
        let mut back = Book::load(&Identity::from_bytes([2u8; 32]), "200", &saved).unwrap();
        assert!(back.replayed("100", &sealed, "901"), "remembered across restarts");
    }

    #[test]
    fn the_safety_code_is_sixty_digits_and_the_same_both_ways() {
        let (k1, k2, k3) = ([1u8; 32], [2u8; 32], [3u8; 32]);
        let code = safety_code(&k1, &k2);
        let groups: Vec<&str> = code.split(' ').collect();
        assert_eq!(groups.len(), 12);
        assert!(groups.iter().all(|g| g.len() == 5 && g.bytes().all(|b| b.is_ascii_digit())));
        assert_eq!(code, safety_code(&k2, &k1));
        assert_ne!(code, safety_code(&k1, &k3));
    }

    #[test]
    fn an_ignored_offer_and_a_forgotten_contact_stay_off() {
        let alice = Identity::from_bytes([1u8; 32]);
        let bob = Identity::from_bytes([2u8; 32]);
        let mut b = Book::default();
        let offer = ann("10", &alice, 7, Role::Offer, 1);
        b.record(&bob, "200", "100", offer.clone(), false);
        assert!(b.ignore_offer("100"));
        assert_eq!(b.view(&bob, "100").unwrap()["status"], "off");
        assert_eq!(b.record(&bob, "200", "100", offer, false), Event::Nothing, "history loaded again");
        assert_eq!(b.view(&bob, "100").unwrap()["status"], "off");
        let other = Identity::from_bytes([4u8; 32]);
        assert_eq!(b.record(&bob, "200", "100", ann("12", &other, 7, Role::Offer, 3), false), Event::Trusted, "a later offer is a first offer, not a changed key");

        let (alice, _, mut a, _) = established();
        a.forget("200");
        a.record(&alice, "100", "200", ann("10", &alice, 7, Role::Offer, 1), true);
        a.record(&alice, "100", "200", ann("11", &Identity::from_bytes([2u8; 32]), 9, Role::Answer, 2), false);
        assert_eq!(a.status(&alice, "200"), Status::Off, "the old key messages don't turn it on again");
    }

    #[test]
    fn old_comparisons_are_asked_again_once() {
        let (alice, _, mut a, _) = established();
        a.set_verified("200", true);
        let mut old = a.to_json(&alice);
        old.as_object_mut().unwrap().remove("code");
        let loaded = Book::load(&alice, "100", &old).unwrap();
        assert!(loaded.code_upgraded && !loaded.contacts["200"].verified);
        let loaded = Book::load(&alice, "100", &a.to_json(&alice)).unwrap();
        assert!(!loaded.code_upgraded && loaded.contacts["200"].verified);
    }

    #[test]
    fn padding_hides_lengths_and_long_text_is_refused() {
        let (alice, _, a, _) = established();
        let short = a.seal(&alice, "200", "555", "100", "hi", 0).unwrap();
        let longer = a.seal(&alice, "200", "555", "100", "hello there, how are you", 0).unwrap();
        assert_eq!(short.len(), longer.len());
        let max = a.seal(&alice, "200", "555", "100", &"x".repeat(MAX_TEXT_BYTES), 0).unwrap();
        assert!(max.len() < 2000, "fits Discord: {}", max.len());
        assert!(a.seal(&alice, "200", "555", "100", &"x".repeat(MAX_TEXT_BYTES + 1), 0).is_err());
    }

    #[test]
    fn waiting_seals_nothing_and_bad_keys_derive_nothing() {
        let alice = Identity::from_bytes([1u8; 32]);
        let mut a = Book::default();
        a.record(&alice, "100", "200", ann("10", &alice, 7, Role::Offer, 1), true);
        assert_eq!(a.status(&alice, "200"), Status::Waiting);
        assert!(a.seal(&alice, "200", "555", "100", "hi", 2).is_err());
        assert!(derive_key(&alice, &[0u8; 32], &[0u8; 32], &[0u8; 32], "100", "200").is_none(), "all-zero shared secret");
        assert!(derive_key(&alice, alice.public.as_bytes(), &[0u8; 32], &[0u8; 32], "100", "100").is_none(), "yourself");
    }

    #[test]
    fn the_contacts_file_round_trips_and_tampering_is_caught() {
        let (alice, _, a, _) = established();
        let saved = a.to_json(&alice);
        let loaded = Book::load(&alice, "100", &saved).unwrap();
        assert_eq!(loaded.contacts, a.contacts);
        assert!(loaded.seal(&alice, "200", "555", "100", "x", 0).is_ok(), "keys derived again on load");
        let mut changed = saved.clone();
        changed["contacts"][0]["trusted"] = json!(STANDARD.encode([5u8; 32]));
        assert!(Book::load(&alice, "100", &changed).is_err());
        assert!(Book::load(&Identity::from_bytes([3u8; 32]), "100", &saved).is_err(), "another profile's file");
    }
}
