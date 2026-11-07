//! Sealed files for encrypted DMs, in the profile folder: this side's one-time secrets
//! (`e2e/secrets.bin`, here) and the per-DM local copies (history.rs). Credential Manager can't hold
//! them (its entries are small), so each file is AES-256-GCM-sealed under a key that does live in
//! the vault (vault.rs: Credential Manager on Windows, the Keychain on macOS).
//!
//! When something is deleted for good (a one-time secret after its grace time, a local copy), the
//! file's key is replaced and the file sealed again: a leftover copy of the old file (an SSD keeps
//! old blocks around) then needs a key that no longer exists. The vault entry holds the new key
//! and, until the new file is in place, the previous one, so a crash halfway loses nothing:
//! "<gen>:<key>" or "<gen>:<key>;<gen>:<key>". The file says which generation sealed it.
//!
//!   file   "LCS1" ‖ u32 generation ‖ 12-byte nonce ‖ AES-256-GCM(content), the authenticated
//!          data naming what the file is, the profile and the generation.

use crate::e2e::Ephemeral;
use crate::settings::Store;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use std::collections::HashMap;
use zeroize::Zeroizing;

const MAGIC: &[u8; 4] = b"LCS1";
pub const SECRETS_FILE: &str = "e2e/secrets.bin";
/// A one-time secret is deleted this long after its epoch was replaced.
pub const GRACE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
/// A one-time key that never made it into a key message (the post failed) is retired after this.
pub const ORPHAN_MS: u64 = 24 * 60 * 60 * 1000;

/// A file key and its generation, plus the previous one while a new file is being put in place.
#[derive(Clone)]
pub struct Ring {
    pub current: (u32, Zeroizing<[u8; 32]>),
    pub previous: Option<(u32, Zeroizing<[u8; 32]>)>,
}

impl Ring {
    pub fn fresh() -> Result<Ring, String> {
        Ok(Ring { current: (1, Zeroizing::new(crate::e2e::random32()?)), previous: None })
    }

    fn encode(&self) -> Zeroizing<String> {
        let one = |(g, k): &(u32, Zeroizing<[u8; 32]>)| format!("{g}:{}", STANDARD.encode(k.as_ref()));
        Zeroizing::new(match &self.previous {
            Some(p) => format!("{};{}", one(&self.current), one(p)),
            None => one(&self.current),
        })
    }

    fn decode(raw: &str) -> Option<Ring> {
        let one = |s: &str| -> Option<(u32, Zeroizing<[u8; 32]>)> {
            let (g, k) = s.split_once(':')?;
            let bytes = Zeroizing::new(STANDARD.decode(k).ok()?);
            Some((g.parse().ok()?, Zeroizing::new(bytes.as_slice().try_into().ok()?)))
        };
        let mut parts = raw.split(';');
        let current = one(parts.next()?)?;
        let previous = match parts.next() {
            Some(p) => Some(one(p)?),
            None => None,
        };
        if parts.next().is_some() {
            return None;
        }
        Some(Ring { current, previous })
    }

    fn key_for(&self, generation: u32) -> Option<&Zeroizing<[u8; 32]>> {
        if self.current.0 == generation {
            return Some(&self.current.1);
        }
        self.previous.as_ref().filter(|p| p.0 == generation).map(|p| &p.1)
    }

    /// The next generation, with this one kept as the previous until `settle`.
    pub fn rotated(&self) -> Result<Ring, String> {
        Ok(Ring { current: (self.current.0.wrapping_add(1), Zeroizing::new(crate::e2e::random32()?)), previous: Some(self.current.clone()) })
    }

    pub fn settled(&self) -> Ring {
        Ring { current: self.current.clone(), previous: None }
    }
}

fn aad(what: &str, profile: &str, generation: u32) -> String {
    format!("LockCord {what} v1|{profile}|{generation}")
}

pub fn seal_blob(ring: &Ring, what: &str, profile: &str, plain: &[u8]) -> Result<Vec<u8>, String> {
    let (generation, key) = &ring.current;
    let mut nonce = [0u8; 12];
    getrandom::fill(&mut nonce).map_err(|e| e.to_string())?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|e| e.to_string())?;
    let associated = aad(what, profile, *generation);
    let sealed = cipher.encrypt(Nonce::from_slice(&nonce), Payload { msg: plain, aad: associated.as_bytes() }).map_err(|_| "Encryption failed.".to_owned())?;
    let mut out = Vec::with_capacity(4 + 4 + 12 + sealed.len());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&generation.to_be_bytes());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&sealed);
    Ok(out)
}

pub fn open_blob(ring: &Ring, what: &str, profile: &str, bytes: &[u8]) -> Result<Zeroizing<Vec<u8>>, String> {
    let broken = || format!("The encrypted {what} file can't be opened (changed outside LockCord, or its key is gone).");
    if bytes.len() < 4 + 4 + 12 + 16 || &bytes[..4] != MAGIC {
        return Err(broken());
    }
    let generation = u32::from_be_bytes(bytes[4..8].try_into().map_err(|_| broken())?);
    let key = ring.key_for(generation).ok_or_else(broken)?;
    let cipher = Aes256Gcm::new_from_slice(key.as_ref()).map_err(|_| broken())?;
    let associated = aad(what, profile, generation);
    cipher.decrypt(Nonce::from_slice(&bytes[8..20]), Payload { msg: &bytes[20..], aad: associated.as_bytes() }).map(Zeroizing::new).map_err(|_| broken())
}

// --- The vault side -----------------------------------------------------------------------------

fn vault_name(what: &str, profile: &str) -> String {
    format!("e2e-{what}:{profile}")
}

pub fn load_ring(what: &str, profile: &str) -> Option<Ring> {
    Ring::decode(&crate::vault::get(&vault_name(what, profile))?)
}

fn save_ring(what: &str, profile: &str, ring: &Ring) -> Result<(), String> {
    crate::vault::set(&vault_name(what, profile), &ring.encode())
}

pub fn delete_ring(what: &str, profile: &str) {
    crate::vault::delete(&vault_name(what, profile));
}

/// Seals `plain` into `file`, with a new key when `rotate` (something was deleted for good). The
/// order keeps every moment readable: the vault learns the new key (keeping the old), the file is
/// replaced, then the old key goes.
pub fn write_sealed(store: &Store, what: &str, profile: &str, file: &str, plain: &[u8], rotate: bool) -> Result<(), String> {
    let ring = match load_ring(what, profile) {
        Some(r) if rotate => {
            let next = r.settled().rotated()?;
            save_ring(what, profile, &next)?;
            next
        }
        Some(r) => r,
        None => {
            let fresh = Ring::fresh()?;
            save_ring(what, profile, &fresh)?;
            fresh
        }
    };
    store.write_bytes(file, &seal_blob(&ring, what, profile, plain)?)?;
    if ring.previous.is_some() {
        save_ring(what, profile, &ring.settled())?;
    }
    Ok(())
}

/// None when there's no file; an error when there is one that won't open.
pub fn read_sealed(store: &Store, what: &str, profile: &str, file: &str) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    let Some(bytes) = store.read_bytes(file) else { return Ok(None) };
    let ring = load_ring(what, profile).ok_or_else(|| format!("The key for the encrypted {what} file is missing from {}.", if cfg!(target_os = "macos") { "the macOS Keychain" } else { "Windows Credential Manager" }))?;
    open_blob(&ring, what, profile, &bytes).map(Some)
}

// --- One-time secrets -----------------------------------------------------------------------------

pub fn encode_secrets(map: &HashMap<[u8; 32], Ephemeral>) -> Zeroizing<Vec<u8>> {
    let mut list: Vec<(&[u8; 32], &Ephemeral)> = map.iter().collect();
    list.sort_by_key(|(k, _)| **k);
    let body: Vec<Value> = list.iter().map(|(k, e)| json!({ "pub": STANDARD.encode(k), "secret": STANDARD.encode(e.secret.as_ref()), "made": e.made, "retired": e.retired })).collect();
    Zeroizing::new(json!(body).to_string().into_bytes())
}

pub fn decode_secrets(bytes: &[u8]) -> Option<HashMap<[u8; 32], Ephemeral>> {
    let v: Value = serde_json::from_slice(bytes).ok()?;
    let mut map = HashMap::new();
    for e in v.as_array()? {
        let public: [u8; 32] = STANDARD.decode(e["pub"].as_str()?).ok()?.try_into().ok()?;
        let raw = Zeroizing::new(STANDARD.decode(e["secret"].as_str()?).ok()?);
        let secret = Zeroizing::new(<[u8; 32]>::try_from(raw.as_slice()).ok()?);
        map.insert(public, Ephemeral { secret, made: e["made"].as_u64().unwrap_or(0), retired: e["retired"].as_u64() });
    }
    Some(map)
}

pub fn load_secrets(store: &Store, profile: &str) -> Result<HashMap<[u8; 32], Ephemeral>, String> {
    match read_sealed(store, "secrets", profile, SECRETS_FILE)? {
        Some(plain) => decode_secrets(&plain).ok_or_else(|| "The encryption secrets file is damaged.".to_owned()),
        None => Ok(HashMap::new()),
    }
}

/// Saved, with a new file key when a secret was deleted for good (`rotate`).
pub fn save_secrets(store: &Store, profile: &str, map: &HashMap<[u8; 32], Ephemeral>, rotate: bool) -> Result<(), String> {
    write_sealed(store, "secrets", profile, SECRETS_FILE, &encode_secrets(map), rotate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_file_opens_with_its_generation_and_nothing_else() {
        let ring = Ring::fresh().unwrap();
        let sealed = seal_blob(&ring, "secrets", "main", b"hello").unwrap();
        assert_eq!(open_blob(&ring, "secrets", "main", &sealed).unwrap().as_slice(), b"hello");
        assert!(open_blob(&ring, "secrets", "alt", &sealed).is_err(), "another profile's file");
        assert!(open_blob(&ring, "history", "main", &sealed).is_err(), "another kind of file");
        let mut tampered = sealed.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 1;
        assert!(open_blob(&ring, "secrets", "main", &tampered).is_err());
        assert!(open_blob(&Ring::fresh().unwrap(), "secrets", "main", &sealed).is_err(), "another key");
    }

    #[test]
    fn rotating_keeps_every_step_readable_and_then_forgets_the_old_file() {
        let old = Ring::fresh().unwrap();
        let old_file = seal_blob(&old, "secrets", "main", b"before").unwrap();
        // Step 1: the vault holds the new key and the old one.
        let both = old.rotated().unwrap();
        let both = Ring::decode(&both.encode()).unwrap();
        assert_eq!(open_blob(&both, "secrets", "main", &old_file).unwrap().as_slice(), b"before", "a crash before the new file is written");
        // Step 2: the new file is written with the new key.
        let new_file = seal_blob(&both, "secrets", "main", b"after").unwrap();
        assert_eq!(open_blob(&both, "secrets", "main", &new_file).unwrap().as_slice(), b"after", "a crash before the old key goes");
        // Step 3: the old key goes; a leftover old file can't be opened any more.
        let settled = Ring::decode(&both.settled().encode()).unwrap();
        assert!(settled.previous.is_none());
        assert_eq!(open_blob(&settled, "secrets", "main", &new_file).unwrap().as_slice(), b"after");
        assert!(open_blob(&settled, "secrets", "main", &old_file).is_err());
    }

    #[test]
    fn the_vault_value_parses_strictly() {
        let ring = Ring::fresh().unwrap().rotated().unwrap();
        let back = Ring::decode(&ring.encode()).unwrap();
        assert_eq!(back.current.0, 2);
        assert_eq!(back.previous.as_ref().unwrap().0, 1);
        for bad in ["", "x", "1:abc", "1:AAAA;", "1:x;2:y;3:z"] {
            assert!(Ring::decode(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn secrets_round_trip() {
        let mut map = HashMap::new();
        map.insert([1u8; 32], Ephemeral { secret: Zeroizing::new([9u8; 32]), made: 5, retired: None });
        map.insert([2u8; 32], Ephemeral { secret: Zeroizing::new([8u8; 32]), made: 6, retired: Some(7) });
        let back = decode_secrets(&encode_secrets(&map)).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(*back[&[1u8; 32]].secret, [9u8; 32]);
        assert_eq!(back[&[2u8; 32]].retired, Some(7));
        assert!(decode_secrets(b"[{\"pub\":\"AA\"}]").is_none());
    }
}
