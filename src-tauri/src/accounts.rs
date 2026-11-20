//! Account discovery contains display metadata only. Sessions stay in the OS vault.
use serde_json::{Value, json};
use std::path::Path;

pub fn valid_profile(profile: &str) -> bool {
    !profile.is_empty() && profile.len() <= 32 && profile.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
}

pub fn list(data: &Path, current: &str) -> Vec<Value> {
    let mut names = vec![current.to_owned(), "default".to_owned()];
    if let Ok(entries) = std::fs::read_dir(data.join("profiles")) {
        names.extend(entries.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|e| e.file_name().into_string().ok()).filter(|p| valid_profile(p)));
    }
    names.sort();
    names.dedup();
    names.into_iter().map(|profile| {
        let metadata = crate::settings::Store::new(data, &profile).read_json("account.json");
        json!({ "profile": profile, "current": profile == current, "name": metadata["name"], "username": metadata["username"] })
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn profile_names_cannot_escape_their_directory() {
        for name in ["default", "work-2", "Alt_account"] { assert!(valid_profile(name)); }
        for name in ["", "../other", "..", "a/b", "a\\b", "C:\\users", "a b", "--profile=other"] { assert!(!valid_profile(name)); }
        assert!(!valid_profile(&"a".repeat(33)));
    }

    #[test]
    fn account_list_deduplicates_profiles_and_exposes_only_display_metadata() {
        let data = std::env::temp_dir().join(format!("lockcord-account-list-test-{}", std::process::id()));
        let store = crate::settings::Store::new(&data, "work");
        store.write_json("account.json", &json!({ "name": "Work", "username": "worker", "token": "never expose" })).unwrap();
        std::fs::create_dir_all(data.join("profiles").join("default")).unwrap();
        let rows = list(&data, "work");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows.iter().filter(|a| a["current"] == true).count(), 1);
        let work = rows.iter().find(|a| a["profile"] == "work").unwrap();
        assert_eq!(work["name"], "Work");
        assert_eq!(work["username"], "worker");
        assert!(work.get("token").is_none());
        assert!(work.get("id").is_none());
        std::fs::remove_dir_all(&data).unwrap();
    }
}
