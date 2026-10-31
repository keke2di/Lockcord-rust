//! Settings, kept on disk per profile (the window is incognito and keeps nothing):
//!
//!   <app data>/profiles/<profile>/settings.json   settings, cleaned here from fixed lists
//!   <app data>/profiles/<profile>/<name>.json     the profile's own files (guard, contacts, ...)
//!
//! Themes are built in (ui/js/theme.js and ui/css/skins/): a setting only names one, so nothing
//! a theme could carry is ever read from disk.

use serde_json::{Map, Value, json};
use std::fs;
use std::path::{Path, PathBuf};

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 40 && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

fn clean_translation(raw: &Value) -> Value {
    let lang = |v: &Value| v.as_str().filter(|l| crate::translate::valid_lang(l) && *l != "auto").unwrap_or("en").to_owned();
    json!({
        "incoming": raw["incoming"].as_bool().unwrap_or(false), "incomingLang": lang(&raw["incomingLang"]),
        "outgoing": raw["outgoing"].as_bool().unwrap_or(false), "outgoingLang": lang(&raw["outgoingLang"]),
    })
}

/// Settings outside themes, from fixed lists.
pub fn clean_settings(raw: &Value) -> Value {
    // A built-in theme's colour scheme (theme.js PRESETS), by id.
    let theme = raw["theme"].as_str().filter(|t| t.strip_prefix("builtin:").is_some_and(valid_id)).unwrap_or("builtin:dark");
    json!({
        "theme": theme,
        // Display: message density, text size (px) and motion, outside themes.
        "display": {
            "density": if raw["display"]["density"] == "compact" { "compact" } else { "cozy" },
            "textSize": raw["display"]["textSize"].as_u64().map(|n| n.clamp(12, 20)).unwrap_or(16),
            "motion": raw["display"]["motion"].as_bool().unwrap_or(true),
        },
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
    let device = |k: &str| raw[k].as_str().filter(|d| !d.is_empty() && d.len() <= 512 && !d.contains('\0')).map(|d| json!(d)).unwrap_or(Value::Null);
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
}

// Written whole to a temporary file first, so a crash never leaves half a file.
fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path.parent().ok_or("bad path")?;
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes).map_err(|e| e.to_string())?;
    fs::rename(&tmp, path).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_keep_only_what_they_may() {
        let s = clean_settings(&json!({ "theme": "builtin:oled-ember", "display": { "density": "compact", "textSize": 40, "motion": false } }));
        assert_eq!(s["theme"], "builtin:oled-ember");
        assert_eq!(s["display"]["density"], "compact");
        assert_eq!(s["display"]["textSize"], 20);
        assert_eq!(s["display"]["motion"], false);
        assert_eq!(clean_settings(&json!({ "theme": "my-theme-abc123" }))["theme"], "builtin:dark", "user themes are gone");
        assert_eq!(clean_settings(&json!({ "theme": "builtin:../x" }))["theme"], "builtin:dark");
        assert_eq!(clean_settings(&Value::Null)["display"]["textSize"], 16);
        assert_eq!(clean_settings(&Value::Null)["checkUpdates"], true);
    }

    #[test]
    fn the_store_round_trips_on_disk() {
        let dir = std::env::temp_dir().join(format!("lockcord-settings-test-{}", std::process::id()));
        let store = Store::new(&dir, "p");
        let saved = store.save_settings(&json!({ "notify": "text" })).unwrap();
        assert_eq!(store.settings(), saved);
        store.write_json("x.json", &json!({ "a": 1 })).unwrap();
        assert_eq!(store.read_json("x.json")["a"], 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
