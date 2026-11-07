//! Encrypted files for encrypted DMs (v2). A file is encrypted here, in Rust, before it goes to
//! Discord's bucket, and uploaded as an opaque "<32 hex>.bin": Discord sees a random name and a
//! padded size, nothing else. Its random key travels inside the sealed message (e2e.rs frame v2),
//! bound to that random name, and it's decrypted here again for showing (`lcmedia:` with an
//! `lcenc:` path) and for Save media. Keys never reach the window.
//!
//!   plain     [u32 header length][header JSON: name, type, size][the file][zero padding to a
//!             Padmé size, which leaks at most O(log log n) bits of the length]
//!   sealed    64 KiB chunks, each AES-256-GCM under the file's key with the nonce
//!             0⁷ ‖ u32 chunk index ‖ last-chunk flag (STREAM: no chunk can be dropped, reordered
//!             or cut off unseen), the authenticated data "LCF2" ‖ the file's random id.

use crate::e2e::FileRef;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use serde_json::json;
use zeroize::Zeroizing;

pub const CHUNK: usize = 64 * 1024;
const TAG: usize = 16;
const MAX_HEADER: usize = 1024;
// Small files are padded to at least this, so a tiny one doesn't stand out.
const MIN_PADDED: u64 = 1024;

#[derive(Debug, PartialEq)]
pub struct Meta {
    pub name: String,
    /// The type the sender's LockCord named (informational only; `sniff` decides display).
    pub kind: String,
    pub size: u64,
}

/// Padmé (Nikitin et al., 2019): the padded length.
pub fn padme(len: u64) -> u64 {
    if len < 2 {
        return len;
    }
    let e = 63 - u64::from(len.leading_zeros());
    let s = 64 - u64::from(e.leading_zeros());
    let mask = (1u64 << (e - s)) - 1;
    (len + mask) & !mask
}

/// The random name a sealed file is uploaded under.
pub fn upload_name(id: &[u8; 16]) -> String {
    format!("{}.bin", crate::e2e::hex16(id))
}

fn nonce(index: u32, last: bool) -> [u8; 12] {
    let mut n = [0u8; 12];
    n[7..11].copy_from_slice(&index.to_be_bytes());
    n[11] = u8::from(last);
    n
}

fn aad(id: &[u8; 16]) -> [u8; 20] {
    let mut a = [0u8; 20];
    a[..4].copy_from_slice(b"LCF2");
    a[4..].copy_from_slice(id);
    a
}

/// The file, sealed under a fresh random key and id: (the bytes to upload, its key for the frame).
pub fn encrypt(file: &[u8], name: &str, kind: &str) -> Result<(Vec<u8>, FileRef), String> {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(|e| e.to_string())?;
    let key = Zeroizing::new(crate::e2e::random32()?);
    let r = FileRef { id, key };
    Ok((encrypt_with(&r, file, name, kind)?, r))
}

fn encrypt_with(r: &FileRef, file: &[u8], name: &str, kind: &str) -> Result<Vec<u8>, String> {
    let name: String = name.chars().take(200).collect();
    let kind: String = kind.chars().take(100).collect();
    let header = json!({ "n": name, "t": kind, "s": file.len() }).to_string().into_bytes();
    let used = 4 + header.len() + file.len();
    let padded = usize::try_from(padme(used as u64).max(MIN_PADDED)).map_err(|_| "too large")?;
    let mut plain = Zeroizing::new(Vec::with_capacity(padded));
    plain.extend_from_slice(&(header.len() as u32).to_be_bytes());
    plain.extend_from_slice(&header);
    plain.extend_from_slice(file);
    plain.resize(padded, 0);
    let cipher = Aes256Gcm::new_from_slice(r.key.as_ref()).map_err(|e| e.to_string())?;
    let associated = aad(&r.id);
    let chunks: Vec<&[u8]> = plain.chunks(CHUNK).collect();
    let count = chunks.len();
    let mut out = Vec::with_capacity(padded + count * TAG);
    for (i, chunk) in chunks.into_iter().enumerate() {
        let index = u32::try_from(i).map_err(|_| "too large")?;
        let sealed = cipher.encrypt(Nonce::from_slice(&nonce(index, i + 1 == count)), Payload { msg: chunk, aad: &associated }).map_err(|_| "Encryption failed.")?;
        out.extend_from_slice(&sealed);
    }
    Ok(out)
}

/// The file back, with what its header says; refused if any chunk is changed, missing, reordered
/// or added, or the padding isn't zeros.
pub fn decrypt(sealed: &[u8], r: &FileRef) -> Result<(Meta, Zeroizing<Vec<u8>>), String> {
    let broken = || "This encrypted file is damaged or isn't the one the message names.".to_owned();
    if sealed.is_empty() {
        return Err(broken());
    }
    let cipher = Aes256Gcm::new_from_slice(r.key.as_ref()).map_err(|_| broken())?;
    let associated = aad(&r.id);
    let pieces: Vec<&[u8]> = sealed.chunks(CHUNK + TAG).collect();
    let count = pieces.len();
    let mut plain = Zeroizing::new(Vec::with_capacity(sealed.len()));
    for (i, piece) in pieces.into_iter().enumerate() {
        let index = u32::try_from(i).map_err(|_| broken())?;
        let opened = Zeroizing::new(cipher.decrypt(Nonce::from_slice(&nonce(index, i + 1 == count)), Payload { msg: piece, aad: &associated }).map_err(|_| broken())?);
        plain.extend_from_slice(&opened);
    }
    let hlen = u32::from_be_bytes(plain.get(..4).ok_or_else(broken)?.try_into().map_err(|_| broken())?) as usize;
    if hlen > MAX_HEADER {
        return Err(broken());
    }
    let header: serde_json::Value = serde_json::from_slice(plain.get(4..4 + hlen).ok_or_else(broken)?).map_err(|_| broken())?;
    let size = header["s"].as_u64().ok_or_else(broken)?;
    let start = 4 + hlen;
    let end = start.checked_add(usize::try_from(size).map_err(|_| broken())?).ok_or_else(broken)?;
    if end > plain.len() || plain[end..].iter().any(|b| *b != 0) {
        return Err(broken());
    }
    let meta = Meta { name: header["n"].as_str().unwrap_or("file").to_owned(), kind: header["t"].as_str().unwrap_or("").to_owned(), size };
    Ok((meta, Zeroizing::new(plain[start..end].to_vec())))
}

/// The type to show a decrypted file as, from its first bytes; None for anything else (a file
/// card, Save only).
pub fn sniff(bytes: &[u8]) -> Option<&'static str> {
    let at = |range: std::ops::Range<usize>| bytes.get(range).unwrap_or(&[]);
    if at(0..8) == b"\x89PNG\r\n\x1a\n" {
        Some("image/png")
    } else if at(0..3) == [0xFF, 0xD8, 0xFF] {
        Some("image/jpeg")
    } else if at(0..4) == b"GIF8" {
        Some("image/gif")
    } else if at(0..4) == b"RIFF" && at(8..12) == b"WEBP" {
        Some("image/webp")
    } else if at(4..8) == b"ftyp" {
        Some("video/mp4")
    } else if at(0..4) == [0x1A, 0x45, 0xDF, 0xA3] {
        Some("video/webm")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padme_pads_a_little_and_never_shrinks() {
        assert_eq!(padme(0), 0);
        assert_eq!(padme(1), 1);
        for n in [2u64, 3, 9, 100, 1000, 65_537, 1_000_000, 25 * 1024 * 1024 + 3] {
            let p = padme(n);
            assert!(p >= n, "{n}");
            assert!((p - n) as f64 / n as f64 <= 0.12, "at most ~12% for {n}: {p}");
        }
        // Lengths close together look the same.
        assert_eq!(padme(1_000_001), padme(1_000_100));
    }

    #[test]
    fn files_round_trip_across_chunks() {
        for size in [0usize, 1, 100, CHUNK - 10, CHUNK, CHUNK * 3 + 7] {
            let file: Vec<u8> = (0..size).map(|i| (i % 251) as u8).collect();
            let (sealed, r) = encrypt(&file, "photo.png", "image/png").unwrap();
            assert!(sealed.len() as u64 >= MIN_PADDED);
            let (meta, back) = decrypt(&sealed, &r).unwrap();
            assert_eq!(back.as_slice(), file.as_slice(), "{size}");
            assert_eq!((meta.name.as_str(), meta.kind.as_str(), meta.size), ("photo.png", "image/png", size as u64));
            assert_eq!(upload_name(&r.id).len(), 36);
        }
    }

    #[test]
    fn tampering_cutting_reordering_and_the_wrong_id_fail() {
        let file: Vec<u8> = (0..(CHUNK * 3)).map(|i| (i % 7) as u8).collect();
        let (sealed, r) = encrypt(&file, "a.bin", "").unwrap();
        let mut flipped = sealed.clone();
        flipped[10] ^= 1;
        assert!(decrypt(&flipped, &r).is_err(), "a changed byte");
        assert!(decrypt(&sealed[..sealed.len() - (CHUNK + TAG)], &r).is_err(), "the last chunk cut off");
        assert!(decrypt(&sealed[..CHUNK + TAG], &r).is_err(), "only the first chunk");
        let mut swapped = sealed[CHUNK + TAG..2 * (CHUNK + TAG)].to_vec();
        swapped.extend_from_slice(&sealed[..CHUNK + TAG]);
        swapped.extend_from_slice(&sealed[2 * (CHUNK + TAG)..]);
        assert!(decrypt(&swapped, &r).is_err(), "chunks reordered");
        let mut extra = sealed.clone();
        extra.extend_from_slice(&[0u8; 40]);
        assert!(decrypt(&extra, &r).is_err(), "bytes added");
        let other = FileRef { id: [9u8; 16], key: r.key.clone() };
        assert!(decrypt(&sealed, &other).is_err(), "the key under another file's id");
        let wrong = FileRef { id: r.id, key: Zeroizing::new([1u8; 32]) };
        assert!(decrypt(&sealed, &wrong).is_err());
        assert!(decrypt(&[], &r).is_err());
    }

    #[test]
    fn non_zero_padding_is_refused() {
        let r = FileRef { id: [3u8; 16], key: Zeroizing::new([4u8; 32]) };
        let mut sealed = encrypt_with(&r, b"hi", "x", "").unwrap();
        assert!(decrypt(&sealed, &r).is_ok());
        // Re-seal a plaintext whose padding isn't zeros: refused.
        let cipher = Aes256Gcm::new_from_slice(r.key.as_ref()).unwrap();
        let mut plain = cipher.decrypt(Nonce::from_slice(&nonce(0, true)), Payload { msg: &sealed, aad: &aad(&r.id) }).unwrap();
        let last = plain.len() - 1;
        plain[last] = 1;
        sealed = cipher.encrypt(Nonce::from_slice(&nonce(0, true)), Payload { msg: &plain, aad: &aad(&r.id) }).unwrap();
        assert!(decrypt(&sealed, &r).is_err());
    }

    #[test]
    fn display_types_come_from_the_bytes() {
        assert_eq!(sniff(b"\x89PNG\r\n\x1a\n...."), Some("image/png"));
        assert_eq!(sniff(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(sniff(b"GIF89a"), Some("image/gif"));
        assert_eq!(sniff(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42"), Some("video/mp4"));
        assert_eq!(sniff(&[0x1A, 0x45, 0xDF, 0xA3, 1]), Some("video/webm"));
        assert_eq!(sniff(b"<svg onload=alert(1)>"), None);
        assert_eq!(sniff(b"MZ\x90\0"), None);
        assert_eq!(sniff(b""), None);
    }
}
