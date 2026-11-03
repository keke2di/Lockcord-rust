//! LockCord on the desktop: the tray icon, closing to it, starting with Windows, notifications,
//! and saving a message's files.
//!
//! Saving follows lockcord-public's archive (`main/features/archive.js`): Discord's own hosts on
//! every hop (redirects included), names Windows can't trip over, a ".download" ending on anything
//! Windows would run or follow, and the Mark-of-the-Web, so SmartScreen and Office treat the file
//! as coming from the internet.

use std::fs;
use std::path::{Path, PathBuf};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

pub const MAX_SAVE: usize = 100 * 1024 * 1024;

const RISKY: [&str; 41] = [
    "exe", "com", "scr", "pif", "msi", "msp", "msix", "appx", "bat", "cmd", "ps1", "psm1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "hta",
    "cpl", "jar", "lnk", "url", "scf", "reg", "inf", "chm", "application", "appref-ms", "library-ms", "searchconnector-ms",
    "settingcontent-ms", "diagcab", "gadget", "xll", "iso", "img", "vhd", "vhdx", "dll", "sys",
];

// --- The tray --------------------------------------------------------------------------------------

/// The tray icon: a click opens the window, its menu opens it or quits.
pub fn tray(app: &AppHandle, main: &'static str) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open LockCord", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit LockCord", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id("lockcord")
        .tooltip("LockCord")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show(app, main),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(move |tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                show(tray.app_handle(), main);
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn show(app: &AppHandle, main: &str) {
    if let Some(w) = app.get_webview_window(main) {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

// --- Saving files ------------------------------------------------------------------------------------

/// A name Windows takes as it is: no path or forbidden characters, no leading or trailing dots, no
/// device name, and ".download" after anything Windows would run or follow.
pub fn safe_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out: String = base.chars().map(|c| if "<>:\"|?*".contains(c) || c.is_control() { '_' } else { c }).take(180).collect();
    out = out.trim_matches(|c: char| c == '.' || c == ' ').to_owned();
    if out.is_empty() {
        out = "file".into();
    }
    let stem = out.split('.').next().unwrap_or("").to_ascii_lowercase();
    let reserved = ["con", "prn", "aux", "nul"].contains(&stem.as_str())
        || ((stem.starts_with("com") || stem.starts_with("lpt")) && stem.len() == 4 && stem.as_bytes()[3].is_ascii_digit());
    if reserved {
        out.insert(0, '_');
    }
    let ext = Path::new(&out).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if RISKY.contains(&ext.as_str()) {
        out.push_str(".download");
    }
    out
}

/// Claims a free name in `dir` by creating the file exclusively: "image.png", "image (1).png"…
pub fn reserve(dir: &Path, name: &str) -> Result<(PathBuf, fs::File), String> {
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("file");
    let ext = path.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    for i in 0..1000 {
        let candidate = dir.join(if i == 0 { format!("{stem}{ext}") } else { format!("{stem} ({i}){ext}") });
        match fs::OpenOptions::new().write(true).create_new(true).open(&candidate) {
            Ok(file) => return Ok((candidate, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.to_string()),
        }
    }
    Err("no free file name".into())
}

/// The Mark-of-the-Web, as browsers write it, without the address (it would leave the channel's
/// attachment link on disk). On macOS that's the quarantine attribute, so Gatekeeper checks a
/// saved app or script before it first runs.
pub fn mark_from_internet(path: &Path) {
    if cfg!(windows) {
        let mut stream = path.as_os_str().to_owned();
        stream.push(":Zone.Identifier");
        let _ = fs::write(PathBuf::from(stream), "[ZoneTransfer]\r\nZoneId=3\r\n");
    }
    #[cfg(target_os = "macos")]
    quarantine(path);
}

#[cfg(target_os = "macos")]
fn quarantine(path: &Path) {
    use std::os::unix::ffi::OsStrExt;
    let Ok(file) = std::ffi::CString::new(path.as_os_str().as_bytes()) else { return };
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    // Downloaded (0x01) by a non-browser app (0x80), when, by whom; no origin URL.
    let value = format!("0081;{secs:08x};LockCord;");
    // SAFETY: both strings are valid for the call; the value is passed with its length.
    unsafe {
        libc::setxattr(file.as_ptr(), c"com.apple.quarantine".as_ptr(), value.as_ptr().cast(), value.len(), 0, 0);
    }
}

/// The default folder for saved files: Pictures\LockCord.
pub fn default_save_dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().picture_dir().ok().map(|p| p.join("LockCord"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_windows_wont_trip_over() {
        assert_eq!(safe_name("cat.png"), "cat.png");
        assert_eq!(safe_name("../../evil.png"), "evil.png");
        assert_eq!(safe_name("C:\\x\\a:b?.txt"), "a_b_.txt");
        assert_eq!(safe_name("setup.EXE"), "setup.EXE.download");
        assert_eq!(safe_name("notes.js"), "notes.js.download");
        assert_eq!(safe_name("con.txt"), "_con.txt");
        assert_eq!(safe_name("COM1"), "_COM1");
        assert_eq!(safe_name("computer.png"), "computer.png");
        assert_eq!(safe_name("..."), "file");
        assert_eq!(safe_name(" trailing. "), "trailing");
    }

    #[test]
    fn names_are_claimed_without_overwriting() {
        let dir = std::env::temp_dir().join(format!("lockcord-save-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let (a, _) = reserve(&dir, "x.png").unwrap();
        let (b, _) = reserve(&dir, "x.png").unwrap();
        assert_eq!(a.file_name().unwrap(), "x.png");
        assert_eq!(b.file_name().unwrap(), "x (1).png");
        let _ = fs::remove_dir_all(&dir);
    }
}
