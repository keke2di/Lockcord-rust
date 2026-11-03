//! Translation through Google, as lockcord-public did it (`main/features/translate-core.js`):
//! the translateHtml endpoint Google Translate's own web client uses, line by line (it would
//! fold newlines into spaces), HTML-escaped so text isn't taken as markup, with what must not be
//! translated (code, mentions, custom emoji, timestamps, links, :shortcodes:) swapped for
//! placeholders first; retries, then the plain "gtx" endpoint as a fallback.
//!
//! Many messages go in one request: all their lines in one batch. Results are cached by
//! (from, to, text). The text of a translated message goes to Google; main.rs never asks for a
//! secret chat or an encrypted DM.

use regex::Regex;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

const HTML_ENDPOINT: &str = "https://translate-pa.googleapis.com/v1/translateHtml";
// The public key Google Translate's own web client uses with this endpoint.
const HTML_KEY: &str = "AIzaSyATBXajvzQLTDHEQbcpq0Ihe0vWDHmO520";
const PLAIN_ENDPOINT: &str = "https://translate.googleapis.com/translate_a/single";
const TIMEOUT: Duration = Duration::from_secs(20);
const MAX_TEXT: usize = 4000;
const MAX_LINES: usize = 200;
const CACHE: usize = 2000;

static TOKENS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    [
        r"```[\s\S]*?```",                // fenced code (may span lines)
        r"`[^`\n]+`",                     // inline code
        r"<a?:\w+:\d+>",                  // custom emoji
        r"<@[!&]?\d+>",                   // user and role mentions
        r"<#\d+>",                        // channel mentions
        r"<t:\d+(?::[tTdDfFR])?>",        // timestamps
        r"https?://\S+",                  // links
        r"(?i):[a-z0-9_+\-]+:",           // :shortcode: emoji
    ]
    .iter()
    .map(|p| Regex::new(p).expect("token pattern"))
    .collect()
});
static PLACEHOLDER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"@@\s*(\d+)\s*@@").expect("placeholder"));
static ENTITY: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"&(#[xX]?[0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);").expect("entity"));

/// What must survive translation, swapped for @@i@@ (which survives any script); returns the
/// text and what was taken out. Whole text first, so fenced code spanning lines is one token.
pub fn protect(text: &str) -> (String, Vec<String>) {
    let mut store: Vec<String> = Vec::new();
    let mut out = text.to_owned();
    for pattern in TOKENS.iter() {
        out = pattern
            .replace_all(&out, |c: &regex::Captures| {
                store.push(c[0].to_owned());
                format!("@@{}@@", store.len() - 1)
            })
            .into_owned();
    }
    (out, store)
}

pub fn restore(text: &str, store: &[String]) -> String {
    PLACEHOLDER
        .replace_all(text, |c: &regex::Captures| c[1].parse::<usize>().ok().and_then(|i| store.get(i).cloned()).unwrap_or_else(|| c[0].to_owned()))
        .into_owned()
}

pub fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn decode_entities(s: &str) -> String {
    ENTITY
        .replace_all(s, |c: &regex::Captures| {
            let body = &c[1];
            let decoded = if let Some(num) = body.strip_prefix('#') {
                let code = match num.strip_prefix(['x', 'X']) {
                    Some(hex) => u32::from_str_radix(hex, 16).ok(),
                    None => num.parse::<u32>().ok(),
                };
                code.and_then(char::from_u32).map(String::from)
            } else {
                named(body).map(String::from)
            };
            decoded.unwrap_or_else(|| c[0].to_owned())
        })
        .into_owned()
}

fn named(name: &str) -> Option<&'static str> {
    Some(match name {
        "amp" => "&", "lt" => "<", "gt" => ">", "quot" => "\"", "apos" => "'", "nbsp" => "\u{a0}",
        "laquo" => "«", "raquo" => "»", "hellip" => "…", "mdash" => "—", "ndash" => "–",
        "rsquo" => "’", "lsquo" => "‘", "ldquo" => "“", "rdquo" => "”", "sbquo" => "‚", "bdquo" => "„",
        "copy" => "©", "reg" => "®", "trade" => "™", "deg" => "°", "middot" => "·", "bull" => "•",
        "euro" => "€", "pound" => "£", "cent" => "¢", "times" => "×", "divide" => "÷",
        "frac12" => "½", "frac14" => "¼",
        _ => return None,
    })
}

/// A language code Google takes: "auto", or 2-12 letters and hyphens (zh-CN, haw).
pub fn valid_lang(code: &str) -> bool {
    (2..=12).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_alphabetic() || b == b'-')
}

#[derive(Clone, Debug, PartialEq)]
pub struct Translated {
    pub text: String,
    pub detected: String,
}

static CACHED: LazyLock<Mutex<HashMap<(String, String, String), Translated>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

// One message, split into its lines (blank ones kept in place), protected.
struct Prepared {
    lines: Vec<String>,
    asked: Vec<usize>,
    store: Vec<String>,
}

fn prepare(text: &str) -> Prepared {
    let (protected, store) = protect(text);
    let lines: Vec<String> = protected.split('\n').map(str::to_owned).collect();
    let asked = lines.iter().enumerate().filter(|(_, l)| !l.trim().is_empty()).map(|(i, _)| i).collect();
    Prepared { lines, asked, store }
}

/// translateHtml's answer: [[translated lines…], [detected languages…]].
pub fn read_html_answer(answer: &Value) -> (Vec<Option<String>>, String) {
    let lines = answer[0].as_array().map(|l| l.iter().map(|t| t.as_str().map(decode_entities)).collect()).unwrap_or_default();
    let detected = answer[1].as_array().and_then(|d| d.iter().find_map(|x| x.as_str().filter(|s| !s.is_empty()))).unwrap_or("").to_owned();
    (lines, detected)
}

async fn html_batch(http: &reqwest::Client, from: &str, to: &str, lines: &[String]) -> Result<Value, String> {
    let body = json!([[lines.iter().map(|l| escape_html(l)).collect::<Vec<_>>(), from, to], "wt_lib"]);
    crate::netlog::note(HTML_ENDPOINT);
    let response = http
        .post(HTML_ENDPOINT)
        .header("Content-Type", "application/json+protobuf")
        .header("X-Goog-API-Key", HTML_KEY)
        .body(body.to_string())
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err(format!("translation {}", response.status()));
    }
    response.json::<Value>().await.map_err(|e| e.without_url().to_string())
}

async fn plain(http: &reqwest::Client, from: &str, to: &str, text: &str) -> Result<Translated, String> {
    let (protected, store) = protect(text);
    let url = url::Url::parse_with_params(PLAIN_ENDPOINT, &[("client", "gtx"), ("sl", from), ("tl", to), ("dt", "t"), ("q", protected.as_str())]).map_err(|e| e.to_string())?;
    crate::netlog::note(PLAIN_ENDPOINT);
    let response = http
        .get(url)
        .timeout(TIMEOUT)
        .send()
        .await
        .map_err(|e| e.without_url().to_string())?;
    if !response.status().is_success() {
        return Err(format!("translation {}", response.status()));
    }
    let answer: Value = response.json().await.map_err(|e| e.without_url().to_string())?;
    let joined: String = answer[0].as_array().into_iter().flatten().filter_map(|part| part[0].as_str()).collect();
    Ok(Translated { text: restore(&decode_entities(&joined), &store), detected: answer[2].as_str().unwrap_or("").to_owned() })
}

/// Translates several messages at once; each answer in the same place as its text. Cached ones
/// aren't asked again. One failed batch falls back to the plain endpoint, message by message.
pub async fn translate_many(http: &reqwest::Client, from: &str, to: &str, texts: &[String]) -> Result<Vec<Translated>, String> {
    let key = |t: &str| (from.to_owned(), to.to_owned(), t.to_owned());
    let mut out: Vec<Option<Translated>> = texts.iter().map(|t| CACHED.lock().unwrap().get(&key(t)).cloned()).collect();
    let todo: Vec<usize> = (0..texts.len()).filter(|&i| out[i].is_none()).collect();
    if !todo.is_empty() {
        let prepared: Vec<Prepared> = todo.iter().map(|&i| prepare(&texts[i].chars().take(MAX_TEXT).collect::<String>())).collect();
        let lines: Vec<String> = prepared.iter().flat_map(|p| p.asked.iter().map(|&i| p.lines[i].clone())).take(MAX_LINES).collect();
        let mut answer = Err(String::new());
        for attempt in 0..3u64 {
            answer = html_batch(http, from, to, &lines).await;
            if answer.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(180 * (attempt + 1))).await;
        }
        match answer {
            Ok(answer) => {
                let (translated, detected) = read_html_answer(&answer);
                let mut next = 0;
                for (p, &i) in prepared.iter().zip(&todo) {
                    let mut lines = p.lines.clone();
                    for &line in &p.asked {
                        if let Some(Some(t)) = translated.get(next) {
                            lines[line] = t.clone();
                        }
                        next += 1;
                    }
                    out[i] = Some(Translated { text: restore(&lines.join("\n"), &p.store), detected: detected.clone() });
                }
            }
            Err(_) => {
                for &i in &todo {
                    out[i] = Some(plain(http, from, to, &texts[i]).await?);
                }
            }
        }
        let mut cache = CACHED.lock().unwrap();
        if cache.len() > CACHE {
            cache.clear();
        }
        for &i in &todo {
            if let Some(t) = &out[i] {
                cache.insert(key(&texts[i]), t.clone());
            }
        }
    }
    Ok(out.into_iter().map(|t| t.unwrap_or(Translated { text: String::new(), detected: String::new() })).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_must_not_be_translated_survives() {
        let text = "hi <@123> look https://x.y/z?a=1 :smile: `code` <:blob:42>\n```\nfn main() {}\n``` <t:1700000000:R>";
        let (protected, store) = protect(text);
        assert!(!protected.contains("<@123>") && !protected.contains("https://") && !protected.contains("fn main"));
        assert_eq!(restore(&protected, &store), text);
        // Translation may put spaces inside the placeholder.
        assert_eq!(restore("salut @@ 0 @@", &["<@1>".to_owned()]), "salut <@1>");
        assert_eq!(restore("@@9@@", &[]), "@@9@@");
    }

    #[test]
    fn html_escaping_round_trips() {
        let text = "a < b && c > d";
        assert_eq!(decode_entities(&escape_html(text)), text);
        assert_eq!(decode_entities("&#39;x&#x27;&hellip;&unknown;"), "'x'…&unknown;");
        assert_eq!(decode_entities("&#99999999;"), "&#99999999;");
    }

    #[test]
    fn answers_go_back_line_by_line() {
        let p = prepare("hello\n\nworld");
        assert_eq!(p.asked, [0, 2], "blank lines aren't sent");
        let (lines, detected) = read_html_answer(&json!([["moi &amp; hei", "maailma"], [null, "en"]]));
        assert_eq!(lines, [Some("moi & hei".to_owned()), Some("maailma".to_owned())]);
        assert_eq!(detected, "en");
        assert!(valid_lang("zh-CN") && valid_lang("auto") && !valid_lang("e") && !valid_lang("en;x"));
    }
}
