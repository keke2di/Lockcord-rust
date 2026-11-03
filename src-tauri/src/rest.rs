//! Discord's REST API, as the web client calls it: the same headers on every request (see
//! fingerprint.rs), and Discord's rate limits honoured. A 429 waits for as long as Discord says
//! and tries again, at most a few times; a bucket that says it's empty waits for its reset first.

use crate::captcha;
use crate::fingerprint;
use reqwest::{Method, StatusCode};
use serde_json::Value;
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const API: &str = "https://discord.com/api/v9";
const MAX_RETRIES: u32 = 3;
const MAX_BODY: usize = 8 * 1024 * 1024;

#[derive(Debug)]
pub enum RestError {
    /// The token was refused (401): the session is over.
    Unauthorized,
    Status(u16, String),
    Network(String),
    TooLarge,
    /// Discord asked for a captcha and it wasn't done.
    Captcha,
}

/// Shows Discord's captcha and waits: the solution, or None (captcha.rs, set by main.rs).
pub type Solver = Arc<dyn Fn(captcha::Challenge) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

impl std::fmt::Display for RestError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestError::Unauthorized => write!(f, "Discord refused the session (401)"),
            RestError::Status(code, message) => write!(f, "Discord answered {code}: {message}"),
            RestError::Network(e) => write!(f, "network: {e}"),
            RestError::TooLarge => write!(f, "response too large"),
            RestError::Captcha => write!(f, "Discord wanted a captcha for this, and it wasn't done"),
        }
    }
}

pub struct Rest {
    http: reqwest::Client,
    token: String,
    // When a route's bucket is empty, the time it refills.
    blocked: Mutex<HashMap<String, Instant>>,
    solver: Option<Solver>,
}

impl Rest {
    pub fn new(http: reqwest::Client, token: String) -> Self {
        Rest { http, token, blocked: Mutex::new(HashMap::new()), solver: None }
    }

    /// A captcha Discord asks for is shown with `solver`, and the request goes again with it.
    pub fn with_solver(mut self, solver: Solver) -> Self {
        self.solver = Some(solver);
        self
    }


    pub async fn get(&self, path: &str) -> Result<Value, RestError> {
        self.request(Method::GET, path, None).await
    }

    pub async fn post(&self, path: &str, body: Value) -> Result<Value, RestError> {
        self.request(Method::POST, path, Some(body)).await
    }

    pub async fn delete(&self, path: &str) -> Result<Value, RestError> {
        self.request(Method::DELETE, path, None).await
    }

    pub async fn put(&self, path: &str) -> Result<Value, RestError> {
        self.request(Method::PUT, path, None).await
    }

    pub async fn patch(&self, path: &str, body: Value) -> Result<Value, RestError> {
        self.request(Method::PATCH, path, Some(body)).await
    }

    pub async fn request(&self, method: Method, path: &str, body: Option<Value>) -> Result<Value, RestError> {
        self.request_with(method, path, body, &[]).await
    }

    /// With extra headers, such as the X-Context-Properties the web client adds to some calls.
    pub async fn request_with(&self, method: Method, path: &str, body: Option<Value>, extra: &[(&str, &str)]) -> Result<Value, RestError> {
        let route = route_key(&method, path);
        // Set once a captcha was solved: the retry carries its headers.
        let mut captcha_headers: Vec<(String, String)> = Vec::new();
        for attempt in 0..=MAX_RETRIES {
            let wait = self.blocked.lock().unwrap().get(&route).and_then(|until| until.checked_duration_since(Instant::now()));
            if let Some(wait) = wait {
                tokio::time::sleep(wait.min(Duration::from_secs(30))).await;
            }

            let mut request = self
                .http
                .request(method.clone(), format!("{API}{path}"))
                .header("Authorization", &self.token)
                .header("User-Agent", fingerprint::user_agent())
                .header("X-Super-Properties", fingerprint::super_properties())
                .header("X-Discord-Locale", fingerprint::discord_locale())
                .header("Origin", "https://discord.com")
                .header("Referer", "https://discord.com/channels/@me");
            for (name, value) in extra {
                request = request.header(*name, *value);
            }
            for (name, value) in &captcha_headers {
                request = request.header(name.as_str(), value.as_str());
            }
            if let Some(body) = &body {
                request = request.json(body);
            }
            crate::netlog::note(API);
            let response = request.send().await.map_err(|e| RestError::Network(e.without_url().to_string()))?;
            let status = response.status();
            let headers = response.headers().clone();

            if headers.get("x-ratelimit-remaining").and_then(|v| v.to_str().ok()) == Some("0") {
                if let Some(secs) = header_secs(&headers, "x-ratelimit-reset-after") {
                    self.blocked.lock().unwrap().insert(route.clone(), Instant::now() + secs);
                }
            }

            if status == StatusCode::TOO_MANY_REQUESTS && attempt < MAX_RETRIES {
                let text = response.text().await.unwrap_or_default();
                let retry = serde_json::from_str::<Value>(&text).ok().and_then(|v| v["retry_after"].as_f64()).unwrap_or(1.0);
                tokio::time::sleep(Duration::from_secs_f64(retry.clamp(0.1, 30.0))).await;
                continue;
            }
            if status == StatusCode::UNAUTHORIZED {
                return Err(RestError::Unauthorized);
            }

            let bytes = response.bytes().await.map_err(|e| RestError::Network(e.without_url().to_string()))?;
            if bytes.len() > MAX_BODY {
                return Err(RestError::TooLarge);
            }
            if !status.is_success() {
                let answer = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
                // A captcha: shown once, then the same request again with its solution.
                if status == StatusCode::BAD_REQUEST {
                    if let Some(challenge) = captcha::challenge_of(&answer) {
                        let solver = self.solver.clone().filter(|_| captcha_headers.is_empty());
                        let Some(solver) = solver else { return Err(RestError::Captcha) };
                        match solver(challenge.clone()).await {
                            Some(solution) => {
                                captcha_headers = captcha::retry_headers(&challenge, &solution);
                                continue;
                            }
                            None => return Err(RestError::Captcha),
                        }
                    }
                }
                let message = answer["message"].as_str().map(str::to_owned).unwrap_or_default();
                return Err(RestError::Status(status.as_u16(), message));
            }
            if bytes.is_empty() {
                return Ok(Value::Null);
            }
            return serde_json::from_slice(&bytes).map_err(|e| RestError::Network(e.to_string()));
        }
        Err(RestError::Status(429, "rate limited".into()))
    }
}

fn header_secs(headers: &reqwest::header::HeaderMap, name: &str) -> Option<Duration> {
    let secs: f64 = headers.get(name)?.to_str().ok()?.parse().ok()?;
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs.min(60.0)))
}

// Discord's buckets are per route with the channel or guild id kept and other ids replaced. All
// of a channel's reactions share one bucket, whatever the emoji.
fn route_key(method: &Method, path: &str) -> String {
    let path = path.split('?').next().unwrap_or(path);
    let mut out = String::from(method.as_str());
    let mut parent = "";
    for part in path.split('/').filter(|p| !p.is_empty()) {
        out.push('/');
        if parent == "reactions" {
            out.push_str(":emoji");
        } else if part.chars().all(|c| c.is_ascii_digit()) && !(parent == "channels" || parent == "guilds") {
            out.push_str(":id");
        } else {
            out.push_str(part);
        }
        parent = part;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets_keep_the_channel_and_drop_message_ids() {
        assert_eq!(route_key(&Method::GET, "/channels/123/messages?limit=50"), "GET/channels/123/messages");
        assert_eq!(route_key(&Method::DELETE, "/channels/123/messages/456"), "DELETE/channels/123/messages/:id");
        assert_eq!(route_key(&Method::GET, "/users/@me"), "GET/users/@me");
    }

    #[test]
    fn reactions_share_a_bucket_whatever_the_emoji() {
        let thumbs = route_key(&Method::PUT, "/channels/123/messages/456/reactions/%F0%9F%91%8D/@me?location=Message&type=0");
        let custom = route_key(&Method::PUT, "/channels/123/messages/789/reactions/blob%3A1234/@me");
        assert_eq!(thumbs, "PUT/channels/123/messages/:id/reactions/:emoji/@me");
        assert_eq!(thumbs, custom);
    }
}
