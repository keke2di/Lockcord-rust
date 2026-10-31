//! Discord's gateway, the way the web client connects to it: JSON over a zlib-stream WebSocket,
//! API v9. Hello, heartbeat, identify (with fingerprint.rs's properties), and on a dropped
//! connection a resume where Discord allows one, a fresh identify where it doesn't. Every
//! dispatch goes to the app as (event name, data); the app's own frames (guild subscriptions)
//! go out once the session is ready.
//!
//! Bounds follow Serein's findings: a big account's READY can be tens of MB once inflated, so
//! the wire and the inflated message are capped at 64 MiB, and anything over that ends the
//! connection instead of growing without limit.

use crate::fingerprint;
use flate2::{Decompress, FlushDecompress};
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio::time::{Instant, sleep, sleep_until};
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::frame::coding::CloseCode;
use tokio_tungstenite::tungstenite::{Message, http::HeaderValue};

const GATEWAY: &str = "wss://gateway.discord.gg";

/// A secure WebSocket address on Discord's own gateway hosts.
fn discord_gateway(address: &str) -> bool {
    let Ok(url) = url::Url::parse(address) else { return false };
    let host = url.host_str().unwrap_or("");
    url.scheme() == "wss" && url.username().is_empty() && url.password().is_none() && (host == "discord.gg" || host.ends_with(".discord.gg") || host.ends_with(".discord.com"))
}
const MAX_WIRE: usize = 64 * 1024 * 1024;
const ZLIB_SUFFIX: [u8; 4] = [0x00, 0x00, 0xff, 0xff];

#[derive(Debug)]
pub enum Event {
    /// The gateway is connecting, connected, resuming… (for the status line).
    Status(&'static str),
    Dispatch(String, Value),
    /// The token was refused: the session is over and needs a new login.
    Unauthorized,
}

// One connection's zlib stream: every message continues the same deflate context, and a
// message is complete when the bytes so far end with the sync-flush suffix.
pub struct Inflater {
    z: Decompress,
    buf: Vec<u8>,
}

impl Inflater {
    pub fn new() -> Self {
        Inflater { z: Decompress::new(true), buf: Vec::new() }
    }

    pub fn push(&mut self, data: &[u8]) -> Result<Option<Vec<u8>>, String> {
        self.buf.extend_from_slice(data);
        if self.buf.len() > MAX_WIRE {
            return Err("gateway message over 64 MiB".into());
        }
        if !self.buf.ends_with(&ZLIB_SUFFIX) {
            return Ok(None);
        }
        let input = std::mem::take(&mut self.buf);
        let mut out = Vec::with_capacity((input.len() * 6).min(MAX_WIRE));
        let mut offset = 0;
        loop {
            if out.capacity() - out.len() < 64 * 1024 {
                out.reserve(512 * 1024);
            }
            let before = self.z.total_in();
            self.z.decompress_vec(&input[offset..], &mut out, FlushDecompress::Sync).map_err(|e| e.to_string())?;
            offset += (self.z.total_in() - before) as usize;
            if out.len() > MAX_WIRE {
                return Err("inflated gateway message over 64 MiB".into());
            }
            if offset >= input.len() && out.len() < out.capacity() {
                return Ok(Some(out));
            }
        }
    }
}

struct Session {
    id: String,
    seq: Option<u64>,
    resume_url: String,
}

enum Ended {
    /// Reconnect, resuming if there's a session.
    Retry,
    /// Discord said the session can't be resumed: identify afresh.
    Fresh,
    Unauthorized,
    Stopped,
}

/// Runs until `stop` turns true or the token is refused. `outbound` carries frames the app sends
/// (op 37 subscriptions); they wait while the gateway is down.
pub async fn run(token: String, events: mpsc::Sender<Event>, mut outbound: mpsc::Receiver<Value>, mut stop: watch::Receiver<bool>) {
    let mut session: Option<Session> = None;
    let mut failures: u32 = 0;
    loop {
        if *stop.borrow() {
            return;
        }
        let _ = events.send(Event::Status(if session.is_some() { "resuming" } else { "connecting" })).await;
        let started = Instant::now();
        match connection(&token, &mut session, &events, &mut outbound, &mut stop).await {
            Ended::Stopped => return,
            Ended::Unauthorized => {
                let _ = events.send(Event::Unauthorized).await;
                return;
            }
            Ended::Fresh => session = None,
            Ended::Retry => {}
        }
        // A connection that lasted a while resets the backoff.
        failures = if started.elapsed() > Duration::from_secs(60) { 0 } else { failures + 1 };
        let _ = events.send(Event::Status("reconnecting")).await;
        let backoff = Duration::from_millis((1000u64 << failures.min(5)).min(30_000) + jitter(1000));
        tokio::select! {
            _ = sleep(backoff) => {}
            _ = stop.changed() => return,
        }
    }
}

async fn connection(token: &str, session: &mut Option<Session>, events: &mpsc::Sender<Event>, outbound: &mut mpsc::Receiver<Value>, stop: &mut watch::Receiver<bool>) -> Ended {
    let base = session.as_ref().map(|s| s.resume_url.clone()).unwrap_or_else(|| GATEWAY.to_owned());
    let url = format!("{}/?encoding=json&v=9&compress=zlib-stream", base.trim_end_matches('/'));
    let Ok(mut request) = url.as_str().into_client_request() else { return Ended::Fresh };
    let headers = request.headers_mut();
    if let Ok(ua) = HeaderValue::from_str(&fingerprint::user_agent()) {
        headers.insert("User-Agent", ua);
    }
    headers.insert("Origin", HeaderValue::from_static("https://discord.com"));

    let socket = tokio::select! {
        r = tokio_tungstenite::connect_async(request) => match r { Ok((s, _)) => s, Err(_) => return Ended::Retry },
        _ = stop.changed() => return Ended::Stopped,
    };
    let (mut write, mut read) = socket.split();
    let mut inflater = Inflater::new();
    let mut interval = Duration::from_secs(41);
    let mut next_beat: Option<Instant> = None;
    let mut acked = true;
    // Discord closes a connection that sends anything but identify/resume before it's ready.
    let mut ready = false;

    loop {
        let beat_at = next_beat.unwrap_or_else(|| Instant::now() + Duration::from_secs(3600));
        let frame = tokio::select! {
            f = read.next() => f,
            _ = sleep_until(beat_at), if next_beat.is_some() => {
                // No ack since the last beat: the connection is a zombie.
                if !acked {
                    let _ = write.send(Message::Close(None)).await;
                    return Ended::Retry;
                }
                acked = false;
                let seq = session.as_ref().and_then(|s| s.seq);
                if write.send(Message::Text(json!({"op": 1, "d": seq}).to_string().into())).await.is_err() {
                    return Ended::Retry;
                }
                next_beat = Some(Instant::now() + interval);
                continue;
            }
            Some(frame) = outbound.recv(), if ready => {
                if write.send(Message::Text(frame.to_string().into())).await.is_err() {
                    return Ended::Retry;
                }
                continue;
            }
            _ = stop.changed() => {
                let _ = write.send(Message::Close(None)).await;
                return Ended::Stopped;
            }
        };

        let bytes = match frame {
            Some(Ok(Message::Binary(data))) => match inflater.push(&data) {
                Ok(Some(full)) => full,
                Ok(None) => continue,
                Err(_) => return Ended::Fresh,
            },
            Some(Ok(Message::Text(text))) => text.as_bytes().to_vec(),
            Some(Ok(Message::Close(frame))) => {
                let code = frame.map(|f| u16::from(f.code)).unwrap_or(1000);
                return closed(code);
            }
            Some(Ok(_)) => continue,
            Some(Err(_)) | None => return Ended::Retry,
        };
        let Ok(payload) = serde_json::from_slice::<Value>(&bytes) else { continue };
        if let Some(s) = payload["s"].as_u64() {
            if let Some(session) = session.as_mut() {
                session.seq = Some(s);
            }
        }

        match payload["op"].as_u64() {
            // Hello: start beating (the first after a random part of the interval), then
            // identify or resume.
            Some(10) => {
                interval = Duration::from_millis(payload["d"]["heartbeat_interval"].as_u64().unwrap_or(41_250).clamp(1_000, 120_000));
                next_beat = Some(Instant::now() + Duration::from_millis(jitter(interval.as_millis() as u64)));
                acked = true;
                let hello = match session.as_ref() {
                    Some(s) => json!({"op": 6, "d": {"token": token, "session_id": s.id, "seq": s.seq}}),
                    None => identify(token),
                };
                if write.send(Message::Text(hello.to_string().into())).await.is_err() {
                    return Ended::Retry;
                }
            }
            Some(11) => acked = true,
            // Discord asking for a beat now.
            Some(1) => {
                let seq = session.as_ref().and_then(|s| s.seq);
                let _ = write.send(Message::Text(json!({"op": 1, "d": seq}).to_string().into())).await;
            }
            Some(7) => return Ended::Retry,
            Some(9) => {
                // Invalid session: resumable or not, wait a moment as Discord asks.
                sleep(Duration::from_millis(1000 + jitter(4000))).await;
                return if payload["d"].as_bool() == Some(true) { Ended::Retry } else { Ended::Fresh };
            }
            Some(0) => {
                let name = payload["t"].as_str().unwrap_or("").to_owned();
                let mut data = payload["d"].clone();
                if name == "READY" {
                    // The token goes to the resume address later: only ever one of Discord's.
                    if let (Some(id), Some(url)) = (data["session_id"].as_str(), data["resume_gateway_url"].as_str().filter(|u| discord_gateway(u))) {
                        *session = Some(Session { id: id.to_owned(), seq: payload["s"].as_u64(), resume_url: url.to_owned() });
                    }
                    ready = true;
                    let _ = events.send(Event::Status("connected")).await;
                } else if name == "RESUMED" {
                    ready = true;
                    let _ = events.send(Event::Status("connected")).await;
                }
                if events.send(Event::Dispatch(name, data.take())).await.is_err() {
                    return Ended::Stopped;
                }
            }
            _ => {}
        }
    }
}

fn identify(token: &str) -> Value {
    json!({
        "op": 2,
        "d": {
            "token": token,
            "properties": fingerprint::properties(),
            "presence": {"status": "unknown", "since": 0, "activities": [], "afk": false},
            "compress": false,
            "client_state": {"guild_versions": {}},
        }
    })
}

// Discord's close codes: 4004 is a refused token; 4010-4014 can't be fixed by reconnecting;
// 4007 (bad seq) and 4009 (timed out) need a fresh session; the rest can resume.
fn closed(code: u16) -> Ended {
    match code {
        4004 => Ended::Unauthorized,
        4010..=4014 => Ended::Unauthorized,
        4007 | 4009 => Ended::Fresh,
        c if c == u16::from(CloseCode::Normal) => Ended::Fresh,
        _ => Ended::Retry,
    }
}

fn jitter(max_ms: u64) -> u64 {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    u64::from_le_bytes(b) % max_ms.max(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::{Compress, Compression, FlushCompress};

    // Two messages on one deflate stream, the second split across frames, as Discord sends them.
    #[test]
    fn resume_only_to_discord() {
        assert!(discord_gateway("wss://gateway-us-east1-b.discord.gg"));
        assert!(discord_gateway("wss://gateway.discord.gg/"));
        assert!(!discord_gateway("ws://gateway.discord.gg"), "not encrypted");
        assert!(!discord_gateway("wss://gateway.discord.gg.evil.example"));
        assert!(!discord_gateway("wss://evildiscord.gg"));
        assert!(!discord_gateway("https://gateway.discord.gg"));
    }

    #[test]
    fn inflates_a_zlib_stream_across_frames() {
        let mut z = Compress::new(Compression::default(), true);
        let mut wire = Vec::new();
        for text in [r#"{"op":10}"#, r#"{"op":0,"t":"READY"}"#] {
            let mut out = Vec::with_capacity(1024);
            z.compress_vec(text.as_bytes(), &mut out, FlushCompress::Sync).unwrap();
            wire.push(out);
        }
        let mut inflater = Inflater::new();
        assert_eq!(inflater.push(&wire[0]).unwrap().unwrap(), br#"{"op":10}"#);
        let (a, b) = wire[1].split_at(3);
        assert!(inflater.push(a).unwrap().is_none());
        assert_eq!(inflater.push(b).unwrap().unwrap(), br#"{"op":0,"t":"READY"}"#);
    }

    #[test]
    fn close_codes() {
        assert!(matches!(closed(4004), Ended::Unauthorized));
        assert!(matches!(closed(4009), Ended::Fresh));
        assert!(matches!(closed(1006), Ended::Retry));
    }
}
