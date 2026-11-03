//! The secrets this computer keeps for a profile (the Discord session, the encrypted-DM identity
//! and the list of encrypted DMs).
//!
//! On Windows they're in Credential Manager, but only on this machine: persisted as
//! CRED_PERSIST_LOCAL_MACHINE, never with the enterprise (roaming) persistence the keyring crate
//! hard-codes, and each value wrapped by DPAPI for the current Windows user first, so a copied
//! credential blob is no use elsewhere. Other programs running as the same Windows user can still
//! unwrap it; that's documented in the README. Values stored by earlier versions through the
//! keyring crate are moved there the first time they're read, and the old entry deleted.
//!
//! On macOS they're items in the user's login Keychain (through the keyring crate), which only
//! this app's code signature may read without asking.

#[cfg(target_os = "macos")]
pub use mac::{delete, get, set};

#[cfg(target_os = "macos")]
mod mac {
    use zeroize::Zeroizing;

    const SERVICE: &str = "chat.lockcord.desktop";

    fn entry(name: &str) -> Result<keyring::Entry, String> {
        keyring::Entry::new(SERVICE, name).map_err(|e| format!("Couldn't open the macOS Keychain: {e}"))
    }

    /// Keeps `secret` under `name` ("discord-session:<profile>" and the like).
    pub fn set(name: &str, secret: &str) -> Result<(), String> {
        entry(name)?.set_password(secret).map_err(|e| format!("Couldn't write to the macOS Keychain: {e}"))
    }

    pub fn get(name: &str) -> Option<Zeroizing<String>> {
        entry(name).ok()?.get_password().ok().map(Zeroizing::new)
    }

    pub fn delete(name: &str) {
        if let Ok(e) = entry(name) {
            let _ = e.delete_credential();
        }
    }
}

#[cfg(windows)]
pub use win::{delete, get, set};

#[cfg(windows)]
mod win {
    use windows_sys::Win32::Foundation::{FILETIME, LocalFree};
    use windows_sys::Win32::Security::Credentials::{CRED_MAX_CREDENTIAL_BLOB_SIZE, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredDeleteW, CredFree, CredReadW, CredWriteW};
    use windows_sys::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptProtectData, CryptUnprotectData};
    use zeroize::Zeroizing;

    // The keyring crate's service name for the entries this replaces.
    const OLD_SERVICE: &str = "LockCord";
    const ENTROPY: &[u8] = b"LockCord vault v1";

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn target(name: &str) -> Vec<u16> {
        wide(&format!("LockCord/{name}"))
    }

    fn blob(bytes: &[u8]) -> CRYPT_INTEGER_BLOB {
        CRYPT_INTEGER_BLOB { cbData: bytes.len() as u32, pbData: bytes.as_ptr() as *mut u8 }
    }

    fn protect(plain: &[u8]) -> Option<Vec<u8>> {
        let (input, entropy) = (blob(plain), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: the input blobs point at live slices for the call; DPAPI allocates `out` with
        // LocalAlloc, copied and then freed here.
        unsafe {
            if CryptProtectData(&input, std::ptr::null(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) == 0 {
                return None;
            }
            let sealed = std::slice::from_raw_parts(out.pbData, out.cbData as usize).to_vec();
            LocalFree(out.pbData as _);
            Some(sealed)
        }
    }

    fn unprotect(sealed: &[u8]) -> Option<Zeroizing<Vec<u8>>> {
        let (input, entropy) = (blob(sealed), blob(ENTROPY));
        let mut out = CRYPT_INTEGER_BLOB { cbData: 0, pbData: std::ptr::null_mut() };
        // SAFETY: as in `protect`; the plain text is copied into zeroizing memory and DPAPI's copy is
        // wiped before it's freed.
        unsafe {
            if CryptUnprotectData(&input, std::ptr::null_mut(), &entropy, std::ptr::null(), std::ptr::null(), CRYPTPROTECT_UI_FORBIDDEN, &mut out) == 0 {
                return None;
            }
            let plain = std::slice::from_raw_parts_mut(out.pbData, out.cbData as usize);
            let copy = Zeroizing::new(plain.to_vec());
            plain.fill(0);
            LocalFree(out.pbData as _);
            Some(copy)
        }
    }

    fn read(name: &str) -> Option<Zeroizing<String>> {
        let target = target(name);
        let mut found: *mut CREDENTIALW = std::ptr::null_mut();
        // SAFETY: `target` is NUL-terminated; a found credential is read, then freed with CredFree.
        let sealed = unsafe {
            if CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut found) == 0 || found.is_null() {
                return None;
            }
            let c = &*found;
            let bytes = std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize).to_vec();
            CredFree(found as _);
            bytes
        };
        let plain = unprotect(&sealed)?;
        String::from_utf8(plain.to_vec()).ok().map(Zeroizing::new)
    }

    /// Keeps `secret` under `name` ("discord-session:<profile>" and the like).
    pub fn set(name: &str, secret: &str) -> Result<(), String> {
        let sealed = protect(secret.as_bytes()).ok_or("Windows couldn't encrypt the secret (DPAPI).")?;
        if sealed.len() > CRED_MAX_CREDENTIAL_BLOB_SIZE as usize {
            return Err("That secret is too big for Credential Manager.".into());
        }
        let mut target = target(name);
        let mut user = wide("LockCord");
        let credential = CREDENTIALW {
            Flags: 0,
            Type: CRED_TYPE_GENERIC,
            TargetName: target.as_mut_ptr(),
            Comment: std::ptr::null_mut(),
            LastWritten: FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 },
            CredentialBlobSize: sealed.len() as u32,
            CredentialBlob: sealed.as_ptr() as *mut u8,
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            AttributeCount: 0,
            Attributes: std::ptr::null_mut(),
            TargetAlias: std::ptr::null_mut(),
            UserName: user.as_mut_ptr(),
        };
        // SAFETY: every pointer in `credential` points at buffers that outlive the call.
        if unsafe { CredWriteW(&credential, 0) } == 0 {
            return Err("Couldn't write to Windows Credential Manager.".into());
        }
        Ok(())
    }

    /// The secret under `name`, moving one kept by an older LockCord (keyring) here first.
    pub fn get(name: &str) -> Option<Zeroizing<String>> {
        if let Some(value) = read(name) {
            return Some(value);
        }
        let old = keyring::Entry::new(OLD_SERVICE, name).ok()?;
        let value = Zeroizing::new(old.get_password().ok()?);
        // Deleted only once it's safely here; otherwise the old one keeps working.
        if set(name, &value).is_ok() && read(name).is_some_and(|v| *v == *value) {
            let _ = old.delete_credential();
        }
        Some(value)
    }

    pub fn delete(name: &str) {
        let target = target(name);
        // SAFETY: `target` is NUL-terminated.
        unsafe {
            CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0);
        }
        if let Ok(old) = keyring::Entry::new(OLD_SERVICE, name) {
            let _ = old.delete_credential();
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn dpapi_round_trips_and_refuses_tampering() {
            let sealed = protect(b"not the real token").unwrap();
            assert!(!sealed.windows(4).any(|w| w == b"real"), "nothing readable in the blob");
            assert_eq!(&**unprotect(&sealed).unwrap(), b"not the real token");
            let mut broken = sealed.clone();
            let last = broken.len() - 1;
            broken[last] ^= 1;
            assert!(unprotect(&broken).is_none());
        }

        #[test]
        fn the_vault_keeps_reads_and_deletes() {
            let name = format!("test-vault:{}", std::process::id());
            set(&name, "secret value").unwrap();
            assert_eq!(get(&name).as_deref().map(String::as_str), Some("secret value"));
            delete(&name);
            assert!(read(&name).is_none());
        }

        #[test]
        fn an_old_keyring_entry_moves_in_once() {
            let name = format!("test-vault-old:{}", std::process::id());
            let old = keyring::Entry::new(OLD_SERVICE, &name).unwrap();
            old.set_password("from 1.0").unwrap();
            assert_eq!(get(&name).as_deref().map(String::as_str), Some("from 1.0"));
            assert!(read(&name).is_some(), "now in the vault");
            assert!(old.get_password().is_err(), "and the roaming entry is gone");
            delete(&name);
        }
    }
}
