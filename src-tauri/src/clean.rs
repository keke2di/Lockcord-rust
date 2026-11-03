//! Cleaning what's sent to Discord (Settings → Privacy):
//!
//!   images  photo metadata is taken out before upload: EXIF (camera, time, GPS position), XMP,
//!           Photoshop/IPTC blocks, comments, text chunks, and anything after the image's end
//!           (phones put previews with their own EXIF there). JPEG keeps only what draws it,
//!           its colour profile, and the rotation (as a minimal EXIF, so photos aren't turned
//!           sideways); PNG keeps its image chunks; WebP drops its EXIF/XMP chunks. Other files
//!           go as they are. An image that claims to be JPEG, PNG or WebP but can't be read
//!           isn't sent at all (it might still carry what cleaning should have removed).
//!   links   tracking parameters (utm_*, fbclid, gclid, si on YouTube/Spotify, ...) are taken out
//!           of links in a message, except inside `code`. A link without any keeps its exact text.

// --- Images ------------------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub enum Kind {
    Jpeg,
    Png,
    Webp,
    Other,
}

pub fn kind_of(bytes: &[u8]) -> Kind {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Kind::Jpeg
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Kind::Png
    } else if bytes.len() >= 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Kind::Webp
    } else {
        Kind::Other
    }
}

/// The file without its metadata; other kinds of file unchanged.
pub fn strip_metadata(bytes: Vec<u8>) -> Result<Vec<u8>, String> {
    let fail = |what: &str| format!("This {what} couldn't be cleaned of its metadata, so it wasn't sent. Turn off “Clean photos” in Settings → Privacy to send it as it is.");
    match kind_of(&bytes) {
        Kind::Jpeg => jpeg(&bytes).ok_or_else(|| fail("JPEG")),
        Kind::Png => png(&bytes).ok_or_else(|| fail("PNG")),
        Kind::Webp => webp(&bytes).ok_or_else(|| fail("WebP")),
        Kind::Other => Ok(bytes),
    }
}

fn be16(b: &[u8], at: usize) -> Option<usize> {
    Some(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?]) as usize)
}

fn jpeg(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(b.len());
    out.extend_from_slice(&[0xFF, 0xD8]);
    let mut i = 2;
    let mut orientation = None;
    let mut wrote_orientation = false;
    loop {
        // A marker: 0xFF, maybe more 0xFF fill bytes, then its code.
        if *b.get(i)? != 0xFF {
            return None;
        }
        while *b.get(i)? == 0xFF {
            i += 1;
        }
        let code = *b.get(i)?;
        i += 1;
        match code {
            0xD9 => {
                // The image's end: whatever follows (previews, MPF images) is left out.
                out.extend_from_slice(&[0xFF, 0xD9]);
                return Some(out);
            }
            0xD0..=0xD7 | 0x01 => {
                out.extend_from_slice(&[0xFF, code]);
                continue;
            }
            _ => {}
        }
        let len = be16(b, i)?;
        if len < 2 || i + len > b.len() {
            return None;
        }
        let segment = &b[i - 2..i + len];
        let data = &b[i + 2..i + len];
        let keep = match code {
            0xE0 | 0xEE => true,                                   // JFIF, Adobe (colour transform)
            0xE2 => data.starts_with(b"ICC_PROFILE\0"),            // the colour profile only
            0xE1 => {
                if data.starts_with(b"Exif\0\0") && orientation.is_none() {
                    orientation = exif_orientation(&data[6..]);
                }
                false
            }
            0xE3..=0xEF | 0xFE => false,                           // other APPn, comments
            _ => true,                                             // what draws the image
        };
        // The rotation goes back in as a minimal EXIF, right after JFIF (or first).
        if !wrote_orientation && code != 0xE0 && code != 0xE1 {
            if let Some(o) = orientation.filter(|o| (2..=8).contains(o)) {
                out.extend_from_slice(&minimal_exif(o));
            }
            wrote_orientation = true;
        }
        if keep {
            out.extend_from_slice(segment);
        }
        i += len;
        if code == 0xDA {
            // Entropy-coded data up to the next real marker (not stuffing 0xFF00, not a restart).
            let start = i;
            loop {
                if i + 1 >= b.len() {
                    return None;
                }
                if b[i] == 0xFF && b[i + 1] != 0x00 && !(0xD0..=0xD7).contains(&b[i + 1]) && b[i + 1] != 0xFF {
                    break;
                }
                i += 1;
            }
            out.extend_from_slice(&b[start..i]);
        }
    }
}

// The Orientation tag (0x0112) of IFD0, from EXIF's TIFF data.
fn exif_orientation(tiff: &[u8]) -> Option<u16> {
    let little = match tiff.get(0..2)? {
        b"II" => true,
        b"MM" => false,
        _ => return None,
    };
    let u16_at = |at: usize| -> Option<u16> {
        let s = [*tiff.get(at)?, *tiff.get(at + 1)?];
        Some(if little { u16::from_le_bytes(s) } else { u16::from_be_bytes(s) })
    };
    let u32_at = |at: usize| -> Option<u32> {
        let s = [*tiff.get(at)?, *tiff.get(at + 1)?, *tiff.get(at + 2)?, *tiff.get(at + 3)?];
        Some(if little { u32::from_le_bytes(s) } else { u32::from_be_bytes(s) })
    };
    let ifd = u32_at(4)? as usize;
    let count = u16_at(ifd)? as usize;
    (0..count.min(256)).find_map(|n| {
        let entry = ifd + 2 + n * 12;
        (u16_at(entry)? == 0x0112).then(|| u16_at(entry + 8)).flatten()
    })
}

// An APP1 segment with EXIF holding only the orientation.
fn minimal_exif(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II*\0");
    tiff.extend_from_slice(&8u32.to_le_bytes());
    tiff.extend_from_slice(&1u16.to_le_bytes());
    tiff.extend_from_slice(&0x0112u16.to_le_bytes());
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes());
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&[0, 0]);
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD
    let mut seg = vec![0xFF, 0xE1];
    seg.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
    seg.extend_from_slice(b"Exif\0\0");
    seg.extend_from_slice(&tiff);
    seg
}

fn png(b: &[u8]) -> Option<Vec<u8>> {
    const DROP: [&[u8; 4]; 5] = [b"tEXt", b"zTXt", b"iTXt", b"tIME", b"eXIf"];
    let mut out = b[..8].to_vec();
    let mut i = 8;
    loop {
        let len = u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?) as usize;
        let kind: &[u8; 4] = b.get(i + 4..i + 8)?.try_into().ok()?;
        let end = i.checked_add(12)?.checked_add(len)?;
        if end > b.len() {
            return None;
        }
        if !DROP.contains(&kind) {
            out.extend_from_slice(&b[i..end]);
        }
        i = end;
        if kind == b"IEND" {
            return Some(out);
        }
    }
}

fn webp(b: &[u8]) -> Option<Vec<u8>> {
    let mut out = b[..12].to_vec();
    // Only what the RIFF header says is the file; anything after it is left out.
    let limit = (8 + u32::from_le_bytes(b[4..8].try_into().ok()?) as usize).min(b.len());
    let mut i = 12;
    let mut vp8x = None;
    while i < limit {
        let kind = b.get(i..i + 4)?;
        let len = u32::from_le_bytes(b.get(i + 4..i + 8)?.try_into().ok()?) as usize;
        let end = i.checked_add(8)?.checked_add(len.checked_add(len & 1)?)?;
        if end > limit {
            // A last chunk without its padding byte is still whole.
            if i + 8 + len != limit {
                return None;
            }
        }
        let end = end.min(limit);
        if kind != b"EXIF" && kind != b"XMP " {
            if kind == b"VP8X" {
                vp8x = Some(out.len());
            }
            out.extend_from_slice(&b[i..end]);
            if (end - i - 8) & 1 == 1 {
                out.push(0);
            }
        }
        i = end;
    }
    // VP8X's flags no longer claim EXIF (0x08) or XMP (0x04).
    if let Some(at) = vp8x {
        *out.get_mut(at + 8)? &= !0x0C;
    }
    let size = (out.len() - 8) as u32;
    out[4..8].copy_from_slice(&size.to_le_bytes());
    Some(out)
}

/// A generic name for an upload ("image.png", "video.mp4", "file.zip"), keeping the extension.
pub fn generic_name(name: &str) -> String {
    let ext = std::path::Path::new(name).extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).filter(|e| e.len() <= 8 && e.bytes().all(|b| b.is_ascii_alphanumeric()));
    let base = match ext.as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "bmp" | "heic") => "image",
        Some("mp4" | "webm" | "mov" | "mkv" | "avi") => "video",
        Some("mp3" | "ogg" | "wav" | "flac" | "m4a" | "opus") => "audio",
        _ => "file",
    };
    // Discord's own spoiler mark is kept.
    let spoiler = if name.starts_with("SPOILER_") { "SPOILER_" } else { "" };
    match ext {
        Some(e) => format!("{spoiler}{base}.{e}"),
        None => format!("{spoiler}{base}"),
    }
}

// --- Links ---------------------------------------------------------------------------------------

// Parameters that only say where a click came from, on any site.
const TRACKERS: [&str; 18] = [
    "fbclid", "gclid", "dclid", "gbraid", "wbraid", "msclkid", "mc_cid", "mc_eid", "igshid", "igsh", "_hsenc", "_hsmi", "yclid", "twclid",
    "ttclid", "ref_src", "_ga", "_gl",
];
// `si` is a share id on these hosts only (elsewhere it can be a real parameter).
const SI_HOSTS: [&str; 6] = ["youtube.com", "www.youtube.com", "m.youtube.com", "music.youtube.com", "youtu.be", "open.spotify.com"];

/// The text with tracking parameters taken out of its links (not inside `code` or ```blocks```).
pub fn clean_links(text: &str) -> String {
    if !text.contains("http") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        // Code is copied as it is.
        let fence = rest.find("```");
        let tick = rest.find('`');
        let code_at = match (fence, tick) {
            (Some(f), Some(t)) => Some(f.min(t)),
            (f, t) => f.or(t),
        };
        let Some(at) = code_at else {
            out.push_str(&clean_plain(rest));
            break;
        };
        out.push_str(&clean_plain(&rest[..at]));
        let marker = if rest[at..].starts_with("```") { "```" } else { "`" };
        let body = &rest[at + marker.len()..];
        match body.find(marker) {
            Some(close) => {
                let end = at + marker.len() + close + marker.len();
                out.push_str(&rest[at..end]);
                rest = &rest[end..];
            }
            None => {
                // An unclosed mark isn't code: the rest is plain text.
                out.push_str(marker);
                rest = body;
                if marker == "`" {
                    out.push_str(&clean_plain(rest));
                    break;
                }
            }
        }
    }
    out
}

fn clean_plain(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(found) = text[i..].find("http").map(|n| i + n) {
        let start_ok = found == 0 || !text[..found].ends_with(|c: char| c.is_alphanumeric());
        let scheme = if text[found..].starts_with("https://") { 8 } else if text[found..].starts_with("http://") { 7 } else { 0 };
        if !start_ok || scheme == 0 {
            out.push_str(&text[i..found + 4]);
            i = found + 4;
            continue;
        }
        out.push_str(&text[i..found]);
        let end = text[found..].find(|c: char| c.is_whitespace() || c == '<' || c == '>').map(|n| found + n).unwrap_or(text.len());
        let (url, tail) = split_tail(&text[found..end]);
        out.push_str(&clean_url(url));
        out.push_str(tail);
        i = end;
    }
    out.push_str(&text[i..]);
    out
}

// Punctuation after a link that isn't part of it: "see https://x.com/a." or "(https://x.com/a)".
fn split_tail(url: &str) -> (&str, &str) {
    let mut end = url.len();
    loop {
        let s = &url[..end];
        let Some(c) = s.chars().last() else { break };
        let unbalanced = c == ')' && s.matches('(').count() < s.matches(')').count();
        if ".,:;!?'\"*_~|".contains(c) || unbalanced {
            end -= c.len_utf8();
        } else {
            break;
        }
    }
    (&url[..end], &url[end..])
}

fn clean_url(url: &str) -> String {
    let Some(q) = url.find('?') else { return url.to_owned() };
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_ascii_lowercase)).unwrap_or_default();
    let (query, fragment) = match url[q + 1..].find('#') {
        Some(h) => (&url[q + 1..q + 1 + h], &url[q + 1 + h..]),
        None => (&url[q + 1..], ""),
    };
    let tracked = |pair: &str| {
        let key = pair.split('=').next().unwrap_or("").to_ascii_lowercase();
        key.starts_with("utm_") || TRACKERS.contains(&key.as_str()) || (key == "si" && SI_HOSTS.contains(&host.as_str()))
    };
    let pairs: Vec<&str> = query.split('&').collect();
    if !pairs.iter().any(|p| tracked(p)) {
        return url.to_owned();
    }
    let kept: Vec<&str> = pairs.into_iter().filter(|p| !p.is_empty() && !tracked(p)).collect();
    let mut out = url[..q].to_owned();
    if !kept.is_empty() {
        out.push('?');
        out.push_str(&kept.join("&"));
    }
    out.push_str(fragment);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exif_with(orientation: u16, extra: &[u8]) -> Vec<u8> {
        // Big-endian TIFF: IFD0 with Orientation, and a GPS-looking blob after it.
        let mut tiff = b"MM\0*".to_vec();
        tiff.extend_from_slice(&8u32.to_be_bytes());
        tiff.extend_from_slice(&1u16.to_be_bytes());
        tiff.extend_from_slice(&0x0112u16.to_be_bytes());
        tiff.extend_from_slice(&3u16.to_be_bytes());
        tiff.extend_from_slice(&1u32.to_be_bytes());
        tiff.extend_from_slice(&orientation.to_be_bytes());
        tiff.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        tiff.extend_from_slice(extra);
        let mut seg = vec![0xFF, 0xE1];
        seg.extend_from_slice(&((2 + 6 + tiff.len()) as u16).to_be_bytes());
        seg.extend_from_slice(b"Exif\0\0");
        seg.extend_from_slice(&tiff);
        seg
    }

    fn segment(code: u8, data: &[u8]) -> Vec<u8> {
        let mut s = vec![0xFF, code];
        s.extend_from_slice(&((data.len() + 2) as u16).to_be_bytes());
        s.extend_from_slice(data);
        s
    }

    fn sample_jpeg(orientation: u16) -> Vec<u8> {
        let mut j = vec![0xFF, 0xD8];
        j.extend(segment(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
        j.extend(exif_with(orientation, b"GPS 60.1699 N 24.9384 E"));
        j.extend(segment(0xE2, b"ICC_PROFILE\0\x01\x01colour"));
        j.extend(segment(0xE2, b"MPF\0secret preview offsets"));
        j.extend(segment(0xED, b"Photoshop 3.0\0IPTC by-line"));
        j.extend(segment(0xFE, b"a comment"));
        j.extend(segment(0xDB, &[0u8; 65]));
        j.extend(segment(0xC0, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]));
        j.extend(segment(0xDA, &[1, 1, 0, 0, 0x3F, 0]));
        j.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56]);
        j.extend_from_slice(&[0xFF, 0xD9]);
        j.extend_from_slice(b"\xFF\xD8trailing preview with Exif GPS");
        j
    }

    fn contains(hay: &[u8], needle: &[u8]) -> bool {
        hay.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn jpeg_loses_its_metadata_but_keeps_the_picture_and_rotation() {
        let out = strip_metadata(sample_jpeg(6)).unwrap();
        for gone in [&b"GPS"[..], b"MPF", b"Photoshop", b"a comment", b"trailing"] {
            assert!(!contains(&out, gone), "{}", String::from_utf8_lossy(gone));
        }
        assert!(contains(&out, b"JFIF") && contains(&out, b"ICC_PROFILE"));
        assert!(contains(&out, &[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56]), "scan data intact");
        assert!(out.ends_with(&[0xFF, 0xD9]));
        let at = out.windows(6).position(|w| w == b"Exif\0\0").expect("minimal EXIF");
        assert_eq!(exif_orientation(&out[at + 6..]), Some(6));
        // Upright photos get no EXIF at all.
        assert!(!contains(&strip_metadata(sample_jpeg(1)).unwrap(), b"Exif"));
        // Cleaning twice changes nothing more.
        let again = strip_metadata(out.clone()).unwrap();
        assert_eq!(again, out);
    }

    #[test]
    fn broken_images_are_refused_and_other_files_pass() {
        let mut cut = sample_jpeg(1);
        cut.truncate(40);
        assert!(strip_metadata(cut).is_err());
        assert!(strip_metadata(b"\x89PNG\r\n\x1a\n\0\0".to_vec()).is_err());
        let zip = b"PK\x03\x04 a zip with GPS inside".to_vec();
        assert_eq!(strip_metadata(zip.clone()).unwrap(), zip);
    }

    fn chunk(kind: &[u8; 4], data: &[u8]) -> Vec<u8> {
        let mut c = (data.len() as u32).to_be_bytes().to_vec();
        c.extend_from_slice(kind);
        c.extend_from_slice(data);
        c.extend_from_slice(&[1, 2, 3, 4]);
        c
    }

    #[test]
    fn png_loses_text_time_and_exif_chunks() {
        let mut p = b"\x89PNG\r\n\x1a\n".to_vec();
        p.extend(chunk(b"IHDR", &[0; 13]));
        p.extend(chunk(b"tEXt", b"Author\0me"));
        p.extend(chunk(b"eXIf", b"MM GPS"));
        p.extend(chunk(b"tIME", &[0; 7]));
        p.extend(chunk(b"IDAT", b"pixels"));
        p.extend(chunk(b"IEND", b""));
        p.extend_from_slice(b"after the end");
        let out = strip_metadata(p).unwrap();
        assert!(contains(&out, b"IHDR") && contains(&out, b"pixels") && contains(&out, b"IEND"));
        for gone in [&b"Author"[..], b"GPS", b"tIME", b"after"] {
            assert!(!contains(&out, gone));
        }
    }

    #[test]
    fn webp_loses_exif_and_xmp_and_its_flags() {
        let mut body = b"WEBP".to_vec();
        let mut vp8x = b"VP8X".to_vec();
        vp8x.extend_from_slice(&10u32.to_le_bytes());
        vp8x.extend_from_slice(&[0x0C | 0x10, 0, 0, 0, 1, 0, 0, 1, 0, 0]);
        body.extend(vp8x);
        for (kind, data) in [(&b"VP8 "[..], &b"frame"[..]), (b"EXIF", b"GPS here"), (b"XMP ", b"<x:xmpmeta/>")] {
            body.extend_from_slice(kind);
            body.extend_from_slice(&(data.len() as u32).to_le_bytes());
            body.extend_from_slice(data);
            if data.len() % 2 == 1 {
                body.push(0);
            }
        }
        let mut w = b"RIFF".to_vec();
        w.extend_from_slice(&(body.len() as u32).to_le_bytes());
        w.extend(body);
        let out = strip_metadata(w).unwrap();
        assert!(!contains(&out, b"GPS") && !contains(&out, b"xmpmeta") && contains(&out, b"frame"));
        assert_eq!(u32::from_le_bytes(out[4..8].try_into().unwrap()) as usize, out.len() - 8);
        assert_eq!(out[20], 0x10, "alpha flag kept, EXIF/XMP flags cleared");
    }

    // Real files, by hand: `LC_CLEAN_DIR=<dir> cargo test clean_real_files -- --ignored` writes
    // <name>.clean.<ext> next to each image there, to check with an image library.
    #[test]
    #[ignore]
    fn clean_real_files() {
        let dir = std::path::PathBuf::from(std::env::var("LC_CLEAN_DIR").expect("LC_CLEAN_DIR"));
        for entry in std::fs::read_dir(&dir).unwrap() {
            let path = entry.unwrap().path();
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            if name.contains(".clean.") {
                continue;
            }
            let (stem, ext) = name.rsplit_once('.').unwrap();
            let out = strip_metadata(std::fs::read(&path).unwrap()).unwrap_or_else(|e| panic!("{name}: {e}"));
            std::fs::write(dir.join(format!("{stem}.clean.{ext}")), out).unwrap();
        }
    }

    #[test]
    fn generic_names_keep_the_kind_and_spoilers() {
        assert_eq!(generic_name("IMG_20261102_GPS.JPG"), "image.jpg");
        assert_eq!(generic_name("SPOILER_my holiday.mp4"), "SPOILER_video.mp4");
        assert_eq!(generic_name("taxes 2026.pdf"), "file.pdf");
        assert_eq!(generic_name("README"), "file");
    }

    #[test]
    fn links_lose_tracking_parameters_only() {
        assert_eq!(clean_links("look https://shop.example/item?id=7&utm_source=x&utm_medium=y!"), "look https://shop.example/item?id=7!");
        assert_eq!(clean_links("https://youtu.be/abc?si=TRACK&t=42"), "https://youtu.be/abc?t=42");
        assert_eq!(clean_links("https://example.com/?si=keep"), "https://example.com/?si=keep");
        assert_eq!(clean_links("(https://x.com/a?fbclid=1)"), "(https://x.com/a)");
        assert_eq!(clean_links("https://a.example/p?gclid=1#top"), "https://a.example/p#top");
        assert_eq!(clean_links("[shop](https://s.example/?utm_campaign=z)"), "[shop](https://s.example/)");
        assert_eq!(clean_links("<https://s.example/?utm_x=1>"), "<https://s.example/>");
        // Code is left alone, and a link without trackers keeps its exact text.
        assert_eq!(clean_links("`https://x.com/?utm_a=1` and ```https://y.com/?fbclid=2```"), "`https://x.com/?utm_a=1` and ```https://y.com/?fbclid=2```");
        assert_eq!(clean_links("https://x.com/a%20b?q=%41&x=1"), "https://x.com/a%20b?q=%41&x=1");
        assert_eq!(clean_links("no links, just http talk"), "no links, just http talk");
        assert_eq!(clean_links("an ` unclosed https://x.com/?utm_a=1"), "an ` unclosed https://x.com/");
    }
}
