//! Encrypted DMs between LockCord users: the everyday DM, end-to-end encrypted. The messages stay
//! in the DM on Discord as ciphertext, noise to everyone but the two LockCords that hold the keys,
//! and are readable there later (unlike a secret chat, which is live and peer to peer).
//!
//!   identity    one X25519 key pair per profile; the secret lives only in Windows Credential
//!               Manager (or the macOS Keychain), on this machine (vault.rs, like the session), and
//!               never reaches the window.
//!   key message LockCords swap public keys through the DM itself. v2 (LCK2): "🔑 LockCord key ·
//!               LCK2:<o|a>:<identity>:<one-time key>:<nonce>"; v1 (LCK1) has no one-time key. Your
//!               click posts an offer (o); the other side answers (a) only after its user accepts,
//!               so a fake offer learns nothing. A renewal from someone encryption is already on
//!               with is answered by itself. Someone on Discord's own app only ever sees a key
//!               message after you chose to turn encryption on.
//!   epochs      every pair of announcements (one of yours, one of theirs) is an epoch, id = a hash
//!               of the pair, so both sides always agree on it, even when both offered at once.
//!               v2: key = HKDF-SHA256(DH(IK,IK) ‖ DH(EK,EK) ‖ DH(IKlo,EKhi) ‖ DH(EKlo,IKhi), salt =
//!               both nonces, info = version + both user ids). Each announcement of ours has a
//!               fresh one-time key (EK) whose secret is deleted a while after the epoch is
//!               replaced (e2e_store.rs); after that, not even both identity keys can read that
//!               epoch again. v1 epochs (identities only, no forward secrecy) stay readable, and
//!               are used only while the other side's newest key message is still v1.
//!   trust       the first identity key seen from someone is trusted (as Signal does). If it ever
//!               changes, sending there stops and messages under the new key stay unread until the
//!               user accepts it: whoever swapped a key can neither read nor impersonate. A one-time
//!               key swapped under the trusted identity gets nowhere either: DH(IK,IK) needs an
//!               identity secret. The safety code depends only on the two identity keys, so it's
//!               compared once. It is 60 digits: nothing commits a side to its key first, so a short
//!               code could be matched by someone generating keys in between.
//!   message     "LC2:<epoch>:<base64url(nonce ‖ AES-256-GCM)>" (v1: "LC1:"). The authenticated data
//!               binds the version, the channel, the sender and the epoch; the frame carries the
//!               send time (an old message posted again is flagged, and so is a second Discord
//!               message with the same ciphertext), in v2 the attached files' keys, and is padded to
//!               a size bucket (lengths don't leak).
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
pub const PREFIX2: &str = "LC2:";
const KEY_LEAD: &str = "🔑 LockCord key · ";
const KEY_PREFIX: &str = "LCK1:";
const KEY_PREFIX2: &str = "LCK2:";
const INFO: &str = "LockCord DM v1";
const INFO2: &str = "LockCord DM v2";
const FRAME_VERSION: u8 = 1;
const FRAME_VERSION2: u8 = 2;
// v1: [version][u64 sent-at ms][u32 length][text][zero padding]
const HEADER: usize = 1 + 8 + 4;
// The largest bucket keeps the whole message under Discord's 2,000 characters after base64.
const BUCKETS: [usize; 5] = [64, 128, 256, 512, 1024];
pub const MAX_TEXT_BYTES: usize = 1024 - HEADER;
// v2: [version][u64 sent-at ms][u16 length][text][u8 file count][files: 16-byte id, 32-byte key][zero padding]
const HEADER2: usize = 1 + 8 + 2 + 1;
// 12 + 1408 + 16 bytes are 1915 characters of base64, 1928 with "LC2:xxxxxxxx:".
const BUCKETS2: [usize; 6] = [64, 128, 256, 512, 1024, 1408];
pub const MAX_TEXT_BYTES2: usize = 1408 - HEADER2;
pub const MAX_FILES: usize = 10;
const FILE_ENTRY: usize = 16 + 32;
/// How far a message's send time may be from when Discord says it was posted.
const CLOCK_SLACK_MS: u64 = 10 * 60 * 1000;
const MAX_ANNOUNCEMENTS: usize = 32;
// Key messages left alone (ignored offers, a forgotten contact's) and messages seen, per contact.
const MAX_IGNORED: usize = 128;
const MAX_SEEN: usize = 512;
// contacts.json's safety-code version: comparisons made with the old 6-digit code don't count.
const CODE_VERSION: u64 = 2;
/// An epoch with this many sealed messages seen is renewed whatever its age (under MAX_SEEN, so
/// they're all still counted).
pub const RENEW_AFTER: usize = 500;

/// An encrypted file's key, carried in a v2 message's frame (efile.rs encrypts the file itself).
/// `id` is the random name the file was uploaded under, so the key matches only that file.
#[derive(Clone, PartialEq)]
pub struct FileRef {
    pub id: [u8; 16],
    pub key: Zeroizing<[u8; 32]>,
}

impl std::fmt::Debug for FileRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never a key in a log.
        write!(f, "FileRef({})", hex16(&self.id))
    }
}

pub fn hex16(id: &[u8; 16]) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

/// A sealed message, opened.
#[derive(Debug, PartialEq)]
pub struct Opened {
    pub text: String,
    pub files: Vec<FileRef>,
    /// Its send time is far from when it was posted: an old message posted again.
    pub stale: bool,
    pub version: u8,
    pub epoch: u32,
    pub sent_at: u64,
}

/// A sealed message's identity: its epoch and GCM nonce. The same pair under another Discord
/// message is a copy posted again.
type Seen = (u32, [u8; 12], String);

#[derive(Debug, PartialEq)]
pub enum E2eError {
    NotOurs,
    NoKey,
    /// A v2 epoch this side knew, whose one-time secret was deleted: it can't be read any more.
    Expired,
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

// --- One-time keys ------------------------------------------------------------------------------

/// The secret half of one of this side's one-time keys (one per v2 announcement of ours), with
/// when it was made and when it stopped being part of the current epoch.
pub struct Ephemeral {
    pub secret: Zeroizing<[u8; 32]>,
    pub made: u64,
    pub retired: Option<u64>,
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
    /// The one-time key (v2); None for a v1 announcement.
    pub ephemeral: Option<[u8; 32]>,
    pub nonce: [u8; 32],
    pub role: Role,
    pub at: u64,
}

impl Announcement {
    pub fn is_v2(&self) -> bool {
        self.ephemeral.is_some()
    }
}

/// The message LockCord posts in a DM to share this side's key: v2 with a one-time key, v1
/// without (only to answer an older LockCord's v1 offer).
pub fn key_message(identity: &Identity, ephemeral: Option<&[u8; 32]>, nonce: &[u8; 32], role: Role) -> String {
    let r = if role == Role::Offer { "o" } else { "a" };
    let id = URL_SAFE_NO_PAD.encode(identity.public.as_bytes());
    match ephemeral {
        Some(ek) => format!("{KEY_LEAD}{KEY_PREFIX2}{r}:{id}:{}:{}", URL_SAFE_NO_PAD.encode(ek), URL_SAFE_NO_PAD.encode(nonce)),
        None => format!("{KEY_LEAD}{KEY_PREFIX}{r}:{id}:{}", URL_SAFE_NO_PAD.encode(nonce)),
    }
}

/// A key message, read back: exactly LockCord's own shape, nothing more. (role, identity key,
/// one-time key if v2, nonce)
pub fn parse_key_message(content: &str) -> Option<(Role, [u8; 32], Option<[u8; 32]>, [u8; 32])> {
    let rest = content.strip_prefix(KEY_LEAD)?;
    let (v2, rest) = match rest.strip_prefix(KEY_PREFIX2) {
        Some(r) => (true, r),
        None => (false, rest.strip_prefix(KEY_PREFIX)?),
    };
    let mut parts = rest.split(':');
    let role = match parts.next()? {
        "o" => Role::Offer,
        "a" => Role::Answer,
        _ => return None,
    };
    let b32 = |s: &str| -> Option<[u8; 32]> { URL_SAFE_NO_PAD.decode(s).ok()?.try_into().ok() };
    let key = b32(parts.next()?)?;
    let ephemeral = if v2 { Some(b32(parts.next()?)?) } else { None };
    let nonce = b32(parts.next()?)?;
    if parts.next().is_some() {
        return None;
    }
    Some((role, key, ephemeral, nonce))
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
    /// Keep an encrypted copy of this DM's opened messages on this device (history.rs): asked
    /// per DM, None until the user answered.
    pub keep_copy: Option<bool>,
}

fn order(a: &Announcement) -> (u64, u64) {
    (a.at, a.id.parse::<u64>().unwrap_or(0))
}

fn newest<'a>(list: impl Iterator<Item = &'a Announcement>) -> Option<&'a Announcement> {
    list.max_by_key(|a| order(a))
}

impl Contact {
    fn new(peer: &str) -> Contact {
        Contact { peer: peer.to_owned(), mine: Vec::new(), theirs: Vec::new(), trusted: None, verified: false, ignored: Vec::new(), seen: Vec::new(), keep_copy: None }
    }

    /// Your newest announcement made with your current key.
    pub fn latest_mine(&self, my_key: &[u8; 32]) -> Option<&Announcement> {
        newest(self.mine.iter().filter(|a| &a.key == my_key))
    }

    pub fn latest_theirs(&self) -> Option<&Announcement> {
        newest(self.theirs.iter())
    }

    /// The pair messages are sealed with: the newest two when both are v2; otherwise the newest
    /// v1 pair (one side's LockCord is older), with their trusted key. The same on both sides.
    pub fn current_pair(&self, my_key: &[u8; 32]) -> Option<(&Announcement, &Announcement)> {
        let (m, t) = (self.latest_mine(my_key)?, self.latest_theirs()?);
        if m.is_v2() && t.is_v2() {
            return Some((m, t));
        }
        let m1 = newest(self.mine.iter().filter(|a| &a.key == my_key && !a.is_v2()))?;
        let t1 = newest(self.theirs.iter().filter(|a| !a.is_v2() && self.trusted == Some(a.key)))?;
        Some((m1, t1))
    }

    pub fn status(&self, my_key: &[u8; 32]) -> Status {
        let theirs = self.latest_theirs();
        if theirs.is_some_and(|t| self.trusted != Some(t.key)) {
            return Status::Changed;
        }
        match (self.latest_mine(my_key), theirs) {
            (Some(_), Some(_)) if self.current_pair(my_key).is_some() => Status::On,
            (None, None) => Status::Off,
            _ => Status::Waiting,
        }
    }

    /// Their newest announcement is an offer this side hasn't answered (and their key is trusted).
    pub fn offer_waiting(&self, my_key: &[u8; 32]) -> bool {
        let Some(t) = self.latest_theirs() else { return false };
        t.role == Role::Offer && self.trusted == Some(t.key) && self.latest_mine(my_key).is_none_or(|m| order(m) < order(t))
    }

    /// An offer to answer without asking: only from someone this side already had encryption on
    /// with (a renewal). A first offer waits for the user (`offer_waiting`).
    pub fn needs_answer(&self, my_key: &[u8; 32]) -> bool {
        self.offer_waiting(my_key) && self.latest_mine(my_key).is_some()
    }

    /// An answer goes out in their offer's version: an older LockCord can't read a v2 key message.
    pub fn answer_v2(&self) -> bool {
        self.latest_theirs().is_none_or(Announcement::is_v2)
    }

    /// This side announced v2, but their newest is still v1: their LockCord is older.
    pub fn peer_older(&self, my_key: &[u8; 32]) -> bool {
        match (self.latest_mine(my_key), self.latest_theirs()) {
            (Some(m), Some(t)) => m.is_v2() && !t.is_v2() && order(m) > order(t),
            _ => false,
        }
    }

    /// This side has announced to them: encryption was chosen here, so the DM is pinned.
    pub fn chosen(&self, my_key: &[u8; 32]) -> bool {
        self.trusted.is_some() && self.latest_mine(my_key).is_some()
    }

    fn chosen_any(&self) -> bool {
        !self.mine.is_empty()
    }

    /// When both sides had first announced v2 (this side with its key, they with the trusted
    /// one): from then on both LockCords are new enough, and newer v1 key messages are refused
    /// (`Book::record`), so nothing can take the DM back to keys without one-time secrets.
    fn v2_since(&self, my_key: &[u8; 32]) -> Option<(u64, u64)> {
        let mine = self.mine.iter().filter(|a| a.is_v2() && &a.key == my_key).map(order).min()?;
        let theirs = self.theirs.iter().filter(|a| a.is_v2() && self.trusted == Some(a.key)).map(order).min()?;
        Some(mine.max(theirs))
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
    /// Not recorded: a v1 key message after v2 (a downgrade), or not a user id.
    Refused,
}

// (peer, version, epoch)
type EpochRef = (String, u8, u32);

/// The contacts, with every epoch's key derived once.
#[derive(Default)]
pub struct Book {
    pub contacts: HashMap<String, Contact>,
    // -> (key, their identity key in that epoch)
    keys: HashMap<EpochRef, (Zeroizing<[u8; 32]>, [u8; 32])>,
    /// Every v2 epoch of the announcements, readable or not: one missing from `keys` has expired.
    known_v2: HashSet<(String, u32)>,
    /// This side's one-time secrets by public key (kept on disk by e2e_store.rs).
    pub ephemerals: HashMap<[u8; 32], Ephemeral>,
    /// Loaded from a file made with the old 6-digit codes: the "compared" marks were cleared.
    pub code_upgraded: bool,
    /// Seen messages changed since the last save.
    pub seen_dirty: bool,
    /// One-time secrets changed since the last save.
    pub secrets_dirty: bool,
}

fn encode_ann(a: &Announcement) -> Value {
    let mut v = json!({ "id": a.id, "key": STANDARD.encode(a.key), "nonce": STANDARD.encode(a.nonce), "role": if a.role == Role::Offer { "o" } else { "a" }, "at": a.at });
    if let Some(ek) = a.ephemeral {
        v["eph"] = json!(STANDARD.encode(ek));
    }
    v
}

fn decode_ann(v: &Value) -> Option<Announcement> {
    let ephemeral = match v.get("eph") {
        None => None,
        Some(e) => Some(STANDARD.decode(e.as_str()?).ok()?.try_into().ok()?),
    };
    Some(Announcement {
        id: v["id"].as_str().filter(|i| is_id(i))?.to_owned(),
        key: STANDARD.decode(v["key"].as_str()?).ok()?.try_into().ok()?,
        ephemeral,
        nonce: STANDARD.decode(v["nonce"].as_str()?).ok()?.try_into().ok()?,
        role: if v["role"] == "o" { Role::Offer } else { Role::Answer },
        at: v["at"].as_u64().unwrap_or(0),
    })
}

fn contacts_body(contacts: &HashMap<String, Contact>) -> Value {
    let mut peers: Vec<&Contact> = contacts.values().collect();
    peers.sort_by(|a, b| a.peer.cmp(&b.peer));
    json!(peers.iter().map(|c| {
        let mut v = json!({
            "peer": c.peer, "trusted": c.trusted.map(|k| STANDARD.encode(k)), "verified": c.verified,
            "mine": c.mine.iter().map(encode_ann).collect::<Vec<_>>(), "theirs": c.theirs.iter().map(encode_ann).collect::<Vec<_>>(),
            "ignored": c.ignored,
            "seen": c.seen.iter().map(|(e, n, id)| json!([e, STANDARD.encode(n), id])).collect::<Vec<_>>(),
        });
        // Only when answered, so a v1.6 file's MAC still matches until something changes.
        if let Some(keep) = c.keep_copy {
            v["keepCopy"] = json!(keep);
        }
        v
    }).collect::<Vec<_>>())
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
    let mut h = Sha256::new();
    if a.is_v2() || b.is_v2() {
        h.update(b"LockCord epoch v2");
        for x in [a, b] {
            h.update(x.key);
            h.update(x.ephemeral.unwrap_or([0u8; 32]));
            h.update(x.nonce);
        }
    } else {
        h.update(b"LockCord epoch v1");
        for x in [a, b] {
            h.update(x.key);
            h.update(x.nonce);
        }
    }
    let digest = h.finalize();
    u32::from_be_bytes([digest[0], digest[1], digest[2], digest[3]])
}

fn salt_and_info(version: &str, me: &str, peer: &str, my_nonce: &[u8; 32], their_nonce: &[u8; 32]) -> ([u8; 64], String) {
    let (lo_nonce, hi_nonce) = ordered(me, my_nonce, peer, their_nonce);
    let (lo, hi) = ordered(me, me, peer, peer);
    let mut salt = [0u8; 64];
    salt[..32].copy_from_slice(lo_nonce);
    salt[32..].copy_from_slice(hi_nonce);
    (salt, format!("{version}|{lo}|{hi}"))
}

pub fn derive_key(identity: &Identity, peer_key: &[u8; 32], my_nonce: &[u8; 32], their_nonce: &[u8; 32], me: &str, peer: &str) -> Option<Zeroizing<[u8; 32]>> {
    if me == peer {
        return None;
    }
    let shared = identity.secret.diffie_hellman(&PublicKey::from(*peer_key));
    if !shared.was_contributory() {
        return None;
    }
    let (salt, info) = salt_and_info(INFO, me, peer, my_nonce, their_nonce);
    let hk = Hkdf::<Sha256>::new(Some(&salt), shared.as_bytes());
    let mut key = Zeroizing::new([0u8; 32]);
    hk.expand(info.as_bytes(), key.as_mut()).ok()?;
    Some(key)
}

/// A v2 epoch's key: the two identities, the two one-time keys and both crossings, in the
/// lo/hi order (by user id) both sides share.
#[allow(clippy::too_many_arguments)]
pub fn derive_key2(identity: &Identity, my_ephemeral: &[u8; 32], peer_key: &[u8; 32], peer_ephemeral: &[u8; 32], my_nonce: &[u8; 32], their_nonce: &[u8; 32], me: &str, peer: &str) -> Option<Zeroizing<[u8; 32]>> {
    if me == peer {
        return None;
    }
    let ek = StaticSecret::from(*my_ephemeral);
    let (their_ik, their_ek) = (PublicKey::from(*peer_key), PublicKey::from(*peer_ephemeral));
    let ik_ik = identity.secret.diffie_hellman(&their_ik);
    let ek_ek = ek.diffie_hellman(&their_ek);
    let ik_ek = identity.secret.diffie_hellman(&their_ek);
    let ek_ik = ek.diffie_hellman(&their_ik);
    if ![&ik_ik, &ek_ek, &ik_ek, &ek_ik].iter().all(|s| s.was_contributory()) {
        return None;
    }
    // DH(IKlo, EKhi) ‖ DH(EKlo, IKhi): if this side is lo, that's (my IK, their EK) then (my EK,
    // their IK); if it's hi, the same two the other way round.
    let (lo_cross, hi_cross) = ordered(me, &ik_ek, peer, &ek_ik);
    let mut ikm = Zeroizing::new([0u8; 128]);
    ikm[..32].copy_from_slice(ik_ik.as_bytes());
    ikm[32..64].copy_from_slice(ek_ek.as_bytes());
    ikm[64..96].copy_from_slice(lo_cross.as_bytes());
    ikm[96..].copy_from_slice(hi_cross.as_bytes());
    let (salt, info) = salt_and_info(INFO2, me, peer, my_nonce, their_nonce);
    let hk = Hkdf::<Sha256>::new(Some(&salt), ikm.as_ref());
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
    /// From contacts.json: refused whole if its MAC doesn't match this identity. `ephemerals` are
    /// this side's one-time secrets (e2e_store.rs), so v2 epochs can be derived.
    pub fn load(identity: &Identity, me: &str, raw: &Value, ephemerals: HashMap<[u8; 32], Ephemeral>) -> Result<Book, String> {
        let mut book = Book { ephemerals, ..Book::default() };
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
            let keep_copy = c["keepCopy"].as_bool();
            let contact = Contact { peer: peer.to_owned(), mine: list("mine"), theirs: list("theirs"), trusted, verified: c["verified"] == true, ignored, seen, keep_copy };
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
        book.derive_all(identity, me);
        Ok(book)
    }

    pub fn to_json(&self, identity: &Identity) -> Value {
        let body = contacts_body(&self.contacts);
        json!({ "version": 2, "code": CODE_VERSION, "mac": STANDARD.encode(mac_of(identity, &body).finalize().into_bytes()), "contacts": body })
    }

    pub fn derive_all(&mut self, identity: &Identity, me: &str) {
        let peers: Vec<String> = self.contacts.keys().cloned().collect();
        for peer in peers {
            self.derive(identity, me, &peer);
        }
    }

    // Every pair of announcements of the same version made with this identity's key is an epoch;
    // a v2 one only while this side still has its one-time secret.
    fn derive(&mut self, identity: &Identity, me: &str, peer: &str) {
        let Book { contacts, keys, known_v2, ephemerals, .. } = self;
        keys.retain(|(p, _, _), _| p != peer);
        known_v2.retain(|(p, _)| p != peer);
        let Some(c) = contacts.get(peer) else { return };
        for m in c.mine.iter().filter(|m| &m.key == identity.public.as_bytes()) {
            for t in &c.theirs {
                let epoch = epoch_id(me, m, peer, t);
                let derived = match (m.ephemeral, t.ephemeral) {
                    (None, None) => derive_key(identity, &t.key, &m.nonce, &t.nonce, me, peer).map(|k| (1, k)),
                    (Some(mine), Some(theirs)) => {
                        known_v2.insert((peer.to_owned(), epoch));
                        ephemerals.get(&mine).and_then(|e| derive_key2(identity, &e.secret, &t.key, &theirs, &m.nonce, &t.nonce, me, peer)).map(|k| (2, k))
                    }
                    _ => None,
                };
                if let Some((version, key)) = derived {
                    keys.insert((peer.to_owned(), version, epoch), (key, t.key));
                }
            }
        }
    }

    /// A fresh one-time key for this side's next announcement; its secret is kept here (and saved
    /// by the caller through e2e_store.rs) before the key message goes out.
    pub fn new_ephemeral(&mut self, now: u64) -> Result<[u8; 32], String> {
        let secret = Zeroizing::new(random32()?);
        let public = *PublicKey::from(&StaticSecret::from(*secret)).as_bytes();
        self.ephemerals.insert(public, Ephemeral { secret, made: now, retired: None });
        self.secrets_dirty = true;
        Ok(public)
    }

    /// A key message seen in the DM with `peer` (yours if `from_me`).
    pub fn record(&mut self, identity: &Identity, me: &str, peer: &str, ann: Announcement, from_me: bool) -> Event {
        if !is_id(peer) {
            return Event::Refused;
        }
        let contact = self.contacts.entry(peer.to_owned()).or_insert_with(|| Contact::new(peer));
        let list = if from_me { &mut contact.mine } else { &mut contact.theirs };
        if list.iter().any(|a| a.id == ann.id) || contact.ignored.contains(&ann.id) {
            return Event::Nothing;
        }
        // Once both sides have announced v2, v1 never comes back: a newer v1 key message (posted
        // by Discord, or by someone in their account) would only take away the one-time keys.
        // Older ones are history and stay. One under another key is a changed key, asked about.
        if !ann.is_v2() && (from_me || contact.trusted == Some(ann.key)) && contact.v2_since(identity.public.as_bytes()).is_some_and(|since| order(&ann) > since) {
            return Event::Refused;
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

    /// The user's answer to "keep a local copy of this DM?"; only for a contact encryption was
    /// chosen with.
    pub fn set_keep_copy(&mut self, peer: &str, keep: bool) -> bool {
        let Some(c) = self.contacts.get_mut(peer).filter(|c| c.chosen_any() || c.trusted.is_some()) else { return false };
        c.keep_copy = Some(keep);
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
    /// so loading its history doesn't turn encryption on again. This side's one-time secrets for
    /// them go too.
    pub fn forget(&mut self, peer: &str) {
        if let Some(c) = self.contacts.get_mut(peer) {
            let ids: Vec<String> = c.mine.iter().chain(&c.theirs).map(|a| a.id.clone()).collect();
            let used: Vec<[u8; 32]> = c.mine.iter().filter_map(|a| a.ephemeral).collect();
            let ignored = std::mem::take(&mut c.ignored);
            *c = Contact::new(peer);
            c.ignored = ignored;
            for id in ids {
                push_ignored(c, id);
            }
            for ek in used {
                if self.ephemerals.remove(&ek).is_some() {
                    self.secrets_dirty = true;
                }
            }
        }
        self.keys.retain(|(p, _, _), _| p != peer);
        self.known_v2.retain(|(p, _)| p != peer);
    }

    /// After a message opened: true if another Discord message carried the same ciphertext
    /// first (an older id), so this one is a copy posted again. Edits keep their id, so they pass.
    pub fn replayed(&mut self, peer: &str, content: &str, message_id: &str) -> bool {
        let Some((_, epoch, data)) = parse(content) else { return false };
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

    /// Contacts encryption is on with whose current epoch is v2 but can't be derived here (this
    /// device's one-time secrets went missing): sending there needs new keys.
    pub fn keys_missing(&self, identity: &Identity, me: &str) -> usize {
        let my_key = identity.public.as_bytes();
        self.contacts
            .iter()
            .filter(|(peer, c)| {
                c.status(my_key) == Status::On
                    && c.current_pair(my_key).is_some_and(|(m, t)| m.is_v2() && !self.keys.contains_key(&((*peer).clone(), 2, epoch_id(me, m, peer, t))))
            })
            .count()
    }

    /// One-time secrets no longer in any current epoch, and not this side's newest (an offer
    /// waiting for its answer), are marked retired; those retired for `grace` are deleted, with
    /// their epochs' keys. Returns how many were deleted (the caller saves the secrets with a new
    /// file key). A one-time key that never made it into a key message counts after `orphan`.
    pub fn retire(&mut self, identity: &Identity, me: &str, now: u64, grace: u64, orphan: u64) -> usize {
        let my_key = identity.public.as_bytes();
        let mut live: HashSet<[u8; 32]> = HashSet::new();
        let mut announced: HashSet<[u8; 32]> = HashSet::new();
        for c in self.contacts.values() {
            announced.extend(c.mine.iter().filter_map(|a| a.ephemeral));
            live.extend(c.current_pair(my_key).and_then(|(m, _)| m.ephemeral));
            live.extend(c.latest_mine(my_key).and_then(|m| m.ephemeral));
        }
        let mut changed = false;
        for (k, e) in self.ephemerals.iter_mut() {
            let in_use = live.contains(k) || (!announced.contains(k) && now.saturating_sub(e.made) < orphan);
            match (in_use, e.retired) {
                (true, Some(_)) => {
                    e.retired = None;
                    changed = true;
                }
                (false, None) => {
                    e.retired = Some(now);
                    changed = true;
                }
                _ => {}
            }
        }
        let before = self.ephemerals.len();
        self.ephemerals.retain(|_, e| e.retired.is_none_or(|r| now.saturating_sub(r) < grace));
        let deleted = before - self.ephemerals.len();
        if changed || deleted > 0 {
            self.secrets_dirty = true;
        }
        if deleted > 0 {
            self.derive_all(identity, me);
        }
        deleted
    }

    /// Time for new keys with `peer`: encryption is on, the current epoch began `period` ago or
    /// carried RENEW_AFTER messages, and no offer of ours is already waiting. Not with an older
    /// LockCord that hasn't answered our v2 offer (asking again changes nothing).
    pub fn renewal_due(&self, identity: &Identity, me: &str, peer: &str, now: u64, period: u64) -> bool {
        let my_key = identity.public.as_bytes();
        let Some(c) = self.contacts.get(peer) else { return false };
        // Their offer waiting for our answer is renewal enough.
        if c.status(my_key) != Status::On || c.peer_older(my_key) || c.offer_waiting(my_key) {
            return false;
        }
        let (Some(m), Some(t)) = (c.latest_mine(my_key), c.latest_theirs()) else { return false };
        if m.role == Role::Offer && order(m) > order(t) {
            return false;
        }
        let Some((pm, pt)) = c.current_pair(my_key) else { return false };
        // An epoch still in v1 while both sides could do v2: renewed straight away.
        if !pm.is_v2() {
            return true;
        }
        let epoch = epoch_id(me, pm, peer, pt);
        let messages = c.seen.iter().filter(|(e, _, _)| *e == epoch).count();
        now.saturating_sub(pm.at.max(pt.at)) >= period || messages >= RENEW_AFTER
    }

    /// Encrypts with the current pair; only when the status is On. Files only in a v2 epoch.
    #[allow(clippy::too_many_arguments)]
    pub fn seal(&self, identity: &Identity, peer: &str, channel: &str, me: &str, text: &str, files: &[FileRef], now: u64) -> Result<String, String> {
        let c = self.contacts.get(peer).ok_or("Encryption isn't on with them.")?;
        match c.status(identity.public.as_bytes()) {
            Status::On => {}
            Status::Changed => return Err("Their key changed. Check it in the DM's header before anything more is sent.".into()),
            _ => return Err("Encryption isn't on with them yet.".into()),
        }
        let (m, t) = c.current_pair(identity.public.as_bytes()).ok_or("Encryption isn't on with them yet.")?;
        let epoch = epoch_id(me, m, peer, t);
        if m.is_v2() {
            let (key, _) = self.keys.get(&(peer.to_owned(), 2, epoch)).ok_or("This DM's newest keys are missing on this device. Renew the keys in the DM's header.")?;
            seal2(key, epoch, channel, me, text, files, now)
        } else {
            if !files.is_empty() {
                return Err("Their LockCord is older and can't open encrypted files yet. Send them unencrypted on purpose, or wait until they update.".into());
            }
            let (key, _) = self.keys.get(&(peer.to_owned(), 1, epoch)).ok_or("That contact's key is missing.")?;
            seal(key, epoch, channel, me, text, now)
        }
    }

    /// Decrypts an LC1/LC2 message in the DM with `peer`, sent by `sender` (you or them).
    pub fn open(&self, peer: &str, channel: &str, sender: &str, content: &str, posted_at: u64) -> Result<Opened, E2eError> {
        let (version, epoch, _) = parse(content).ok_or(E2eError::NotOurs)?;
        let Some((key, their_key)) = self.keys.get(&(peer.to_owned(), version, epoch)) else {
            return Err(if version == 2 && self.known_v2.contains(&(peer.to_owned(), epoch)) { E2eError::Expired } else { E2eError::NoKey });
        };
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
        let pair = c.current_pair(my_key);
        Some(json!({
            "peer": peer, "status": status, "verified": c.verified, "since": since,
            "version": pair.map(|(m, _)| if m.is_v2() { 2 } else { 1 }),
            // When the current epoch began: the later of its two key messages.
            "epochSince": pair.map(|(m, t)| m.at.max(t.at)),
            "older": c.peer_older(my_key),
            "keepCopy": c.keep_copy,
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

fn aad(version: u8, channel: &str, sender: &str, epoch: u32) -> String {
    format!("LC{version}|{channel}|{sender}|{epoch:08x}")
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

fn frame2(text: &str, files: &[FileRef], now: u64) -> Result<Zeroizing<Vec<u8>>, String> {
    let bytes = text.as_bytes();
    if files.len() > MAX_FILES {
        return Err(format!("Up to {MAX_FILES} encrypted files per message."));
    }
    let need = HEADER2 + bytes.len() + files.len() * FILE_ENTRY;
    let Some(size) = BUCKETS2.iter().copied().find(|b| *b >= need) else {
        let room = MAX_TEXT_BYTES2 - files.len() * FILE_ENTRY;
        return Err(format!("Encrypted messages can be up to about {room} characters{} (fewer with emoji); split it up.", if files.is_empty() { "" } else { " with these files" }));
    };
    let mut out = Zeroizing::new(vec![0u8; size]);
    out[0] = FRAME_VERSION2;
    out[1..9].copy_from_slice(&now.to_be_bytes());
    out[9..11].copy_from_slice(&(bytes.len() as u16).to_be_bytes());
    let mut at = 11;
    out[at..at + bytes.len()].copy_from_slice(bytes);
    at += bytes.len();
    out[at] = files.len() as u8;
    at += 1;
    for f in files {
        out[at..at + 16].copy_from_slice(&f.id);
        out[at + 16..at + FILE_ENTRY].copy_from_slice(f.key.as_ref());
        at += FILE_ENTRY;
    }
    Ok(out)
}

fn unframe2(bytes: &[u8]) -> Option<(u64, String, Vec<FileRef>)> {
    if bytes.first() != Some(&FRAME_VERSION2) || !BUCKETS2.contains(&bytes.len()) {
        return None;
    }
    let sent = u64::from_be_bytes(bytes[1..9].try_into().ok()?);
    let len = u16::from_be_bytes(bytes[9..11].try_into().ok()?) as usize;
    let text = bytes.get(11..11 + len)?;
    let mut at = 11 + len;
    let count = *bytes.get(at)? as usize;
    at += 1;
    if count > MAX_FILES {
        return None;
    }
    let mut files = Vec::with_capacity(count);
    for _ in 0..count {
        let entry = bytes.get(at..at + FILE_ENTRY)?;
        let mut key = Zeroizing::new([0u8; 32]);
        key.copy_from_slice(&entry[16..]);
        files.push(FileRef { id: entry[..16].try_into().ok()?, key });
        at += FILE_ENTRY;
    }
    if bytes[at..].iter().any(|b| *b != 0) {
        return None;
    }
    Some((sent, String::from_utf8(text.to_vec()).ok()?, files))
}

/// "LC1:/LC2:<epoch, 8 hex>:<data>" split up (version, epoch, data), or None if it isn't one.
pub fn parse(content: &str) -> Option<(u8, u32, &str)> {
    let (version, rest) = match content.strip_prefix(PREFIX2) {
        Some(r) => (2, r),
        None => (1, content.strip_prefix(PREFIX)?),
    };
    let (epoch, data) = rest.split_once(':')?;
    if epoch.len() != 8 || data.is_empty() || data.len() > 2000 {
        return None;
    }
    Some((version, u32::from_str_radix(epoch, 16).ok()?, data))
}

/// An encrypted DM message (either version).
pub fn is_sealed(content: &str) -> bool {
    parse(content).is_some()
}

fn encrypt(key: &[u8; 32], aad: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|e| e.to_string())?;
    let sealed = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: aad.as_bytes() }).map_err(|_| "Encryption failed.".to_owned())?;
    let mut data = Vec::with_capacity(12 + sealed.len());
    data.extend_from_slice(&nonce);
    data.extend_from_slice(&sealed);
    Ok(data)
}

pub fn seal(key: &[u8; 32], epoch: u32, channel: &str, sender: &str, text: &str, now: u64) -> Result<String, String> {
    let data = encrypt(key, &aad(1, channel, sender, epoch), &frame(text, now)?)?;
    Ok(format!("{PREFIX}{epoch:08x}:{}", URL_SAFE_NO_PAD.encode(data)))
}

#[allow(clippy::too_many_arguments)]
pub fn seal2(key: &[u8; 32], epoch: u32, channel: &str, sender: &str, text: &str, files: &[FileRef], now: u64) -> Result<String, String> {
    let data = encrypt(key, &aad(2, channel, sender, epoch), &frame2(text, files, now)?)?;
    Ok(format!("{PREFIX2}{epoch:08x}:{}", URL_SAFE_NO_PAD.encode(data)))
}

pub fn open(key: &[u8; 32], channel: &str, sender: &str, content: &str, posted_at: u64) -> Result<Opened, E2eError> {
    let (version, epoch, data) = parse(content).ok_or(E2eError::NotOurs)?;
    let bytes = URL_SAFE_NO_PAD.decode(data).map_err(|_| E2eError::Broken)?;
    if bytes.len() < 12 + 16 {
        return Err(E2eError::Broken);
    }
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| E2eError::Broken)?;
    let associated = aad(version, channel, sender, epoch);
    let plain = Zeroizing::new(cipher.decrypt(Nonce::from_slice(&bytes[..12]), Payload { msg: &bytes[12..], aad: associated.as_bytes() }).map_err(|_| E2eError::Broken)?);
    let (sent_at, text, files) = if version == 2 {
        unframe2(&plain).ok_or(E2eError::Broken)?
    } else {
        unframe(&plain).map(|(at, text)| (at, text, Vec::new())).ok_or(E2eError::Broken)?
    };
    Ok(Opened { text, files, stale: sent_at.abs_diff(posted_at) > CLOCK_SLACK_MS, version, epoch, sent_at })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ann(id: &str, who: &Identity, nonce: u8, role: Role, at: u64) -> Announcement {
        Announcement { id: id.into(), key: *who.public.as_bytes(), ephemeral: None, nonce: [nonce; 32], role, at }
    }

    // A v2 announcement whose one-time secret `book` keeps (book is the announcer's own).
    fn ann2(book: &mut Book, id: &str, who: &Identity, nonce: u8, role: Role, at: u64) -> Announcement {
        let ek = book.new_ephemeral(at).unwrap();
        Announcement { id: id.into(), key: *who.public.as_bytes(), ephemeral: Some(ek), nonce: [nonce; 32], role, at }
    }

    fn text(o: Result<Opened, E2eError>) -> Result<String, E2eError> {
        o.map(|o| o.text)
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

    // The same with v2 key messages.
    fn established2() -> (Identity, Identity, Book, Book) {
        let alice = Identity::from_bytes([1u8; 32]);
        let bob = Identity::from_bytes([2u8; 32]);
        let (mut a, mut b) = (Book::default(), Book::default());
        let offer = ann2(&mut a, "10", &alice, 7, Role::Offer, 1);
        let answer = ann2(&mut b, "11", &bob, 9, Role::Answer, 2);
        a.record(&alice, "100", "200", offer.clone(), true);
        b.record(&bob, "200", "100", offer, false);
        assert!(b.contacts["100"].answer_v2(), "a v2 offer gets a v2 answer");
        b.record(&bob, "200", "100", answer.clone(), true);
        a.record(&alice, "100", "200", answer, false);
        (alice, bob, a, b)
    }

    #[test]
    fn key_messages_round_trip_and_nothing_else_parses() {
        let alice = Identity::from_bytes([1u8; 32]);
        let text = key_message(&alice, None, &[7u8; 32], Role::Offer);
        assert!(text.starts_with("🔑 LockCord key · LCK1:o:"));
        assert_eq!(parse_key_message(&text), Some((Role::Offer, *alice.public.as_bytes(), None, [7u8; 32])));
        assert_eq!(parse_key_message(&text.replace("LCK1:o:", "LCK1:x:")), None);
        assert_eq!(parse_key_message(&(text.clone() + ":extra")), None);
        assert_eq!(parse_key_message("LCK1:o:abc:def"), None);
        let v2 = key_message(&alice, Some(&[5u8; 32]), &[7u8; 32], Role::Answer);
        assert!(v2.starts_with("🔑 LockCord key · LCK2:a:"));
        assert_eq!(parse_key_message(&v2), Some((Role::Answer, *alice.public.as_bytes(), Some([5u8; 32]), [7u8; 32])));
        assert_eq!(parse_key_message(&(v2.clone() + ":extra")), None);
        assert_eq!(parse_key_message(&v2.replace("LCK2:", "LCK1:")), None, "a v1 prefix with a v2 body");
        assert_eq!(parse_key_message(&text.replace("LCK1:", "LCK2:")), None, "a v2 prefix with a v1 body");
    }

    #[test]
    fn both_sides_agree_and_read_each_other() {
        let (alice, bob, a, b) = established();
        assert_eq!(a.status(&alice, "200"), Status::On);
        assert_eq!(b.status(&bob, "100"), Status::On);
        let now = 1_700_000_000_000;
        let sealed = a.seal(&alice, "200", "555", "100", "hei Bob 👋", &[], now).unwrap();
        assert!(sealed.starts_with("LC1:") && !sealed.contains("hei"));
        assert_eq!(text(b.open("100", "555", "100", &sealed, now + 500)), Ok("hei Bob 👋".into()));
        assert_eq!(text(a.open("200", "555", "100", &sealed, now)), Ok("hei Bob 👋".into()), "your own, read back");
        let reply = b.seal(&bob, "100", "555", "200", "moi", &[], now).unwrap();
        assert_eq!(text(a.open("200", "555", "200", &reply, now)), Ok("moi".into()));
        assert_eq!(a.view(&alice, "200").unwrap()["safety"], b.view(&bob, "100").unwrap()["safety"]);
    }

    #[test]
    fn v2_both_sides_agree_and_read_each_other() {
        let (alice, bob, a, b) = established2();
        assert_eq!(a.status(&alice, "200"), Status::On);
        assert_eq!(b.status(&bob, "100"), Status::On);
        let now = 1_700_000_000_000;
        let sealed = a.seal(&alice, "200", "555", "100", "hei Bob 👋", &[], now).unwrap();
        assert!(sealed.starts_with("LC2:") && !sealed.contains("hei"));
        let opened = b.open("100", "555", "100", &sealed, now + 500).unwrap();
        assert_eq!((opened.text.as_str(), opened.version, opened.stale), ("hei Bob 👋", 2, false));
        assert_eq!(text(a.open("200", "555", "100", &sealed, now)), Ok("hei Bob 👋".into()), "your own, read back");
        let reply = b.seal(&bob, "100", "555", "200", "moi", &[], now).unwrap();
        assert_eq!(text(a.open("200", "555", "200", &reply, now)), Ok("moi".into()));
        assert_eq!(a.view(&alice, "200").unwrap()["version"], 2);
        // Lo and hi the other way round derive the same key too.
        let k_ab = derive_key2(&alice, &[3u8; 32], bob.public.as_bytes(), PublicKey::from(&StaticSecret::from([4u8; 32])).as_bytes(), &[1u8; 32], &[2u8; 32], "100", "200").unwrap();
        let k_ba = derive_key2(&bob, &[4u8; 32], alice.public.as_bytes(), PublicKey::from(&StaticSecret::from([3u8; 32])).as_bytes(), &[2u8; 32], &[1u8; 32], "200", "100").unwrap();
        assert_eq!(k_ab, k_ba);
        let v1 = derive_key(&alice, bob.public.as_bytes(), &[1u8; 32], &[2u8; 32], "100", "200").unwrap();
        assert_ne!(k_ab, v1);
    }

    #[test]
    fn v2_offering_at_once_still_meets_and_renewing_keeps_history() {
        let (alice, bob, mut a, mut b) = established2();
        let old = a.seal(&alice, "200", "555", "100", "old", &[], 5).unwrap();
        let ao = ann2(&mut a, "20", &alice, 3, Role::Offer, 10);
        let bo = ann2(&mut b, "21", &bob, 4, Role::Offer, 10);
        a.record(&alice, "100", "200", ao.clone(), true);
        a.record(&alice, "100", "200", bo.clone(), false);
        b.record(&bob, "200", "100", bo, true);
        b.record(&bob, "200", "100", ao, false);
        let new = a.seal(&alice, "200", "555", "100", "new", &[], 11).unwrap();
        assert_eq!(text(b.open("100", "555", "100", &new, 11)), Ok("new".into()), "the same newest pair on both sides");
        assert_eq!(text(b.open("100", "555", "100", &old, 5)), Ok("old".into()), "old epochs still read while their secrets are kept");
        assert_ne!(parse(&new).unwrap().1, parse(&old).unwrap().1);
    }

    #[test]
    fn a_deleted_one_time_secret_makes_its_epoch_unreadable() {
        let (alice, bob, mut a, mut b) = established2();
        let old = a.seal(&alice, "200", "555", "100", "old", &[], 5).unwrap();
        // Renewed, then both sides delete their old one-time secrets.
        let ao = ann2(&mut a, "20", &alice, 3, Role::Offer, 10);
        let ba = ann2(&mut b, "21", &bob, 4, Role::Answer, 11);
        for (book, me, peer, who) in [(&mut a, "100", "200", &alice), (&mut b, "200", "100", &bob)] {
            book.record(who, me, peer, ao.clone(), me == "100");
            book.record(who, me, peer, ba.clone(), me == "200");
        }
        let old_mine_a = a.contacts["200"].mine.iter().find(|m| m.id == "10").unwrap().ephemeral.unwrap();
        let old_mine_b = b.contacts["100"].mine.iter().find(|m| m.id == "11").unwrap().ephemeral.unwrap();
        a.ephemerals.remove(&old_mine_a);
        b.ephemerals.remove(&old_mine_b);
        a.derive_all(&alice, "100");
        b.derive_all(&bob, "200");
        assert_eq!(b.open("100", "555", "100", &old, 5), Err(E2eError::Expired));
        assert_eq!(a.open("200", "555", "100", &old, 5), Err(E2eError::Expired));
        // Both identity keys and every public value are no help: the epoch's key needs a deleted
        // one-time secret. Trying with the identities alone (v1 style) gives another key.
        let (_, epoch, _) = parse(&old).unwrap();
        let o = &a.contacts["200"];
        let (m, t) = (o.mine.iter().find(|m| m.id == "10").unwrap(), o.theirs.iter().find(|t| t.id == "11").unwrap());
        assert_eq!(epoch_id("100", m, "200", t), epoch);
        let guess = derive_key(&alice, &t.key, &m.nonce, &t.nonce, "100", "200").unwrap();
        assert_eq!(open(&guess, "555", "100", &old, 5), Err(E2eError::Broken));
        // The new epoch still works.
        let new = a.seal(&alice, "200", "555", "100", "new", &[], 12).unwrap();
        assert_eq!(text(b.open("100", "555", "100", &new, 12)), Ok("new".into()));
    }

    #[test]
    fn a_one_time_key_swapped_under_the_trusted_identity_gets_nowhere() {
        let (alice, bob, mut a, mut b) = established2();
        // Mallory has Alice's Discord account (not her LockCord) and posts a renewal carrying
        // Alice's public identity key with a one-time key of her own.
        let mallory = Identity::from_bytes([9u8; 32]);
        let mut m = Book::default();
        let fake = ann2(&mut m, "30", &alice, 5, Role::Offer, 20);
        assert_eq!(b.record(&bob, "200", "100", fake.clone(), false), Event::Nothing, "the identity is the trusted one");
        assert!(b.contacts["100"].needs_answer(bob.public.as_bytes()), "so Bob's LockCord answers it");
        let answer = ann2(&mut b, "31", &bob, 6, Role::Answer, 21);
        b.record(&bob, "200", "100", answer.clone(), true);
        let to_alice = b.seal(&bob, "100", "555", "200", "for Alice only", &[], 22).unwrap();
        // Mallory knows her one-time secret but not Alice's identity secret: wrong key.
        m.record(&mallory, "100", "200", fake, true);
        m.record(&mallory, "100", "200", answer.clone(), false);
        assert!(m.open("200", "555", "200", &to_alice, 22).is_err());
        // Nor can she write as Alice under that epoch.
        if let Ok(forged) = m.seal(&mallory, "200", "555", "100", "send me your password", &[], 22) {
            assert!(b.open("100", "555", "100", &forged, 22).is_err());
        }
        // Alice herself doesn't hold the fake one-time secret either: that epoch stays unreadable
        // to her, never readable to Mallory.
        a.record(&alice, "100", "200", answer, false);
        assert!(a.open("200", "555", "200", &to_alice, 22).is_err());
    }

    #[test]
    fn an_older_peer_keeps_v1_until_it_answers_v2() {
        // Encryption on in v1 (both on 1.6); Alice updates to 1.7 and renews: a v2 offer that
        // Bob's 1.6 can't read.
        let (alice, bob, mut a, mut b) = established();
        let renewal = ann2(&mut a, "20", &alice, 3, Role::Offer, 10);
        a.record(&alice, "100", "200", renewal.clone(), true);
        assert_eq!(a.status(&alice, "200"), Status::On);
        assert!(a.view(&alice, "200").unwrap()["older"] == true);
        let sealed = a.seal(&alice, "200", "555", "100", "still v1", &[], 11).unwrap();
        assert!(sealed.starts_with("LC1:"), "Bob's 1.6 reads only LC1");
        assert_eq!(text(b.open("100", "555", "100", &sealed, 11)), Ok("still v1".into()));
        assert!(a.seal(&alice, "200", "555", "100", "x", &[FileRef { id: [1; 16], key: Zeroizing::new([2; 32]) }], 11).is_err(), "no encrypted files for an older LockCord");
        // Bob's 1.6 renews in v1 itself: Alice answers in v1, and her next renewal is v2 again.
        let bob_v1 = ann("21", &bob, 4, Role::Offer, 12);
        a.record(&alice, "100", "200", bob_v1.clone(), false);
        assert!(a.contacts["200"].needs_answer(alice.public.as_bytes()) && !a.contacts["200"].answer_v2());
        let alice_v1 = ann("23", &alice, 8, Role::Answer, 13);
        a.record(&alice, "100", "200", alice_v1.clone(), true);
        assert!(!a.contacts["200"].peer_older(alice.public.as_bytes()));
        assert!(a.renewal_due(&alice, "100", "200", 14, 7 * 86_400_000), "a v1 epoch between two v2-able sides is renewed");
        // Bob updates to 1.7 (his LockCord read none of Alice's v2 offers): his own renewal is v2,
        // and Alice's LockCord answers it in v2 by itself.
        b.record(&bob, "200", "100", bob_v1, true);
        b.record(&bob, "200", "100", renewal, false);
        b.record(&bob, "200", "100", alice_v1, false);
        assert!(!b.contacts["100"].offer_waiting(bob.public.as_bytes()));
        assert!(b.renewal_due(&bob, "200", "100", 14, 7 * 86_400_000));
        let bob_v2 = ann2(&mut b, "24", &bob, 5, Role::Offer, 14);
        b.record(&bob, "200", "100", bob_v2.clone(), true);
        a.record(&alice, "100", "200", bob_v2.clone(), false);
        assert!(a.contacts["200"].needs_answer(alice.public.as_bytes()) && a.contacts["200"].answer_v2());
        let answer = ann2(&mut a, "25", &alice, 6, Role::Answer, 15);
        a.record(&alice, "100", "200", answer.clone(), true);
        b.record(&bob, "200", "100", answer, false);
        let v2 = a.seal(&alice, "200", "555", "100", "now v2", &[], 16).unwrap();
        assert!(v2.starts_with("LC2:"));
        assert_eq!(text(b.open("100", "555", "100", &v2, 14)), Ok("now v2".into()));
        assert_eq!(text(b.open("100", "555", "100", &sealed, 11)), Ok("still v1".into()), "v1 history stays readable");
        assert!(a.view(&alice, "200").unwrap()["older"] == false);
    }

    #[test]
    fn once_both_are_on_v2_a_v1_key_message_cant_take_it_back() {
        let (alice, bob, mut a, _b) = established2();
        // A v1 offer under Bob's trusted key (Discord, or someone in his account): not recorded,
        // not answered, and sealing stays v2.
        let downgrade = ann("30", &bob, 4, Role::Offer, 20);
        assert_eq!(a.record(&alice, "100", "200", downgrade, false), Event::Refused);
        assert!(!a.contacts["200"].needs_answer(alice.public.as_bytes()));
        assert!(a.seal(&alice, "200", "555", "100", "x", &[], 21).unwrap().starts_with("LC2:"));
        // Nor one posted as if from this side.
        assert_eq!(a.record(&alice, "100", "200", ann("31", &alice, 5, Role::Answer, 22), true), Event::Refused);
        assert!(a.seal(&alice, "200", "555", "100", "x", &[], 23).unwrap().starts_with("LC2:"));
        // Older v1 key messages are history and still recorded; a new key is still a changed key.
        assert_eq!(a.record(&alice, "100", "200", ann("5", &bob, 6, Role::Offer, 0), false), Event::Nothing);
        let mallory = Identity::from_bytes([9u8; 32]);
        assert_eq!(a.record(&alice, "100", "200", ann("32", &mallory, 7, Role::Offer, 24), false), Event::Changed);
        // A peer id that isn't a user id is never recorded.
        assert_eq!(a.record(&alice, "100", "../x", ann("33", &bob, 8, Role::Offer, 25), false), Event::Refused);
        assert!(!a.contacts.contains_key("../x"));
    }

    #[test]
    fn offering_at_once_still_meets_and_renewing_keeps_history() {
        let (alice, bob, mut a, mut b) = established();
        let old = a.seal(&alice, "200", "555", "100", "old", &[], 5).unwrap();
        // Both renew at once: each sees the other's offer.
        let ao = ann("20", &alice, 3, Role::Offer, 10);
        let bo = ann("21", &bob, 4, Role::Offer, 10);
        a.record(&alice, "100", "200", ao.clone(), true);
        a.record(&alice, "100", "200", bo.clone(), false);
        b.record(&bob, "200", "100", bo, true);
        b.record(&bob, "200", "100", ao, false);
        let new = a.seal(&alice, "200", "555", "100", "new", &[], 11).unwrap();
        assert_eq!(text(b.open("100", "555", "100", &new, 11)), Ok("new".into()), "the same newest pair on both sides");
        assert_eq!(text(b.open("100", "555", "100", &old, 5)), Ok("old".into()), "old epochs still read");
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
        assert!(a.seal(&alice, "200", "555", "100", "hi", &[], 21).is_err(), "nothing goes out");
        assert!(!a.contacts["200"].needs_answer(alice.public.as_bytes()), "no automatic answer to a changed key");
        // Mallory, who swapped the key, writes as Bob: not shown.
        let mut m = Book::default();
        let alice_answer = ann("32", &alice, 7, Role::Answer, 22);
        m.record(&mallory, "200", "100", ann("30", &mallory, 5, Role::Offer, 20), true);
        m.record(&mallory, "200", "100", alice_answer.clone(), false);
        let forged = m.seal(&mallory, "100", "555", "200", "send me your password", &[], 22).unwrap();
        a.record(&alice, "100", "200", alice_answer, true);
        assert_eq!(a.open("200", "555", "200", &forged, 22), Err(E2eError::Untrusted));
        assert!(a.view(&alice, "200").unwrap()["newSafety"].is_string());
        assert!(a.accept_change("200"));
        assert_eq!(a.status(&alice, "200"), Status::On);
    }

    #[test]
    fn tampering_replay_and_impersonation_fail() {
        for (alice, _, a, b) in [established(), established2()] {
            let now = 1_700_000_000_000;
            let sealed = a.seal(&alice, "200", "555", "100", "secret", &[], now).unwrap();
            assert_eq!(b.open("100", "556", "100", &sealed, now), Err(E2eError::Broken), "another DM");
            assert_eq!(b.open("100", "555", "200", &sealed, now), Err(E2eError::Broken), "claimed to be from Bob");
            let mut chars: Vec<char> = sealed.chars().collect();
            let i = chars.len() - 5;
            chars[i] = if chars[i] == 'A' { 'B' } else { 'A' };
            assert_eq!(b.open("100", "555", "100", &chars.into_iter().collect::<String>(), now), Err(E2eError::Broken));
            let stale = b.open("100", "555", "100", &sealed, now + 3_600_000).unwrap();
            assert!(stale.stale && stale.text == "secret", "posted again later");
            assert_eq!(b.open("100", "555", "100", &format!("{}deadbeef:{}", &sealed[..4], &sealed[13..]), now), Err(E2eError::NoKey));
            assert_eq!(b.open("100", "555", "100", "hello", now), Err(E2eError::NotOurs));
            // A v2 ciphertext relabelled as v1 (or back) doesn't open: the version is authenticated.
            let other = if sealed.starts_with(PREFIX2) { sealed.replacen(PREFIX2, PREFIX, 1) } else { sealed.replacen(PREFIX, PREFIX2, 1) };
            assert!(b.open("100", "555", "100", &other, now).is_err());
        }
    }

    #[test]
    fn a_ciphertext_posted_again_is_flagged_but_an_edit_is_not() {
        let (alice, _, a, mut b) = established();
        let sealed = a.seal(&alice, "200", "555", "100", "pay me", &[], 50).unwrap();
        assert!(!b.replayed("100", &sealed, "900"));
        assert!(!b.replayed("100", &sealed, "900"), "the same message again (an edit, a reload)");
        assert!(b.replayed("100", &sealed, "901"), "the same ciphertext under another message");
        assert!(!b.replayed("100", &sealed, "899"), "an older id is the original");
        assert!(b.replayed("100", &sealed, "900"));
        let saved = b.to_json(&Identity::from_bytes([2u8; 32]));
        let mut back = Book::load(&Identity::from_bytes([2u8; 32]), "200", &saved, HashMap::new()).unwrap();
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

        // Forgetting a v2 contact drops this side's one-time secrets for them.
        let (_, _, mut a, _) = established2();
        assert_eq!(a.ephemerals.len(), 1);
        a.forget("200");
        assert!(a.ephemerals.is_empty() && a.secrets_dirty);
    }

    #[test]
    fn old_comparisons_are_asked_again_once() {
        let (alice, _, mut a, _) = established();
        a.set_verified("200", true);
        let mut old = a.to_json(&alice);
        old.as_object_mut().unwrap().remove("code");
        let loaded = Book::load(&alice, "100", &old, HashMap::new()).unwrap();
        assert!(loaded.code_upgraded && !loaded.contacts["200"].verified);
        let loaded = Book::load(&alice, "100", &a.to_json(&alice), HashMap::new()).unwrap();
        assert!(!loaded.code_upgraded && loaded.contacts["200"].verified);
    }

    #[test]
    fn padding_hides_lengths_and_long_text_is_refused() {
        let (alice, _, a, _) = established();
        let short = a.seal(&alice, "200", "555", "100", "hi", &[], 0).unwrap();
        let longer = a.seal(&alice, "200", "555", "100", "hello there, how are you", &[], 0).unwrap();
        assert_eq!(short.len(), longer.len());
        let max = a.seal(&alice, "200", "555", "100", &"x".repeat(MAX_TEXT_BYTES), &[], 0).unwrap();
        assert!(max.len() < 2000, "fits Discord: {}", max.len());
        assert!(a.seal(&alice, "200", "555", "100", &"x".repeat(MAX_TEXT_BYTES + 1), &[], 0).is_err());
    }

    #[test]
    fn v2_frames_carry_files_fit_discord_and_refuse_oversize() {
        let (alice, _, a, b) = established2();
        let short = a.seal(&alice, "200", "555", "100", "hi", &[], 0).unwrap();
        let longer = a.seal(&alice, "200", "555", "100", "hello there, how are you", &[], 0).unwrap();
        assert_eq!(short.len(), longer.len());
        let max = a.seal(&alice, "200", "555", "100", &"x".repeat(MAX_TEXT_BYTES2), &[], 0).unwrap();
        assert!(max.len() < 2000, "fits Discord: {}", max.len());
        assert!(a.seal(&alice, "200", "555", "100", &"x".repeat(MAX_TEXT_BYTES2 + 1), &[], 0).is_err());
        let files: Vec<FileRef> = (0..MAX_FILES as u8).map(|i| FileRef { id: [i; 16], key: Zeroizing::new([i + 100; 32]) }).collect();
        let with = a.seal(&alice, "200", "555", "100", "ten files", &files, 0).unwrap();
        assert!(with.len() < 2000);
        let opened = b.open("100", "555", "100", &with, 0).unwrap();
        assert_eq!((opened.text.as_str(), &opened.files), ("ten files", &files));
        assert!(format!("{:?}", opened.files[3]).find("103").is_none(), "keys never in Debug output");
        let none = b.open("100", "555", "100", &short, 0).unwrap();
        assert!(none.files.is_empty());
        let eleven: Vec<FileRef> = (0..11u8).map(|i| FileRef { id: [i; 16], key: Zeroizing::new([i; 32]) }).collect();
        assert!(a.seal(&alice, "200", "555", "100", "x", &eleven, 0).is_err());
        let room = MAX_TEXT_BYTES2 - MAX_FILES * FILE_ENTRY;
        assert!(a.seal(&alice, "200", "555", "100", &"x".repeat(room), &files, 0).is_ok());
        assert!(a.seal(&alice, "200", "555", "100", &"x".repeat(room + 1), &files, 0).is_err());
        // A frame claiming more files than it holds, or non-zero padding, is broken.
        let mut f = frame2("a", &files[..1], 0).unwrap();
        f[12] = 2;
        assert!(unframe2(&f).is_none());
        let mut g = frame2("a", &[], 0).unwrap();
        let last = g.len() - 1;
        g[last] = 1;
        assert!(unframe2(&g).is_none());
    }

    #[test]
    fn waiting_seals_nothing_and_bad_keys_derive_nothing() {
        let alice = Identity::from_bytes([1u8; 32]);
        let mut a = Book::default();
        a.record(&alice, "100", "200", ann("10", &alice, 7, Role::Offer, 1), true);
        assert_eq!(a.status(&alice, "200"), Status::Waiting);
        assert!(a.seal(&alice, "200", "555", "100", "hi", &[], 2).is_err());
        assert!(derive_key(&alice, &[0u8; 32], &[0u8; 32], &[0u8; 32], "100", "200").is_none(), "all-zero shared secret");
        assert!(derive_key(&alice, alice.public.as_bytes(), &[0u8; 32], &[0u8; 32], "100", "100").is_none(), "yourself");
        let good = *PublicKey::from(&StaticSecret::from([8u8; 32])).as_bytes();
        assert!(derive_key2(&alice, &[3u8; 32], &good, &[0u8; 32], &[0u8; 32], &[0u8; 32], "100", "200").is_none(), "an all-zero one-time key");
        assert!(derive_key2(&alice, &[3u8; 32], &[0u8; 32], &good, &[0u8; 32], &[0u8; 32], "100", "200").is_none(), "an all-zero identity");
        // A v2 offer alone, to someone on 1.6 who never answers: waiting, nothing sealed.
        let mut a = Book::default();
        let offer = ann2(&mut a, "10", &alice, 7, Role::Offer, 1);
        a.record(&alice, "100", "200", offer, true);
        assert_eq!(a.status(&alice, "200"), Status::Waiting);
    }

    #[test]
    fn the_contacts_file_round_trips_and_tampering_is_caught() {
        let (alice, _, mut a, _) = established();
        let saved = a.to_json(&alice);
        let loaded = Book::load(&alice, "100", &saved, HashMap::new()).unwrap();
        assert_eq!(loaded.contacts, a.contacts);
        assert!(loaded.seal(&alice, "200", "555", "100", "x", &[], 0).is_ok(), "keys derived again on load");
        let mut changed = saved.clone();
        changed["contacts"][0]["trusted"] = json!(STANDARD.encode([5u8; 32]));
        assert!(Book::load(&alice, "100", &changed, HashMap::new()).is_err());
        assert!(Book::load(&Identity::from_bytes([3u8; 32]), "100", &saved, HashMap::new()).is_err(), "another profile's file");
        // keepCopy is covered by the MAC too.
        a.contacts.get_mut("200").unwrap().keep_copy = Some(true);
        let mut kept = a.to_json(&alice);
        assert_eq!(Book::load(&alice, "100", &kept, HashMap::new()).unwrap().contacts["200"].keep_copy, Some(true));
        kept["contacts"][0]["keepCopy"] = json!(false);
        assert!(Book::load(&alice, "100", &kept, HashMap::new()).is_err());
    }

    #[test]
    fn one_time_secrets_retire_then_go_after_the_grace_time() {
        let (alice, _, mut a, _) = established2();
        let (day, week) = (86_400_000, 7 * 86_400_000);
        assert_eq!(a.retire(&alice, "100", 10 * day, week, day), 0);
        assert!(a.ephemerals.values().all(|e| e.retired.is_none()), "the current epoch's secret stays");
        let old_msg = a.seal(&alice, "200", "555", "100", "old", &[], 5).unwrap();
        // Alice renews: her offer's secret is the newest (kept), the old one retires.
        let ao = ann2(&mut a, "20", &alice, 3, Role::Offer, 10 * day);
        a.record(&alice, "100", "200", ao, true);
        let old = a.contacts["200"].mine.iter().find(|m| m.id == "10").unwrap().ephemeral.unwrap();
        assert_eq!(a.retire(&alice, "100", 10 * day + 1, week, day), 0);
        assert!(a.ephemerals[&old].retired.is_some() && a.secrets_dirty);
        assert_eq!(a.ephemerals.len(), 2);
        assert_eq!(a.retire(&alice, "100", 17 * day, week, day), 0, "still within the grace time");
        assert!(a.open("200", "555", "100", &old_msg, 5).is_ok());
        assert_eq!(a.retire(&alice, "100", 17 * day + 2, week, day), 1);
        assert!(!a.ephemerals.contains_key(&old));
        assert_eq!(a.open("200", "555", "100", &old_msg, 5), Err(E2eError::Expired));
        // A one-time key whose key message never went out: kept a day, then retired.
        let orphan = a.new_ephemeral(20 * day).unwrap();
        a.retire(&alice, "100", 20 * day + 1, week, day);
        assert!(a.ephemerals[&orphan].retired.is_none());
        a.retire(&alice, "100", 21 * day + 1, week, day);
        assert!(a.ephemerals[&orphan].retired.is_some());
        assert_eq!(a.retire(&alice, "100", 28 * day + 2, week, day), 1);
    }

    #[test]
    fn renewal_comes_when_due_and_only_once() {
        let week = 7 * 86_400_000;
        let (alice, bob, mut a, mut b) = established2();
        assert!(!a.renewal_due(&alice, "100", "200", 2, week));
        assert!(a.renewal_due(&alice, "100", "200", week + 2, week));
        let ao = ann2(&mut a, "20", &alice, 3, Role::Offer, week + 3);
        a.record(&alice, "100", "200", ao.clone(), true);
        assert!(!a.renewal_due(&alice, "100", "200", 3 * week, week), "our offer is waiting for its answer");
        b.record(&bob, "200", "100", ao, false);
        assert!(!b.renewal_due(&bob, "200", "100", 3 * week, week), "their offer gets an answer instead");

        // By count, whatever the age.
        let (alice, _, mut a, _) = established2();
        let c = a.contacts.get_mut("200").unwrap();
        let (m, t) = c.current_pair(alice.public.as_bytes()).map(|(m, t)| (m.clone(), t.clone())).unwrap();
        let epoch = epoch_id("100", &m, "200", &t);
        c.seen = (0..RENEW_AFTER).map(|i| (epoch, [i as u8; 12], (1000 + i).to_string())).collect();
        assert!(a.renewal_due(&alice, "100", "200", 3, week));

        // A v1 epoch is renewed straight away (to move to v2), but not again while an older
        // LockCord hasn't answered.
        let (alice, _, mut a, _) = established();
        assert!(a.renewal_due(&alice, "100", "200", 3, week));
        let ao = ann2(&mut a, "20", &alice, 3, Role::Offer, 10);
        a.record(&alice, "100", "200", ao, true);
        assert!(!a.renewal_due(&alice, "100", "200", 10 * week, week));
    }

    #[test]
    fn v2_contacts_round_trip_and_need_the_one_time_secrets() {
        let (alice, _, a, _) = established2();
        let saved = a.to_json(&alice);
        let without = Book::load(&alice, "100", &saved, HashMap::new()).unwrap();
        assert_eq!(without.contacts, a.contacts, "one-time public keys kept");
        assert!(without.seal(&alice, "200", "555", "100", "x", &[], 0).is_err(), "no secret, no key");
        let secrets = a.ephemerals.iter().map(|(k, e)| (*k, Ephemeral { secret: e.secret.clone(), made: e.made, retired: e.retired })).collect();
        let with = Book::load(&alice, "100", &saved, secrets).unwrap();
        assert!(with.seal(&alice, "200", "555", "100", "x", &[], 0).unwrap().starts_with("LC2:"));
    }
}
