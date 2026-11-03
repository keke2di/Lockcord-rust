//! The Privacy page's modules that live in Rust (the settings come cleaned from settings.rs):
//!
//!   shield     the main window is left out of screen capture (screenshots, recorders, screen
//!              sharing and Snipping Tool show what's behind it): while an encrypted DM or a live
//!              secret chat is on screen ("secure", the default), always, or never.
//!   ghost      no typing indicator, and no read marks sent to Discord, everywhere or in the chats
//!              listed. Unreads still clear here; other devices keep them.
//!   clipboard  text copied from a secret chat or an encrypted DM stays out of Windows' clipboard
//!              history and cloud clipboard, and is cleared after 30 seconds unless something else
//!              was copied since.
//!
//! Cleaning what's sent (photo metadata, tracking parameters) is clean.rs; the app lock is lock.rs.

use serde_json::Value;
use std::collections::HashSet;

pub const CLIPBOARD_CLEAR_SECS: u64 = 30;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shield {
    Secure,
    Always,
    Off,
}

#[derive(Clone, Debug)]
pub struct Privacy {
    pub shield: Shield,
    pub ghost_typing: bool,
    pub ghost_read: bool,
    pub ghost_chats: HashSet<String>,
    pub clean_uploads: bool,
    pub generic_names: bool,
    pub clean_links: bool,
    pub no_embeds: bool,
    pub clipboard: bool,
}

impl Default for Privacy {
    fn default() -> Self {
        Privacy::from(&Value::Null)
    }
}

impl Privacy {
    /// From settings.json's cleaned `privacy` section (missing values take the defaults).
    pub fn from(v: &Value) -> Privacy {
        let flag = |k: &str, default: bool| v[k].as_bool().unwrap_or(default);
        Privacy {
            shield: match v["shield"].as_str() {
                Some("always") => Shield::Always,
                Some("off") => Shield::Off,
                _ => Shield::Secure,
            },
            ghost_typing: flag("ghostTyping", false),
            ghost_read: flag("ghostRead", false),
            ghost_chats: v["ghostChats"].as_array().map(|l| l.iter().filter_map(|c| c.as_str().map(str::to_owned)).collect()).unwrap_or_default(),
            clean_uploads: flag("cleanUploads", true),
            generic_names: flag("genericNames", false),
            clean_links: flag("cleanLinks", true),
            no_embeds: flag("noEmbeds", false),
            clipboard: flag("clipboard", true),
        }
    }

    /// Whether typing in this chat stays unannounced.
    pub fn ghost_typing(&self, channel: &str) -> bool {
        self.ghost_typing || self.ghost_chats.contains(channel)
    }

    /// Whether reading this chat stays unreported to Discord.
    pub fn ghost_read(&self, channel: &str) -> bool {
        self.ghost_read || self.ghost_chats.contains(channel)
    }

    /// Whether the window should be left out of capture now; `sensitive` is an encrypted DM or a
    /// live secret chat on screen.
    pub fn shield_on(&self, sensitive: bool) -> bool {
        match self.shield {
            Shield::Always => true,
            Shield::Secure => sensitive,
            Shield::Off => false,
        }
    }
}

// --- The shield ------------------------------------------------------------------------------------

/// Leaves the window out of screen capture, or lets it back in. Windows 10 2004 and later leave it
/// out entirely (WDA_EXCLUDEFROMCAPTURE); older ones fall back to WDA_MONITOR (it shows black).
#[cfg(windows)]
pub fn set_capture_shield(window: &tauri::WebviewWindow, on: bool) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE, WDA_MONITOR, WDA_NONE};
    let hwnd = window.hwnd().map_err(|e| e.to_string())?.0 as windows_sys::Win32::Foundation::HWND;
    // SAFETY: the handle is this process's own live window.
    let done = unsafe {
        if on {
            SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) != 0 || SetWindowDisplayAffinity(hwnd, WDA_MONITOR) != 0
        } else {
            SetWindowDisplayAffinity(hwnd, WDA_NONE) != 0
        }
    };
    if done { Ok(()) } else { Err("the capture shield couldn't be changed".into()) }
}

/// macOS: the window's sharing type (NSWindowSharingNone keeps it out of screenshots and most
/// screen recording; ScreenCaptureKit on macOS 15 and later may still see it).
#[cfg(target_os = "macos")]
pub fn set_capture_shield(window: &tauri::WebviewWindow, on: bool) -> Result<(), String> {
    use objc2_app_kit::{NSWindow, NSWindowSharingType};
    let w = window.clone();
    window.run_on_main_thread(move || {
        let Ok(ptr) = w.ns_window() else { return };
        // SAFETY: the pointer is this process's own live NSWindow, used on the main thread.
        let ns = unsafe { &*ptr.cast::<NSWindow>() };
        #[allow(unused_unsafe)]
        unsafe { ns.setSharingType(if on { NSWindowSharingType::None } else { NSWindowSharingType::ReadOnly }) };
    }).map_err(|e| e.to_string())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn set_capture_shield(_window: &tauri::WebviewWindow, _on: bool) -> Result<(), String> {
    Ok(())
}

// --- The clipboard -------------------------------------------------------------------------------------

/// Puts text on the clipboard. Sensitive text is marked so Windows' clipboard history, the cloud
/// clipboard and clipboard monitors leave it alone. Returns the clipboard's sequence number after
/// the copy, for `clear_clipboard_if`.
#[cfg(windows)]
pub fn copy_text(text: &str, sensitive: bool) -> Result<u32, String> {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData};
    const CF_UNICODETEXT: u32 = 13;
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    // Another program may hold the clipboard for a moment.
    let mut open = false;
    for _ in 0..10 {
        // SAFETY: plain Win32 call; no owner window.
        if unsafe { OpenClipboard(std::ptr::null_mut()) } != 0 {
            open = true;
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    if !open {
        return Err("The clipboard is busy.".into());
    }
    // SAFETY: the clipboard is open; each block is handed to it, which then owns it.
    let result = unsafe {
        EmptyClipboard();
        let mut ok = set_global(CF_UNICODETEXT, bytes_of(&wide), SetClipboardData);
        if ok && sensitive {
            let zero = 0u32.to_le_bytes();
            for name in ["ExcludeClipboardContentFromMonitorProcessing", "CanIncludeInClipboardHistory", "CanUploadToCloudClipboard"] {
                let wname: Vec<u16> = name.encode_utf16().chain(std::iter::once(0)).collect();
                let format = RegisterClipboardFormatW(wname.as_ptr());
                ok &= format != 0 && set_global(format, &zero, SetClipboardData);
            }
        }
        CloseClipboard();
        ok
    };
    if !result {
        return Err("Couldn't copy.".into());
    }
    // SAFETY: plain Win32 call.
    Ok(unsafe { GetClipboardSequenceNumber() })
}

#[cfg(windows)]
fn bytes_of(wide: &[u16]) -> &[u8] {
    // SAFETY: u16 has no padding; the slice covers exactly the same memory.
    unsafe { std::slice::from_raw_parts(wide.as_ptr().cast::<u8>(), wide.len() * 2) }
}

// One clipboard format's data in a movable global block, as the clipboard wants it.
#[cfg(windows)]
unsafe fn set_global(format: u32, bytes: &[u8], set: unsafe extern "system" fn(u32, windows_sys::Win32::Foundation::HANDLE) -> windows_sys::Win32::Foundation::HANDLE) -> bool {
    use windows_sys::Win32::Foundation::GlobalFree;
    use windows_sys::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
    // SAFETY: (the caller has the clipboard open) the block is sized for the bytes, written while
    // locked, and freed only if the clipboard didn't take it.
    unsafe {
        let block = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1));
        if block.is_null() {
            return false;
        }
        let ptr = GlobalLock(block).cast::<u8>();
        if ptr.is_null() {
            GlobalFree(block);
            return false;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        GlobalUnlock(block);
        if set(format, block).is_null() {
            GlobalFree(block);
            return false;
        }
        true
    }
}

/// Empties the clipboard if nothing was copied since `sequence` (what `copy_text` returned).
#[cfg(windows)]
pub fn clear_clipboard_if(sequence: u32) {
    use windows_sys::Win32::System::DataExchange::{CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard};
    // SAFETY: plain Win32 calls; the clipboard is closed again after emptying it.
    unsafe {
        if GetClipboardSequenceNumber() != sequence || OpenClipboard(std::ptr::null_mut()) == 0 {
            return;
        }
        if GetClipboardSequenceNumber() == sequence {
            EmptyClipboard();
        }
        CloseClipboard();
    }
}

/// macOS: the general pasteboard. Sensitive text carries the nspasteboard.org markers that
/// clipboard managers and Universal Clipboard honour (concealed, transient). The returned number is
/// the pasteboard's change count.
#[cfg(target_os = "macos")]
pub fn copy_text(text: &str, sensitive: bool) -> Result<u32, String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::NSString;
    #[allow(unused_unsafe)]
    // SAFETY: plain AppKit calls on the shared pasteboard with valid strings.
    unsafe {
        let board = NSPasteboard::generalPasteboard();
        board.clearContents();
        let mut ok = board.setString_forType(&NSString::from_str(text), NSPasteboardTypeString);
        if ok && sensitive {
            for marker in ["org.nspasteboard.ConcealedType", "org.nspasteboard.TransientType"] {
                ok &= board.setString_forType(&NSString::from_str(""), &NSString::from_str(marker));
            }
        }
        if !ok {
            return Err("Couldn't copy.".into());
        }
        Ok(board.changeCount() as u32)
    }
}

#[cfg(target_os = "macos")]
pub fn clear_clipboard_if(sequence: u32) {
    use objc2_app_kit::NSPasteboard;
    #[allow(unused_unsafe)]
    // SAFETY: plain AppKit calls on the shared pasteboard.
    unsafe {
        let board = NSPasteboard::generalPasteboard();
        if board.changeCount() as u32 == sequence {
            board.clearContents();
        }
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn copy_text(_text: &str, _sensitive: bool) -> Result<u32, String> {
    Err("Copying isn't available here.".into())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn clear_clipboard_if(_sequence: u32) {}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ghost_mode_applies_everywhere_or_per_chat() {
        let p = Privacy::from(&json!({ "ghostChats": ["10"] }));
        assert!(p.ghost_typing("10") && p.ghost_read("10"));
        assert!(!p.ghost_typing("11") && !p.ghost_read("11"));
        let p = Privacy::from(&json!({ "ghostTyping": true }));
        assert!(p.ghost_typing("11") && !p.ghost_read("11"));
    }

    #[test]
    fn the_shield_follows_its_mode() {
        let secure = Privacy::default();
        assert!(secure.shield_on(true) && !secure.shield_on(false));
        assert!(Privacy::from(&json!({ "shield": "always" })).shield_on(false));
        assert!(!Privacy::from(&json!({ "shield": "off" })).shield_on(true));
        assert!(secure.clean_uploads && secure.clean_links && !secure.no_embeds && secure.clipboard);
    }
}
