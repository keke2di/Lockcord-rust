//! Deleting your own messages, ported from Lazer V2 by way of lockcord-public
//! (`main/features/purge.js`, `main/purge-filters.js`):
//!
//!   the queue   everything that deletes (a single delete, "Delete selected", purge, Guard) goes
//!               through one queue, one DELETE per 350 ms (Lazer's pacing; faster mostly buys
//!               429s), on top of rest.rs's buckets and retry_after. 404 counts as gone.
//!   filters     which of your messages a purge picks up (keywords, a range, attachments only).
//!   collect     walks a channel newest first, 100 a page, paced, down to a snowflake.

use crate::rest::{Rest, RestError};
use serde_json::Value;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::{mpsc, oneshot};
use tokio::time::{Instant, sleep, sleep_until};

pub const DELETE_GAP: Duration = Duration::from_millis(350);
pub const PAGE: usize = 100;
const PAGE_GAP: Duration = Duration::from_millis(500);
const DISCORD_EPOCH_MS: u64 = 1_420_070_400_000;
// Default, reply, slash command, context menu command. System messages (calls, pins, joins)
// can't be deleted by their author, so they're not picked up.
const DELETABLE: [u64; 4] = [0, 19, 20, 23];
const MAX_KEYWORDS: usize = 50;

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// The smallest snowflake made at `ms`.
pub fn snowflake_at(ms: u64) -> u64 {
    ms.saturating_sub(DISCORD_EPOCH_MS) << 22
}

/// When a snowflake was made, in ms.
pub fn time_of(id: &str) -> u64 {
    id.parse::<u64>().map(|n| (n >> 22) + DISCORD_EPOCH_MS).unwrap_or(0)
}

// --- Filters ----------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct Filters {
    pub keywords: Vec<String>,
    /// true: only messages with a keyword; false: all but those.
    pub only: bool,
    /// Days back, or None for all time.
    pub days: Option<u64>,
    pub attachments_only: bool,
}

impl Filters {
    pub fn all() -> Filters {
        Filters { keywords: Vec::new(), only: false, days: None, attachments_only: false }
    }

    /// From the UI: { keywords: [..], keywordMode: "only"|"exclude", range: "all"|"24h"|"7d"|"31d", attachmentsOnly }.
    pub fn from(raw: &Value) -> Filters {
        let keywords = raw["keywords"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|k| k.as_str())
            .map(|k| k.trim().to_lowercase())
            .filter(|k| !k.is_empty() && k.len() <= 100)
            .take(MAX_KEYWORDS)
            .collect();
        let days = match raw["range"].as_str() {
            Some("24h") => Some(1),
            Some("7d") => Some(7),
            Some("31d") => Some(31),
            _ => None,
        };
        Filters { keywords, only: raw["keywordMode"] == "only", days, attachments_only: raw["attachmentsOnly"] == true }
    }

    /// The oldest id a scan has to reach, or None for all time.
    pub fn min_id(&self, now: u64) -> Option<u64> {
        self.days.map(|d| snowflake_at(now.saturating_sub(d * 86_400_000)))
    }

    pub fn matches(&self, m: &Value, me: &str) -> bool {
        if m["author"]["id"] != me || !m["type"].as_u64().is_some_and(|t| DELETABLE.contains(&t)) {
            return false;
        }
        if !self.keywords.is_empty() {
            let content = m["content"].as_str().unwrap_or("").to_lowercase();
            let hit = self.keywords.iter().any(|k| content.contains(k.as_str()));
            if hit != self.only {
                return false;
            }
        }
        !self.attachments_only || m["attachments"].as_array().is_some_and(|a| !a.is_empty())
    }
}

// --- Collecting -------------------------------------------------------------------------------

/// The ids of your messages in a channel that match, newest first, down to `min_id`. Stops when
/// `cancel` turns true; `progress` hears the count after each page.
pub async fn collect(rest: &Rest, channel: &str, me: &str, filters: &Filters, min_id: Option<u64>, cancel: &AtomicBool, mut progress: impl FnMut(usize)) -> Result<Vec<String>, String> {
    let mut ids = Vec::new();
    let mut before: Option<String> = None;
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(ids);
        }
        let mut path = format!("/channels/{channel}/messages?limit={PAGE}");
        if let Some(b) = &before {
            path.push_str(&format!("&before={b}"));
        }
        let page = rest.get(&path).await.map_err(|e| e.to_string())?;
        let list = page.as_array().cloned().unwrap_or_default();
        for m in &list {
            let id = m["id"].as_str().unwrap_or("");
            if min_id.is_some_and(|floor| id.parse::<u64>().unwrap_or(0) < floor) {
                return Ok(ids);
            }
            if filters.matches(m, me) {
                ids.push(id.to_owned());
            }
        }
        progress(ids.len());
        if list.len() < PAGE {
            return Ok(ids);
        }
        before = list.last().and_then(|m| m["id"].as_str()).map(str::to_owned);
        sleep(PAGE_GAP).await;
    }
}

// --- The queue --------------------------------------------------------------------------------

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Deleted,
    Refused(String),
    Stopped,
}

struct Job {
    channel: String,
    id: String,
    cancel: Option<Arc<AtomicBool>>,
    done: oneshot::Sender<Outcome>,
}

#[derive(Clone)]
pub struct Queue {
    jobs: mpsc::UnboundedSender<Job>,
}

impl Queue {
    /// The queue for one session; it ends when the session drops its last handle.
    pub fn start(rest: Arc<Rest>) -> Queue {
        let (jobs, mut rx) = mpsc::unbounded_channel::<Job>();
        tokio::spawn(async move {
            let mut next = Instant::now();
            while let Some(job) = rx.recv().await {
                if job.cancel.as_ref().is_some_and(|c| c.load(Ordering::Relaxed)) {
                    let _ = job.done.send(Outcome::Stopped);
                    continue;
                }
                sleep_until(next).await;
                next = Instant::now() + DELETE_GAP;
                let outcome = match rest.delete(&format!("/channels/{}/messages/{}", job.channel, job.id)).await {
                    Ok(_) | Err(RestError::Status(404, _)) => Outcome::Deleted,
                    Err(e) => Outcome::Refused(e.to_string()),
                };
                let _ = job.done.send(outcome);
            }
        });
        Queue { jobs }
    }

    /// Queues one delete; the answer comes when it's done (or stopped by `cancel`).
    pub fn delete(&self, channel: &str, id: &str, cancel: Option<Arc<AtomicBool>>) -> oneshot::Receiver<Outcome> {
        let (done, answer) = oneshot::channel();
        let job = Job { channel: channel.to_owned(), id: id.to_owned(), cancel, done };
        if let Err(mpsc::error::SendError(job)) = self.jobs.send(job) {
            let _ = job.done.send(Outcome::Stopped);
        }
        answer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn snowflakes_and_times() {
        let t = 1_700_000_000_000;
        assert_eq!(time_of(&snowflake_at(t).to_string()), t);
        assert_eq!(snowflake_at(0), 0);
    }

    #[test]
    fn filters_pick_only_your_deletable_messages() {
        let me = "5";
        let mine = |content: &str, kind: u64, files: usize| json!({ "author": { "id": me }, "type": kind, "content": content, "attachments": vec![json!({}); files] });
        let all = Filters::all();
        assert!(all.matches(&mine("hi", 0, 0), me));
        assert!(all.matches(&mine("re", 19, 0), me));
        assert!(!all.matches(&mine("pinned", 6, 0), me), "system messages aren't yours to delete");
        assert!(!all.matches(&json!({ "author": { "id": "6" }, "type": 0, "content": "x" }), me));

        let only = Filters::from(&json!({ "keywords": ["Secret", " "], "keywordMode": "only" }));
        assert_eq!(only.keywords, ["secret"]);
        assert!(only.matches(&mine("my SECRET plan", 0, 0), me));
        assert!(!only.matches(&mine("nothing here", 0, 0), me));
        let except = Filters::from(&json!({ "keywords": ["keep"], "keywordMode": "exclude" }));
        assert!(!except.matches(&mine("keep this", 0, 0), me));
        assert!(except.matches(&mine("bin this", 0, 0), me));
        let files = Filters::from(&json!({ "attachmentsOnly": true }));
        assert!(files.matches(&mine("", 0, 1), me) && !files.matches(&mine("text", 0, 0), me));
    }

    #[test]
    fn ranges_set_how_far_back_a_scan_goes() {
        let now = 1_700_000_000_000;
        assert_eq!(Filters::from(&json!({ "range": "all" })).min_id(now), None);
        assert_eq!(Filters::from(&json!({ "range": "24h" })).min_id(now), Some(snowflake_at(now - 86_400_000)));
        assert_eq!(Filters::from(&json!({ "range": "weird" })).days, None);
    }

    #[tokio::test]
    async fn a_cancelled_delete_never_reaches_discord() {
        let rest = Arc::new(Rest::new(reqwest::Client::builder().build().unwrap(), String::new()));
        let queue = Queue::start(rest);
        let cancel = Arc::new(AtomicBool::new(true));
        assert_eq!(queue.delete("1", "2", Some(cancel)).await.unwrap(), Outcome::Stopped);
    }
}
