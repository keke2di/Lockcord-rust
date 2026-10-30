//! Discord's captcha. When Discord answers a request with a captcha (the first message to someone
//! new, a friend request…), the request waits while a small window shows the hCaptcha, then goes
//! again with the solution (rest.rs). Nothing of it is in the main window, which still loads
//! nothing from outside.
//!
//! The window shows LockCord's own page, served by the `lccaptcha:` scheme with a CSP that allows
//! hCaptcha's hosts and nothing else; hCaptcha runs in its native-app host mode, as Serein does
//! (MIT, see THIRD_PARTY_NOTICES.md), since Discord's site key is otherwise tied to discord.com.
//! The solution comes back as the login's token does: the page starts a navigation to an address
//! that doesn't exist, which the window refuses and LockCord reads. Tauri still injects its IPC
//! into the page, and the scheme counts as a local origin, so LockCord's commands are refused to
//! every window but the main one: by the capability (build.rs) and by main.rs's `only_main`.

use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::oneshot;

pub const SCHEME: &str = "lccaptcha";
pub const LABEL: &str = "captcha";
const DONE_HOST: &str = "lockcord-captcha.invalid";
const WAIT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, PartialEq)]
pub struct Challenge {
    pub sitekey: String,
    pub rqdata: Option<String>,
    pub rqtoken: Option<String>,
    pub session_id: Option<String>,
}

fn printable(s: &str, max: usize) -> bool {
    !s.is_empty() && s.len() <= max && s.bytes().all(|b| (0x21..=0x7e).contains(&b))
}

/// Discord's 400 that asks for an hCaptcha, read: { captcha_key, captcha_sitekey,
/// captcha_service, captcha_rqdata?, captcha_rqtoken?, captcha_session_id? }.
pub fn challenge_of(body: &Value) -> Option<Challenge> {
    if body.get("captcha_key").is_none() || body["captcha_service"].as_str().is_some_and(|s| s != "hcaptcha") {
        return None;
    }
    let sitekey = body["captcha_sitekey"].as_str().filter(|k| k.len() == 36 && k.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-'))?.to_owned();
    let field = |name: &str, max: usize| body[name].as_str().filter(|v| printable(v, max)).map(str::to_owned);
    Some(Challenge { sitekey, rqdata: field("captcha_rqdata", 8192), rqtoken: field("captcha_rqtoken", 8192), session_id: field("captcha_session_id", 128) })
}

/// The headers the retried request carries.
pub fn retry_headers(challenge: &Challenge, solution: &str) -> Vec<(String, String)> {
    let mut out = vec![("X-Captcha-Key".to_owned(), solution.to_owned())];
    if let Some(t) = &challenge.rqtoken {
        out.push(("X-Captcha-Rqtoken".to_owned(), t.clone()));
    }
    if let Some(s) = &challenge.session_id {
        out.push(("X-Captcha-Session-Id".to_owned(), s.clone()));
    }
    out
}

pub fn valid_solution(s: &str) -> bool {
    printable(s, 8192)
}

/// The window's CSP: hCaptcha's hosts and the page's own script, nothing else.
pub fn csp(nonce: &str) -> String {
    format!(
        "default-src 'none'; script-src 'nonce-{nonce}' https://js.hcaptcha.com https://*.hcaptcha.com; frame-src https://*.hcaptcha.com; \
         style-src 'unsafe-inline' https://*.hcaptcha.com; img-src https://*.hcaptcha.com data:; connect-src https://*.hcaptcha.com; \
         base-uri 'none'; form-action 'none'"
    )
}

/// The page. `key` ties the solution to this window; the config is JSON with `<` escaped, so
/// nothing in it can end the script.
pub fn page(challenge: &Challenge, key: &str, nonce: &str) -> String {
    let config = json!({ "sitekey": challenge.sitekey, "rqdata": challenge.rqdata, "key": key, "done": format!("https://{DONE_HOST}/") }).to_string().replace('<', "\\u003c");
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><title>Verify — LockCord</title>
<style>
  html, body {{ margin: 0; height: 100%; background: #313338; color: #dbdee1; font: 15px/1.4 'Segoe UI', system-ui, sans-serif; }}
  main {{ display: flex; flex-direction: column; align-items: center; justify-content: center; gap: 16px; min-height: 100%; padding: 16px; box-sizing: border-box; text-align: center; }}
  h1 {{ margin: 0; font-size: 20px; color: #f2f3f5; }}
  p {{ margin: 0; max-width: 340px; color: #b5bac1; }}
  button {{ border: 0; border-radius: 8px; padding: 8px 16px; background: #4e5058; color: #fff; font: inherit; cursor: pointer; }}
</style></head>
<body><main>
  <h1>Discord wants a quick check</h1>
  <p id="status">Loading the check…</p>
  <div id="captcha"></div>
  <button id="cancel" type="button">Cancel</button>
</main>
<script nonce="{nonce}">
(() => {{
  "use strict";
  const config = {config};
  let finished = false;
  const send = (kind, value) => {{
    if (finished) return;
    finished = true;
    location.href = config.done + "?k=" + encodeURIComponent(config.key) + "&" + kind + "=" + encodeURIComponent(value || "1");
  }};
  const status = document.getElementById("status");
  const fail = () => {{ status.textContent = "The check couldn't load. Close this window and try again."; }};
  document.getElementById("cancel").addEventListener("click", () => send("cancel"));
  document.addEventListener("keydown", (e) => {{ if (e.key === "Escape") send("cancel"); }});
  window.lcCaptchaLoaded = () => {{
    try {{
      const widget = window.hcaptcha.render("captcha", {{
        sitekey: config.sitekey, theme: "dark",
        callback: (value) => {{
          if (typeof value !== "string" || !/^[\x21-\x7e]{{1,8192}}$/.test(value)) return fail();
          status.textContent = "Done. Sending your message…";
          send("t", value);
        }},
        "error-callback": fail,
      }});
      if (config.rqdata) window.hcaptcha.setData(widget, {{ rqdata: config.rqdata }});
      status.textContent = "Complete the check below and LockCord carries on.";
    }} catch (e) {{ fail(); }}
  }};
  const script = document.createElement("script");
  script.nonce = "{nonce}";
  script.src = "https://js.hcaptcha.com/1/api.js?onload=lcCaptchaLoaded&render=explicit&recaptchacompat=off&host=" + encodeURIComponent(config.sitekey) + ".react-native.hcaptcha.com";
  script.async = true;
  script.onerror = fail;
  document.head.appendChild(script);
}})();
</script>
</body></html>"#
    )
}

/// What the scheme serves: the challenge being solved, its key and the script nonce.
#[derive(Default)]
pub struct Showing {
    pub current: Mutex<Option<(Challenge, String, String)>>,
    // One captcha window at a time.
    pub one: tokio::sync::Mutex<()>,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn page_url() -> url::Url {
    if cfg!(windows) { "http://lccaptcha.localhost/".parse().expect("url") } else { "lccaptcha://localhost/".parse().expect("url") }
}

/// Shows the captcha and waits for it: the solution, or None if it was cancelled, closed or not
/// done within ten minutes. `data_dir` is the profile's own WebView2 folder.
pub async fn solve(app: AppHandle, showing: Arc<Showing>, data_dir: std::path::PathBuf, user_agent: String, challenge: Challenge) -> Option<String> {
    let _one = showing.one.lock().await;
    let mut raw = [0u8; 32];
    getrandom::fill(&mut raw).ok()?;
    let (key, nonce) = (hex(&raw[..16]), hex(&raw[16..]));
    *showing.current.lock().unwrap() = Some((challenge, key.clone(), nonce));

    let (tx, rx) = oneshot::channel::<Option<String>>();
    let answer = Arc::new(Mutex::new(Some(tx)));
    let on_nav = answer.clone();
    let on_close = answer.clone();
    let handle = app.clone();
    let built = WebviewWindowBuilder::new(&app, LABEL, WebviewUrl::CustomProtocol(page_url()))
        .title("Verify — LockCord")
        .inner_size(420.0, 640.0)
        .incognito(true)
        .data_directory(data_dir)
        .user_agent(&user_agent)
        .on_navigation(move |url| {
            if url.host_str() == Some(DONE_HOST) {
                let pairs: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
                if pairs.get("k") == Some(&key) {
                    let solution = pairs.get("t").filter(|t| valid_solution(t)).cloned();
                    if let Some(tx) = on_nav.lock().unwrap().take() {
                        let _ = tx.send(solution);
                    }
                    if let Some(w) = handle.get_webview_window(LABEL) {
                        let _ = w.destroy();
                    }
                }
                return false;
            }
            // Only its own page; hCaptcha's frames aren't top-level navigations.
            url.host_str() == page_url().host_str() && url.scheme() == page_url().scheme()
        })
        .build();
    let result = match built {
        Ok(window) => {
            window.on_window_event(move |event| {
                if matches!(event, tauri::WindowEvent::Destroyed) {
                    if let Some(tx) = on_close.lock().unwrap().take() {
                        let _ = tx.send(None);
                    }
                }
            });
            let _ = window.set_focus();
            tokio::time::timeout(WAIT, rx).await.ok().and_then(|r| r.ok()).flatten()
        }
        Err(_) => None,
    };
    if let Some(w) = app.get_webview_window(LABEL) {
        let _ = w.destroy();
    }
    *showing.current.lock().unwrap() = None;
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discords_captcha_answer_is_read_and_checked() {
        let body = json!({
            "captcha_key": ["captcha-required"], "captcha_sitekey": "a9b5fb07-92ff-493f-86fe-352a2803b3df", "captcha_service": "hcaptcha",
            "captcha_rqdata": "abc", "captcha_rqtoken": "tok", "captcha_session_id": "s1",
        });
        let c = challenge_of(&body).unwrap();
        assert_eq!(c.rqdata.as_deref(), Some("abc"));
        let headers = retry_headers(&c, "P1_solution");
        assert_eq!(headers[0], ("X-Captcha-Key".to_owned(), "P1_solution".to_owned()));
        assert_eq!(headers.len(), 3);
        assert!(challenge_of(&json!({ "message": "Missing Access" })).is_none());
        assert!(challenge_of(&json!({ "captcha_key": [], "captcha_sitekey": "x" })).is_none(), "a bad site key");
        assert!(challenge_of(&json!({ "captcha_key": [], "captcha_sitekey": "a9b5fb07-92ff-493f-86fe-352a2803b3df", "captcha_service": "recaptcha" })).is_none());
        assert!(!valid_solution("has space") && !valid_solution("") && valid_solution("P1_abc.def"));
    }

    #[test]
    fn the_page_cant_be_broken_out_of() {
        let c = Challenge { sitekey: "a9b5fb07-92ff-493f-86fe-352a2803b3df".into(), rqdata: Some("</script><script>alert(1)</script>".into()), rqtoken: None, session_id: None };
        let html = page(&c, "k", "n");
        assert!(!html.contains("</script><script>alert(1)"));
        assert!(html.contains("\\u003c/script>"));
        assert!(csp("n").contains("script-src 'nonce-n' https://js.hcaptcha.com"));
    }
}
