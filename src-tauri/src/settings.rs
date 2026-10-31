//! Settings and themes, kept on disk per profile (the window is incognito and keeps nothing):
//!
//!   <app data>/profiles/<profile>/settings.json
//!   <app data>/profiles/<profile>/themes/<id>/theme.json   tokens, options, custom CSS
//!   <app data>/profiles/<profile>/themes/<id>/assets/<name> images and fonts, by their bytes
//!
//! Everything a theme carries is checked here before it's saved or served: tokens by kind (no
//! `url(`, no way out of a declaration), options from fixed lists, sizes capped, assets only
//! images and fonts. The window loads assets through the `lctheme:` scheme, never by path.

use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};

pub const SCHEME: &str = "lctheme";
pub const FORMAT: u64 = 1;
const MAX_CSS: usize = 256 * 1024;
const MAX_ASSET: usize = 10 * 1024 * 1024;
const MAX_ASSETS_TOTAL: usize = 20 * 1024 * 1024;
const MAX_ASSETS: usize = 16;
const MAX_THEMES: usize = 100;
// A packed theme file: its JSON plus its assets in base64.
const MAX_FILE: usize = 32 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Color,
    Length,
    Number,
    Font,
    Shadow,
}

// The tokens a theme may set (app.css's :root), and what each one holds.
const TOKENS: &[(&str, Kind)] = &[
    ("--lc-chat-bg", Kind::Color),
    ("--lc-text", Kind::Color),
    ("--lc-header", Kind::Color),
    ("--lc-muted", Kind::Color),
    ("--lc-interactive", Kind::Color),
    ("--lc-interactive-hover", Kind::Color),
    ("--lc-hover", Kind::Color),
    ("--lc-popout", Kind::Color),
    ("--lc-composer", Kind::Color),
    ("--lc-border", Kind::Color),
    ("--lc-brand", Kind::Color),
    ("--lc-link", Kind::Color),
    ("--lc-danger", Kind::Color),
    ("--lc-positive", Kind::Color),
    ("--lc-warning", Kind::Color),
    ("--lc-code-bg", Kind::Color),
    ("--lc-mention", Kind::Color),
    ("--lc-sidebar", Kind::Color),
    ("--lc-sidebar-hover", Kind::Color),
    ("--lc-sidebar-selected", Kind::Color),
    ("--lc-panel", Kind::Color),
    ("--lc-rail", Kind::Color),
    ("--lc-deep", Kind::Color),
    ("--lc-secret", Kind::Color),
    ("--lc-on-accent", Kind::Color),
    ("--lc-shade", Kind::Color),
    ("--lc-status-offline", Kind::Color),
    ("--lc-overlay-soft", Kind::Color),
    ("--lc-overlay", Kind::Color),
    ("--lc-scrim", Kind::Color),
    ("--lc-backdrop", Kind::Color),
    ("--lc-backdrop-dark", Kind::Color),
    ("--lc-card", Kind::Color),
    ("--lc-bubble", Kind::Color),
    ("--lc-bubble-mine", Kind::Color),
    ("--lc-shadow-low", Kind::Shadow),
    ("--lc-shadow-card", Kind::Shadow),
    ("--lc-shadow-popout", Kind::Shadow),
    ("--lc-shadow-header", Kind::Shadow),
    ("--lc-shadow-divider", Kind::Shadow),
    ("--lc-shadow-side", Kind::Shadow),
    ("--lc-font", Kind::Font),
    ("--lc-font-code", Kind::Font),
    ("--lc-font-size", Kind::Length),
    ("--lc-line-height", Kind::Number),
    ("--lc-radius-s", Kind::Length),
    ("--lc-radius-m", Kind::Length),
    ("--lc-radius-l", Kind::Length),
    ("--lc-avatar-radius", Kind::Length),
    ("--lc-sidebar-width", Kind::Length),
    ("--lc-members-width", Kind::Length),
    ("--lc-avatar-size", Kind::Length),
    ("--lc-group-gap", Kind::Length),
    ("--lc-motion", Kind::Number),
    ("--lc-glass-blur", Kind::Length),
    ("--lc-glass-alpha", Kind::Number),
    ("--lc-gradient-a", Kind::Color),
    ("--lc-gradient-b", Kind::Color),
    ("--lc-gradient-c", Kind::Color),
    ("--lc-gradient-speed", Kind::Number),
    ("--lc-background-dim", Kind::Number),
    ("--lc-glow", Kind::Length),
];

// Options and the values each may take; the first is the default.
const OPTIONS: &[(&str, &[&str])] = &[
    ("scheme", &["dark", "light"]),
    ("density", &["cozy", "compact"]),
    ("messages", &["flat", "cards", "bubbles"]),
    ("panels", &["solid", "glass"]),
    ("background", &["none", "image", "gradient", "animated"]),
    ("effects", &["on", "off"]),
    ("animations", &["on", "off"]),
];

// --- Checking ---------------------------------------------------------------------------------

// Characters that could end a declaration, start a block or rule, load something, or escape.
fn unsafe_css(v: &str) -> bool {
    let lower = v.to_ascii_lowercase();
    v.contains([';', '{', '}', '\\', '@', '<', '>', '!']) || lower.contains("url(") || lower.contains("/*") || lower.contains("expression") || lower.contains("var(")
}

fn number(v: &str) -> bool {
    !v.is_empty() && v.len() <= 12 && v.parse::<f64>().is_ok_and(|n| n.is_finite() && (0.0..=10_000.0).contains(&n))
}

fn length(v: &str) -> bool {
    if v == "0" {
        return true;
    }
    ["px", "rem", "em", "%"].iter().any(|unit| v.strip_suffix(unit).is_some_and(number))
}

fn color(v: &str) -> bool {
    if v == "transparent" {
        return true;
    }
    if let Some(hex) = v.strip_prefix('#') {
        return [3, 4, 6, 8].contains(&hex.len()) && hex.bytes().all(|b| b.is_ascii_hexdigit());
    }
    // rgb(), rgba(), hsl(), hsla(): numbers, %, commas, dots, spaces and a slash inside.
    ["rgb(", "rgba(", "hsl(", "hsla("].iter().any(|f| {
        v.strip_prefix(f).and_then(|rest| rest.strip_suffix(')')).is_some_and(|args| {
            !args.is_empty() && args.len() <= 60 && args.bytes().all(|b| b.is_ascii_digit() || b" .,%/-".contains(&b) || b == b'd' || b == b'e' || b == b'g')
        })
    })
}

fn font(v: &str) -> bool {
    !v.is_empty() && v.len() <= 300 && v.chars().all(|c| c.is_alphanumeric() || " ,'\"-_".contains(c))
}

fn shadow(v: &str) -> bool {
    v == "none" || (v.len() <= 300 && v.bytes().all(|b| b.is_ascii_alphanumeric() || b" .,()%#-".contains(&b)))
}

fn token_ok(kind: Kind, value: &str) -> bool {
    let value = value.trim();
    !unsafe_css(value)
        && match kind {
            Kind::Color => color(value),
            Kind::Length => length(value),
            Kind::Number => number(value),
            Kind::Font => font(value),
            Kind::Shadow => shadow(value),
        }
}

pub fn valid_id(id: &str) -> bool {
    (1..=48).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-') && !id.starts_with('-')
}

fn valid_asset_name(name: &str) -> bool {
    (1..=40).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'.') && !name.starts_with('.')
}

/// An asset's media type from its bytes: images and fonts only.
pub fn asset_type(bytes: &[u8]) -> Option<&'static str> {
    let starts = |magic: &[u8]| bytes.starts_with(magic);
    if starts(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if starts(&[0xff, 0xd8, 0xff]) {
        Some("image/jpeg")
    } else if starts(b"GIF87a") || starts(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() > 12 && starts(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else if starts(b"wOF2") {
        Some("font/woff2")
    } else if starts(b"wOFF") {
        Some("font/woff")
    } else if starts(&[0, 1, 0, 0]) || starts(b"true") {
        Some("font/ttf")
    } else if starts(b"OTTO") {
        Some("font/otf")
    } else {
        None
    }
}

fn text(v: &Value, max: usize) -> String {
    v.as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(max).collect()
}

/// A theme as the UI or a file gives it, checked and cut down to what LockCord understands.
/// Unknown tokens and options are dropped; a bad value is an error, so a broken theme isn't
/// half-applied.
pub fn clean_theme(raw: &Value) -> Result<Value, String> {
    if raw["format"].as_u64().unwrap_or(FORMAT) != FORMAT {
        return Err("That theme is for another version of LockCord.".into());
    }
    let name = text(&raw["name"], 60);
    if name.trim().is_empty() {
        return Err("A theme needs a name.".into());
    }
    let mut tokens = Map::new();
    for (key, value) in raw["tokens"].as_object().into_iter().flatten() {
        let Some((_, kind)) = TOKENS.iter().find(|(k, _)| k == key) else { continue };
        let value = value.as_str().ok_or_else(|| format!("{key} isn't text."))?;
        if !token_ok(*kind, value) {
            return Err(format!("{key} has a value LockCord can't use: {}", value.chars().take(40).collect::<String>()));
        }
        tokens.insert(key.clone(), json!(value.trim()));
    }
    let mut options = Map::new();
    for (key, allowed) in OPTIONS {
        let value = raw["options"][key].as_str().unwrap_or(allowed[0]);
        if !allowed.contains(&value) {
            return Err(format!("The {key} option can't be {value}."));
        }
        options.insert((*key).to_owned(), json!(value));
    }
    let css = raw["css"].as_str().unwrap_or("");
    if css.len() > MAX_CSS {
        return Err("The custom CSS is over 256 KB.".into());
    }
    let background = text(&raw["backgroundAsset"], 40);
    let font = text(&raw["fontAsset"], 40);
    if [&background, &font].iter().any(|n| !n.is_empty() && !valid_asset_name(n)) {
        return Err("Bad file name in the theme.".into());
    }
    Ok(json!({
        "format": FORMAT, "name": name.trim(), "author": text(&raw["author"], 60).trim(),
        "tokens": tokens, "options": options, "css": css, "backgroundAsset": background, "fontAsset": font,
    }))
}

// Translation for one channel (or the defaults): incoming and outgoing, each on/off and a
// target language.
fn clean_translation(raw: &Value) -> Value {
    let lang = |v: &Value| v.as_str().filter(|l| crate::translate::valid_lang(l) && *l != "auto").unwrap_or("en").to_owned();
    json!({
        "incoming": raw["incoming"].as_bool().unwrap_or(false), "incomingLang": lang(&raw["incomingLang"]),
        "outgoing": raw["outgoing"].as_bool().unwrap_or(false), "outgoingLang": lang(&raw["outgoingLang"]),
    })
}

/// Settings outside themes, from fixed lists.
pub fn clean_settings(raw: &Value) -> Value {
    let theme = raw["theme"].as_str().filter(|t| t.strip_prefix("builtin:").map_or(valid_id(t), valid_id)).unwrap_or("builtin:dark");
    json!({
        "theme": theme,
        "sidePinned": raw["sidePinned"].as_bool().unwrap_or(false),
        "memberList": raw["memberList"].as_bool().unwrap_or(true),
        "closeToTray": raw["closeToTray"].as_bool().unwrap_or(true),
        "autostart": raw["autostart"].as_bool().unwrap_or(false),
        "startHidden": raw["startHidden"].as_bool().unwrap_or(false),
        // The daily update check (update.rs): on unless turned off.
        "checkUpdates": raw["checkUpdates"].as_bool().unwrap_or(true),
        // Notifications: "off", "name" (who, not what) or "text" (the message too).
        "notify": raw["notify"].as_str().filter(|n| ["off", "name", "text"].contains(n)).unwrap_or("name"),
        "voice": clean_voice(&raw["voice"]),
        "translate": {
            "defaults": clean_translation(&raw["translate"]["defaults"]),
            "channels": raw["translate"]["channels"].as_object().map(|m| m.iter()
                .filter(|(id, _)| (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()))
                .take(500)
                .map(|(id, v)| (id.clone(), clean_translation(v)))
                .collect::<Map<String, Value>>()).unwrap_or_default(),
        },
    })
}

// A global shortcut as the window records it ("Ctrl+Shift+M"): at least one modifier and a key.
fn shortcut(v: &Value) -> &str {
    v.as_str().filter(|k| k.len() <= 40 && k.contains('+') && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'+')).unwrap_or("")
}

// Voice & Video (call.rs VoiceSettings): devices by their system ids, levels 0-200%, voice
// activity or push-to-talk (a KeyboardEvent code), the microphone's processing, and people's
// volumes.
fn clean_voice(raw: &Value) -> Value {
    let device = |k: &str| raw[k].as_str().filter(|d| !d.is_empty() && d.len() <= 512 && !d.contains(' ')).map(|d| json!(d)).unwrap_or(Value::Null);
    let pct = |k: &str| raw[k].as_u64().map(|p| p.min(200)).unwrap_or(100);
    let key = raw["pttKey"].as_str().filter(|k| !k.is_empty() && k.len() <= 32 && k.bytes().all(|b| b.is_ascii_alphanumeric())).unwrap_or("Backquote");
    let sensitivity = if raw.get("sensitivity").is_some_and(Value::is_null) { Value::Null } else { json!(raw["sensitivity"].as_i64().unwrap_or(-55).clamp(-80, 0)) };
    json!({
        "input": device("input"), "output": device("output"), "camera": device("camera"),
        "inputGain": pct("inputGain"), "outputGain": pct("outputGain"),
        "mode": if raw["mode"] == "ptt" { "ptt" } else { "vad" }, "pttKey": key,
        "muteKey": shortcut(&raw["muteKey"]), "deafenKey": shortcut(&raw["deafenKey"]),
        "sensitivity": sensitivity,
        "noise": raw["noise"].as_str().filter(|n| ["rnnoise", "webrtc", "off"].contains(n)).unwrap_or("rnnoise"),
        "echo": raw["echo"].as_bool().unwrap_or(true), "agc": raw["agc"].as_bool().unwrap_or(true),
        "volumes": raw["volumes"].as_object().map(|m| m.iter()
            .filter(|(id, v)| (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()) && v.is_u64())
            .take(64)
            .map(|(id, v)| (id.clone(), json!(v.as_u64().unwrap_or(100).min(200))))
            .collect::<Map<String, Value>>()).unwrap_or_default(),
    })
}

// --- On disk ----------------------------------------------------------------------------------

pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn new(app_data: &Path, profile: &str) -> Store {
        Store { root: app_data.join("profiles").join(profile) }
    }

    fn themes(&self) -> PathBuf {
        self.root.join("themes")
    }

    fn theme_dir(&self, id: &str) -> Result<PathBuf, String> {
        if !valid_id(id) {
            return Err("bad theme id".into());
        }
        Ok(self.themes().join(id))
    }

    /// A JSON file of the profile's own (guard.json, contacts.json); Null if there's none.
    pub fn read_json(&self, name: &str) -> Value {
        fs::read(self.root.join(name)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null)
    }

    pub fn write_json(&self, name: &str, value: &Value) -> Result<(), String> {
        write(&self.root.join(name), value.to_string().as_bytes())
    }

    pub fn settings(&self) -> Value {
        let raw = fs::read(self.root.join("settings.json")).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
        clean_settings(&raw)
    }

    pub fn save_settings(&self, raw: &Value) -> Result<Value, String> {
        let clean = clean_settings(raw);
        write(&self.root.join("settings.json"), clean.to_string().as_bytes())?;
        Ok(clean)
    }

    /// The user's themes: id, name, author.
    pub fn list(&self) -> Vec<Value> {
        let Ok(dirs) = fs::read_dir(self.themes()) else { return Vec::new() };
        let mut out: Vec<Value> = dirs
            .flatten()
            .filter_map(|d| {
                let id = d.file_name().into_string().ok().filter(|id| valid_id(id))?;
                let theme = self.theme(&id).ok()?;
                Some(json!({ "id": id, "name": theme["name"], "author": theme["author"] }))
            })
            .collect();
        out.sort_by(|a, b| a["name"].as_str().unwrap_or("").to_lowercase().cmp(&b["name"].as_str().unwrap_or("").to_lowercase()));
        out
    }

    /// A theme with its asset names (the files themselves come through the scheme).
    pub fn theme(&self, id: &str) -> Result<Value, String> {
        let dir = self.theme_dir(id)?;
        let raw: Value = serde_json::from_slice(&fs::read(dir.join("theme.json")).map_err(|_| "No such theme.")?).map_err(|_| "That theme's file is damaged.")?;
        let mut theme = clean_theme(&raw)?;
        theme["id"] = json!(id);
        theme["assets"] = json!(self.asset_names(id));
        Ok(theme)
    }

    /// Saves a theme; a new one (no valid id) gets one made from its name.
    pub fn save_theme(&self, raw: &Value) -> Result<Value, String> {
        let clean = clean_theme(raw)?;
        let id = match raw["id"].as_str().filter(|id| valid_id(id)) {
            Some(id) => id.to_owned(),
            None => {
                if self.list().len() >= MAX_THEMES {
                    return Err(format!("At most {MAX_THEMES} themes."));
                }
                new_id(clean["name"].as_str().unwrap_or("theme"))
            }
        };
        write(&self.theme_dir(&id)?.join("theme.json"), clean.to_string().as_bytes())?;
        self.theme(&id)
    }

    pub fn delete_theme(&self, id: &str) -> Result<(), String> {
        let dir = self.theme_dir(id)?;
        if dir.exists() {
            fs::remove_dir_all(dir).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn asset_names(&self, id: &str) -> Vec<String> {
        let Ok(dir) = self.theme_dir(id).map(|d| d.join("assets")) else { return Vec::new() };
        let mut names: Vec<String> = fs::read_dir(dir).into_iter().flatten().flatten().filter_map(|f| f.file_name().into_string().ok()).filter(|n| valid_asset_name(n)).collect();
        names.sort();
        names
    }

    /// Adds an image or font to a theme, by its bytes; returns the name it's stored under.
    pub fn add_asset(&self, id: &str, name: &str, bytes: &[u8]) -> Result<String, String> {
        let dir = self.theme_dir(id)?;
        if !dir.join("theme.json").exists() {
            return Err("Save the theme first.".into());
        }
        let kind = asset_type(bytes).ok_or("Only PNG, JPEG, GIF, WebP images and WOFF2, WOFF, TTF, OTF fonts.")?;
        if bytes.len() > MAX_ASSET {
            return Err("Files in a theme can be up to 10 MB.".into());
        }
        let names = self.asset_names(id);
        let total: u64 = names.iter().filter_map(|n| fs::metadata(dir.join("assets").join(n)).ok()).map(|m| m.len()).sum();
        if names.len() >= MAX_ASSETS || total as usize + bytes.len() > MAX_ASSETS_TOTAL {
            return Err("A theme can hold up to 16 files and 20 MB.".into());
        }
        let ext = kind.rsplit('/').next().unwrap_or("bin").replace("jpeg", "jpg");
        let base: String = name.rsplit(['/', '\\']).next().unwrap_or("").split('.').next().unwrap_or("").to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-').take(30).collect();
        let stem = if base.is_empty() { "file".to_owned() } else { base };
        let mut file = format!("{stem}.{ext}");
        let mut n = 2;
        while names.contains(&file) {
            file = format!("{stem}-{n}.{ext}");
            n += 1;
        }
        write(&dir.join("assets").join(&file), bytes)?;
        Ok(file)
    }

    pub fn remove_asset(&self, id: &str, name: &str) -> Result<(), String> {
        if !valid_asset_name(name) {
            return Err("bad asset name".into());
        }
        let path = self.theme_dir(id)?.join("assets").join(name);
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// For the scheme: `<theme id>/<asset name>`, with its media type.
    pub fn read_asset(&self, path: &str) -> Option<(&'static str, Vec<u8>)> {
        let (id, name) = path.trim_start_matches('/').split_once('/')?;
        if !valid_asset_name(name) {
            return None;
        }
        let bytes = fs::read(self.theme_dir(id).ok()?.join("assets").join(name)).ok()?;
        Some((asset_type(&bytes)?, bytes))
    }
}

// --- Theme files (.lctheme) -------------------------------------------------------------------------

impl Store {
    /// One file holding the theme and its assets (base64), to share.
    pub fn pack(&self, id: &str) -> Result<(String, Vec<u8>), String> {
        use base64::Engine;
        let mut theme = self.theme(id)?;
        let mut assets = Map::new();
        for name in self.asset_names(id) {
            let bytes = fs::read(self.theme_dir(id)?.join("assets").join(&name)).map_err(|e| e.to_string())?;
            assets.insert(name, json!(base64::engine::general_purpose::STANDARD.encode(bytes)));
        }
        theme["assets"] = Value::Object(assets);
        if let Some(t) = theme.as_object_mut() {
            t.remove("id");
        }
        let name = theme["name"].as_str().unwrap_or("theme").to_owned();
        Ok((name, serde_json::to_vec_pretty(&theme).map_err(|e| e.to_string())?))
    }

    /// A shared theme file, checked like any theme and every asset by its bytes, saved as a new
    /// theme (it never replaces one already here).
    pub fn unpack(&self, bytes: &[u8]) -> Result<Value, String> {
        use base64::Engine;
        if bytes.len() > MAX_FILE {
            return Err("That file is too big for a theme.".into());
        }
        let raw: Value = serde_json::from_slice(bytes).map_err(|_| "That isn't a LockCord theme file.")?;
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let mut total = 0;
        for (name, data) in raw["assets"].as_object().into_iter().flatten() {
            let data = data.as_str().ok_or("A file in the theme is damaged.")?;
            let decoded = base64::engine::general_purpose::STANDARD.decode(data).map_err(|_| "A file in the theme is damaged.")?;
            if !valid_asset_name(name) || asset_type(&decoded).is_none() || decoded.len() > MAX_ASSET {
                return Err(format!("The theme's file {name} isn't an image or font LockCord takes."));
            }
            total += decoded.len();
            files.push((name.clone(), decoded));
        }
        if files.len() > MAX_ASSETS || total > MAX_ASSETS_TOTAL {
            return Err("A theme can hold up to 16 files and 20 MB.".into());
        }
        let mut theme = clean_theme(&raw)?;
        if let Some(t) = theme.as_object_mut() {
            t.remove("id");
        }
        let saved = self.save_theme(&theme)?;
        let id = saved["id"].as_str().unwrap_or_default().to_owned();
        let dir = self.theme_dir(&id)?.join("assets");
        for (name, data) in files {
            if let Err(e) = write(&dir.join(&name), &data) {
                let _ = self.delete_theme(&id);
                return Err(e);
            }
        }
        // A background or font the theme names but doesn't carry is dropped.
        let names = self.asset_names(&id);
        let mut fixed = theme;
        for slot in ["backgroundAsset", "fontAsset"] {
            if fixed[slot].as_str().is_some_and(|n| !n.is_empty() && !names.iter().any(|x| x == n)) {
                fixed[slot] = json!("");
            }
        }
        fixed["id"] = json!(id);
        self.save_theme(&fixed)
    }
}

/// A file name for an exported theme.
pub fn file_name(name: &str) -> String {
    let base: String = name.chars().filter(|c| c.is_alphanumeric() || " -_".contains(*c)).take(60).collect();
    let base = base.trim();
    format!("{}.lctheme", if base.is_empty() { "theme" } else { base })
}

// Written whole to a temporary file first, so a crash never leaves half a file.
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("bad path")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

fn new_id(name: &str) -> String {
    let mut slug: String = name.to_lowercase().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect();
    slug = slug.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-");
    slug.truncate(36);
    let slug = slug.trim_end_matches('-');
    let mut random = [0u8; 3];
    let _ = getrandom::fill(&mut random);
    let suffix: String = random.iter().map(|b| format!("{b:02x}")).collect();
    if slug.is_empty() { format!("theme-{suffix}") } else { format!("{slug}-{suffix}") }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_checked_by_kind() {
        for good in ["#313338", "#fff", "#5865f2cc", "rgba(78, 80, 88, 0.48)", "hsl(220deg 10% 20%)", "transparent"] {
            assert!(token_ok(Kind::Color, good), "{good}");
        }
        for bad in ["red; background: url(x)", "#31333", "rgb(1,2,3", "url(https://x)", "var(--x)", "expression(alert(1))", "#fff}body{"] {
            assert!(!token_ok(Kind::Color, bad), "{bad}");
        }
        assert!(token_ok(Kind::Length, "16px") && token_ok(Kind::Length, "0.5rem") && token_ok(Kind::Length, "50%") && token_ok(Kind::Length, "0"));
        assert!(!token_ok(Kind::Length, "16") && !token_ok(Kind::Length, "calc(1px)") && !token_ok(Kind::Length, "-5px"));
        assert!(token_ok(Kind::Font, "'Inter', 'Segoe UI', sans-serif"));
        assert!(!token_ok(Kind::Font, "x; } * { color: red"));
        assert!(token_ok(Kind::Shadow, "0 8px 16px rgba(0, 0, 0, 0.24)") && token_ok(Kind::Shadow, "none"));
        assert!(!token_ok(Kind::Shadow, "0 0 url(x)"));
        assert!(token_ok(Kind::Number, "1.375") && !token_ok(Kind::Number, "NaN") && !token_ok(Kind::Number, "1e400"));
    }

    #[test]
    fn a_theme_keeps_what_it_may_and_refuses_bad_values() {
        let raw = json!({
            "name": "Neon", "author": "me", "tokens": { "--lc-brand": "#ff00ff", "--lc-unknown": "#000" },
            "options": { "messages": "bubbles" }, "css": ".x { color: red }",
        });
        let clean = clean_theme(&raw).unwrap();
        assert_eq!(clean["tokens"]["--lc-brand"], "#ff00ff");
        assert!(clean["tokens"].get("--lc-unknown").is_none());
        assert_eq!(clean["options"]["messages"], "bubbles");
        assert_eq!(clean["options"]["density"], "cozy");
        assert!(clean_theme(&json!({ "name": "X", "tokens": { "--lc-brand": "red;}" } })).is_err());
        assert!(clean_theme(&json!({ "name": "X", "options": { "panels": "evil" } })).is_err());
        assert!(clean_theme(&json!({ "name": " " })).is_err());
        assert!(clean_theme(&json!({ "name": "X", "format": 9 })).is_err());
        assert!(clean_theme(&json!({ "name": "X", "css": "a".repeat(MAX_CSS + 1) })).is_err());
    }

    #[test]
    fn ids_names_and_bytes() {
        assert!(valid_id("neon-a1b2c3") && !valid_id("../x") && !valid_id("A") && !valid_id("") && !valid_id("-x"));
        assert!(valid_asset_name("bg.png") && !valid_asset_name("../bg.png") && !valid_asset_name(".hidden"));
        assert_eq!(asset_type(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(asset_type(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(asset_type(b"wOF2...."), Some("font/woff2"));
        assert_eq!(asset_type(b"<svg onload=alert(1)>"), None);
        let id = new_id("My Neon Theme!");
        assert!(valid_id(&id) && id.starts_with("my-neon-theme-"), "{id}");
        assert_eq!(clean_settings(&json!({ "theme": "../../x" }))["theme"], "builtin:dark");
        assert_eq!(clean_settings(&json!({ "theme": "builtin:amoled", "sidePinned": true }))["sidePinned"], true);
        let t = clean_settings(&json!({ "translate": { "channels": { "12": { "incoming": true, "incomingLang": "fi" }, "x": {}, "13": { "outgoingLang": "en;bad" } } } }))["translate"].clone();
        assert_eq!(t["channels"]["12"]["incomingLang"], "fi");
        assert!(t["channels"].get("x").is_none());
        assert_eq!(t["channels"]["13"]["outgoingLang"], "en");
    }

    #[test]
    fn the_store_round_trips_on_disk() {
        let dir = std::env::temp_dir().join(format!("lockcord-settings-test-{}", std::process::id()));
        let store = Store::new(&dir, "test");
        let saved = store.save_theme(&json!({ "name": "Glass", "tokens": { "--lc-brand": "#00ffaa" } })).unwrap();
        let id = saved["id"].as_str().unwrap().to_owned();
        assert_eq!(store.list().len(), 1);
        let png = b"\x89PNG\r\n\x1a\n0000";
        let name = store.add_asset(&id, "C:\\Users\\x\\Back Ground.png", png).unwrap();
        assert_eq!(name, "background.png");
        assert_eq!(store.add_asset(&id, "background.png", png).unwrap(), "background-2.png");
        assert!(store.add_asset(&id, "x.svg", b"<svg/>").is_err());
        assert_eq!(store.read_asset(&format!("{id}/background.png")).unwrap().0, "image/png");
        assert!(store.read_asset(&format!("{id}/../theme.json")).is_none());
        assert_eq!(store.theme(&id).unwrap()["assets"].as_array().unwrap().len(), 2);
        store.save_settings(&json!({ "theme": id, "sidePinned": true })).unwrap();
        assert_eq!(store.settings()["theme"], id.as_str());
        // Packed and unpacked: a new theme with the same look and files.
        let (_, file) = store.pack(&id).unwrap();
        let copy = store.unpack(&file).unwrap();
        assert_ne!(copy["id"], json!(id));
        assert_eq!(copy["tokens"]["--lc-brand"], "#00ffaa");
        assert_eq!(copy["assets"].as_array().unwrap().len(), 2);
        assert!(store.unpack(br#"{"name":"X","assets":{"evil.svg":"PHN2Zz4="}}"#).is_err());
        assert!(store.unpack(b"not json").is_err());
        assert_eq!(file_name("My: Theme/1"), "My Theme1.lctheme");
        store.delete_theme(copy["id"].as_str().unwrap()).unwrap();
        store.delete_theme(&id).unwrap();
        assert!(store.list().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
