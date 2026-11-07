//! The local copy of encrypted DMs, kept only for the DMs where the user said yes (asked per DM,
//! e2e.rs `keep_copy`). With rotating keys an epoch can't be opened again once its one-time secret
//! is deleted; the copy keeps what this device already opened (text, send time, the files' keys),
//! so the history stays readable here. Messages never opened before then can't be read any more.
//!
//! One sealed file per DM, `e2e/history-<peer>.bin` (e2e_store.rs; the key lives in the vault as
//! `e2e-history:<profile>`), loaded the first time that DM needs it. Edits overwrite an entry,
//! deletions remove it. Erasing a DM's copy deletes its file and seals the others under a new key.

use crate::e2e::{FileRef, Opened};
use crate::e2e_store;
use crate::settings::Store;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use zeroize::Zeroizing;

const WHAT: &str = "history";
/// The oldest go first past this many messages per DM.
const MAX_ENTRIES: usize = 20_000;

#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    /// Who sent it: an entry is only shown again on a message from the same sender, so Discord
    /// can't put one side's words under the other's name.
    pub sender: String,
    pub text: String,
    pub sent_at: u64,
    pub files: Vec<FileRef>,
}

fn file_of(peer: &str) -> String {
    format!("e2e/history-{peer}.bin")
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

pub fn encode(entries: &BTreeMap<u64, Entry>) -> Zeroizing<Vec<u8>> {
    let body: serde_json::Map<String, Value> = entries
        .iter()
        .map(|(id, e)| {
            let files: Vec<Value> = e.files.iter().map(|f| json!([crate::e2e::hex16(&f.id), STANDARD.encode(f.key.as_ref())])).collect();
            (id.to_string(), json!({ "s": e.sender, "t": e.text, "at": e.sent_at, "f": files }))
        })
        .collect();
    Zeroizing::new(Value::Object(body).to_string().into_bytes())
}

fn decode_file(v: &Value) -> Option<FileRef> {
    let hex = v[0].as_str().filter(|h| h.len() == 32)?;
    let mut id = [0u8; 16];
    for (i, b) in id.iter_mut().enumerate() {
        *b = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    let raw = Zeroizing::new(STANDARD.decode(v[1].as_str()?).ok()?);
    Some(FileRef { id, key: Zeroizing::new(<[u8; 32]>::try_from(raw.as_slice()).ok()?) })
}

pub fn decode(bytes: &[u8]) -> Option<BTreeMap<u64, Entry>> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let mut out = BTreeMap::new();
    for (id, e) in v.as_object()? {
        let id: u64 = id.parse().ok()?;
        let files = e["f"].as_array()?.iter().map(decode_file).collect::<Option<Vec<_>>>()?;
        let sender = e["s"].as_str().filter(|s| is_id(s))?.to_owned();
        out.insert(id, Entry { sender, text: e["t"].as_str()?.to_owned(), sent_at: e["at"].as_u64().unwrap_or(0), files });
    }
    Some(out)
}

/// The local copies, per DM, loaded when first needed.
pub struct Copies {
    store: Option<Store>,
    profile: String,
    loaded: HashMap<String, BTreeMap<u64, Entry>>,
    dirty: HashSet<String>,
    /// A file that wouldn't open is left alone (never overwritten) and said once.
    pub problem: Option<String>,
    broken: HashSet<String>,
}

impl Default for Copies {
    fn default() -> Copies {
        Copies::new(None, "")
    }
}

impl Copies {
    pub fn new(store: Option<Store>, profile: &str) -> Copies {
        Copies { store, profile: profile.to_owned(), loaded: HashMap::new(), dirty: HashSet::new(), problem: None, broken: HashSet::new() }
    }

    fn dm(&mut self, peer: &str) -> Option<&mut BTreeMap<u64, Entry>> {
        if !is_id(peer) || self.broken.contains(peer) {
            return None;
        }
        if !self.loaded.contains_key(peer) {
            let entries = match &self.store {
                Some(store) => match e2e_store::read_sealed(store, WHAT, &self.profile, &file_of(peer)) {
                    Ok(Some(plain)) => decode(&plain),
                    Ok(None) => Some(BTreeMap::new()),
                    Err(e) => {
                        self.problem = Some(e);
                        None
                    }
                },
                None => Some(BTreeMap::new()),
            };
            let Some(entries) = entries else {
                self.broken.insert(peer.to_owned());
                self.problem.get_or_insert_with(|| "A local copy of an encrypted DM is damaged; it's left as it is.".to_owned());
                return None;
            };
            self.loaded.insert(peer.to_owned(), entries);
        }
        self.loaded.get_mut(peer)
    }

    /// An opened message from `sender`, kept (or updated, after an edit).
    pub fn put(&mut self, peer: &str, message_id: &str, sender: &str, opened: &Opened) {
        let Ok(id) = message_id.parse::<u64>() else { return };
        if !is_id(sender) {
            return;
        }
        let entry = Entry { sender: sender.to_owned(), text: opened.text.clone(), sent_at: opened.sent_at, files: opened.files.clone() };
        let Some(dm) = self.dm(peer) else { return };
        if dm.get(&id) == Some(&entry) {
            return;
        }
        dm.insert(id, entry);
        while dm.len() > MAX_ENTRIES {
            dm.pop_first();
        }
        self.dirty.insert(peer.to_owned());
    }

    /// The kept message, only if this message comes from the same sender as the one kept.
    pub fn get(&mut self, peer: &str, message_id: &str, sender: &str) -> Option<Entry> {
        let id = message_id.parse::<u64>().ok()?;
        self.dm(peer)?.get(&id).filter(|e| e.sender == sender).cloned()
    }

    /// A message deleted in the DM: out of the copy too.
    pub fn remove(&mut self, peer: &str, message_id: &str) {
        let Ok(id) = message_id.parse::<u64>() else { return };
        if !is_id(peer) {
            return;
        }
        // Only a copy that exists is loaded for this.
        if !self.loaded.contains_key(peer) && !self.store.as_ref().is_some_and(|s| s.exists(&file_of(peer))) {
            return;
        }
        if self.dm(peer).is_some_and(|dm| dm.remove(&id).is_some()) {
            self.dirty.insert(peer.to_owned());
        }
    }

    /// The changed copies, written.
    pub fn save(&mut self) -> Result<(), String> {
        let Some(store) = &self.store else { return Ok(()) };
        for peer in std::mem::take(&mut self.dirty) {
            if let Some(entries) = self.loaded.get(&peer) {
                e2e_store::write_sealed(store, WHAT, &self.profile, &file_of(&peer), &encode(entries), false)?;
            }
        }
        Ok(())
    }

    /// A DM's copy deleted; the other DMs' copies are sealed again under a new key, so a leftover
    /// of the deleted file can't be opened.
    pub fn erase(&mut self, peer: &str, others: &[String]) -> Result<(), String> {
        // The id becomes part of a file name.
        if !is_id(peer) {
            return Err("Not a user.".into());
        }
        self.loaded.remove(peer);
        self.dirty.remove(peer);
        self.broken.remove(peer);
        let Some(store) = self.store.clone() else { return Ok(()) };
        if !store.exists(&file_of(peer)) {
            return Ok(());
        }
        store.remove(&file_of(peer));
        let mut kept = Vec::new();
        for other in others.iter().filter(|o| is_id(o) && o.as_str() != peer && store.exists(&file_of(o))) {
            if let Some(entries) = self.dm(other) {
                kept.push((other.clone(), encode(entries)));
            }
        }
        match kept.split_first() {
            // The first one written rotates the key; the rest are written under the new one.
            Some(((first, plain), rest)) => {
                e2e_store::write_sealed(&store, WHAT, &self.profile, &file_of(first), plain, true)?;
                for (other, plain) in rest {
                    e2e_store::write_sealed(&store, WHAT, &self.profile, &file_of(other), plain, false)?;
                }
                self.dirty.clear();
            }
            // No copies left: the key goes too.
            None => e2e_store::delete_ring(WHAT, &self.profile),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(text: &str, files: Vec<FileRef>) -> Opened {
        Opened { text: text.into(), files, stale: false, version: 2, epoch: 7, sent_at: 42 }
    }

    #[test]
    fn entries_round_trip_with_their_files() {
        let mut c = Copies::new(None, "main");
        let file = FileRef { id: [0xab; 16], key: Zeroizing::new([7u8; 32]) };
        c.put("200", "900", "200", &opened("hei", vec![file.clone()]));
        c.put("200", "901", "100", &opened("moi", vec![]));
        let back = decode(&encode(&c.loaded["200"])).unwrap();
        assert_eq!(back[&900].files, vec![file]);
        assert_eq!(back[&901].text, "moi");
        assert_eq!(back[&900].sent_at, 42);
        assert_eq!((back[&900].sender.as_str(), back[&901].sender.as_str()), ("200", "100"));
        assert!(decode(br#"{"1":{"t":"a","f":[]}}"#).is_none(), "every entry names its sender");
        assert!(decode(br#"{"x":{"s":"1","t":"a","f":[]}}"#).is_none(), "ids are numbers");
        assert!(decode(br#"{"1":{"s":"1","t":"a","f":[["zz","AA"]]}}"#).is_none());
    }

    #[test]
    fn edits_overwrite_and_deletes_remove() {
        let mut c = Copies::new(None, "main");
        c.put("200", "900", "200", &opened("first", vec![]));
        c.put("200", "900", "200", &opened("edited", vec![]));
        assert_eq!(c.get("200", "900", "200").unwrap().text, "edited");
        c.remove("200", "900");
        assert!(c.get("200", "900", "200").is_none());
        assert!(c.get("x", "900", "200").is_none(), "not a user id");
    }

    #[test]
    fn a_kept_message_comes_back_only_under_its_sender() {
        let mut c = Copies::new(None, "main");
        c.put("200", "900", "100", &opened("mine", vec![]));
        assert_eq!(c.get("200", "900", "100").unwrap().text, "mine");
        assert!(c.get("200", "900", "200").is_none(), "the same message shown as theirs: nothing");
        c.put("200", "901", "../x", &opened("y", vec![]));
        assert!(c.get("200", "901", "../x").is_none(), "senders are user ids");
    }

    #[test]
    fn peer_ids_that_arent_user_ids_never_reach_a_file_name() {
        let mut c = Copies::new(None, "main");
        for bad in [r"..\..\x", "../x", "", "12a", "123456789012345678901"] {
            assert!(c.erase(bad, &[]).is_err(), "{bad}");
            c.put(bad, "900", "200", &opened("x", vec![]));
            assert!(c.get(bad, "900", "200").is_none());
        }
        assert!(c.erase("200", &["../x".to_owned()]).is_ok());
    }

    #[test]
    fn the_copy_is_capped() {
        let mut c = Copies::new(None, "main");
        for i in 0..(MAX_ENTRIES + 5) {
            c.put("200", &(1000 + i).to_string(), "200", &opened("x", vec![]));
        }
        assert_eq!(c.loaded["200"].len(), MAX_ENTRIES);
        assert!(c.get("200", "1000", "200").is_none(), "the oldest went first");
    }

    #[test]
    fn a_sealed_copy_opens_only_with_its_key() {
        let ring = e2e_store::Ring::fresh().unwrap();
        let mut entries = BTreeMap::new();
        entries.insert(5u64, Entry { sender: "200".into(), text: "secret".into(), sent_at: 1, files: vec![] });
        let sealed = e2e_store::seal_blob(&ring, WHAT, "main", &encode(&entries)).unwrap();
        let plain = e2e_store::open_blob(&ring, WHAT, "main", &sealed).unwrap();
        assert_eq!(decode(&plain).unwrap(), entries);
        assert!(!sealed.windows(6).any(|w| w == b"secret"));
        let rotated = ring.rotated().unwrap().settled();
        assert!(e2e_store::open_blob(&rotated, WHAT, "main", &sealed).is_err(), "after erasing, a leftover can't be opened");
    }
}
