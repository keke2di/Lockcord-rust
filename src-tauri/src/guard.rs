//! Guard, from Lazer's monitor by way of lockcord-public (`main/features/purge.js`): per channel,
//!
//!   a trigger word   your message containing it, sent after Guard was armed there, wipes all of
//!                    your messages in that channel (the trigger too): a panic button;
//!   a time limit     your messages there are deleted once they're that old, but only those sent
//!                    after arming, so turning Guard on never touches the past.
//!
//! The rules live in the profile folder (guard.json). main.rs feeds Guard your own messages from
//! the gateway, and sweeps every minute for what was sent from elsewhere while LockCord was closed.

use crate::deletion::{snowflake_at, time_of};
use serde_json::{Value, json};

pub const MAX_TTL_S: u64 = 365 * 86_400;
/// How many of a channel's newest messages a sweep looks at for the trigger.
pub const TRIGGER_LOOKBACK: usize = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct Rule {
    pub channel: String,
    pub name: String,
    pub trigger: String,
    pub ttl: u64,
    pub armed_at: u64,
}

fn is_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 20 && s.bytes().all(|b| b.is_ascii_digit())
}

impl Rule {
    /// From the UI. A new or changed rule is armed now: it can never be armed in the past.
    pub fn from(raw: &Value, now: u64) -> Option<Rule> {
        let channel = raw["channel"].as_str().filter(|c| is_id(c))?.to_owned();
        let trigger: String = raw["trigger"].as_str().unwrap_or("").trim().chars().take(100).collect();
        let ttl = raw["ttl"].as_u64().unwrap_or(0).min(MAX_TTL_S);
        if trigger.is_empty() && ttl == 0 {
            return None;
        }
        let name: String = raw["name"].as_str().unwrap_or("").chars().filter(|c| !c.is_control()).take(100).collect();
        Some(Rule { channel, name, trigger, ttl, armed_at: now })
    }

    fn stored(v: &Value) -> Option<Rule> {
        let mut rule = Rule::from(v, 0)?;
        rule.armed_at = v["armedAt"].as_u64()?;
        Some(rule)
    }

    pub fn to_json(&self) -> Value {
        json!({ "channel": self.channel, "name": self.name, "trigger": self.trigger, "ttl": self.ttl, "armedAt": self.armed_at })
    }

    /// Whether a message counts for this rule at all: yours, and sent after arming.
    fn counts(&self, m: &Value, me: &str) -> bool {
        m["author"]["id"] == me && m["id"].as_str().is_some_and(|id| time_of(id) >= self.armed_at)
    }

    /// Your message, sent after arming, with the trigger word in it (any case).
    pub fn is_trigger(&self, m: &Value, me: &str) -> bool {
        !self.trigger.is_empty() && self.counts(m, me) && m["content"].as_str().is_some_and(|c| c.to_lowercase().contains(&self.trigger.to_lowercase()))
    }

    /// When your message is due to go, if the time limit applies to it.
    pub fn expires_at(&self, m: &Value, me: &str) -> Option<u64> {
        (self.ttl > 0 && self.counts(m, me)).then(|| time_of(m["id"].as_str().unwrap_or("")) + self.ttl * 1000)
    }

    /// The newest id a sweep for expired messages starts below, and the oldest it may reach.
    pub fn expiry_window(&self, now: u64) -> Option<(u64, u64)> {
        (self.ttl > 0 && now > self.armed_at + self.ttl * 1000).then(|| (snowflake_at(now - self.ttl * 1000), snowflake_at(self.armed_at)))
    }
}

pub fn load(raw: &Value) -> Vec<Rule> {
    raw["rules"].as_array().into_iter().flatten().filter_map(Rule::stored).take(200).collect()
}

pub fn save(rules: &[Rule]) -> Value {
    json!({ "rules": rules.iter().map(Rule::to_json).collect::<Vec<_>>() })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(id_ms: u64, author: &str, content: &str) -> Value {
        json!({ "id": snowflake_at(id_ms).to_string(), "author": { "id": author }, "content": content })
    }

    #[test]
    fn rules_are_checked_and_armed_now() {
        let now = 1_700_000_000_000;
        assert!(Rule::from(&json!({ "channel": "1" }), now).is_none(), "a rule needs a trigger or a time");
        assert!(Rule::from(&json!({ "channel": "x", "ttl": 60 }), now).is_none());
        let r = Rule::from(&json!({ "channel": "1", "trigger": "  NUKE ", "ttl": 9_999_999_999u64, "armedAt": 5 }), now).unwrap();
        assert_eq!((r.trigger.as_str(), r.ttl, r.armed_at), ("NUKE", MAX_TTL_S, now));
        assert_eq!(load(&save(&[r.clone()])), vec![r]);
    }

    #[test]
    fn only_your_messages_after_arming_count() {
        let armed = 1_700_000_000_000;
        let rule = Rule { channel: "1".into(), name: String::new(), trigger: "nuke".into(), ttl: 60, armed_at: armed };
        assert!(rule.is_trigger(&message(armed + 1000, "5", "ok NUKE it"), "5"));
        assert!(!rule.is_trigger(&message(armed - 1000, "5", "nuke"), "5"), "sent before arming");
        assert!(!rule.is_trigger(&message(armed + 1000, "6", "nuke"), "5"), "someone else's");
        assert_eq!(rule.expires_at(&message(armed + 1000, "5", "hi"), "5"), Some(armed + 1000 + 60_000));
        assert_eq!(rule.expires_at(&message(armed - 1000, "5", "hi"), "5"), None);
        assert_eq!(rule.expiry_window(armed + 30_000), None, "nothing can be due yet");
        let (newest, oldest) = rule.expiry_window(armed + 120_000).unwrap();
        assert_eq!((newest, oldest), (snowflake_at(armed + 60_000), snowflake_at(armed)));
    }
}
