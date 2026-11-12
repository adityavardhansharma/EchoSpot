//! Remote pictures (store icons, an extension's thumbnails), fetched once with `curl` and kept
//! on disk. A view asks for a URL's file; a miss starts the download and the view redraws when
//! it lands.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

use rune::Waker;

use crate::util;

static WAKER: OnceLock<Waker> = OnceLock::new();
static DIRTY: AtomicBool = AtomicBool::new(false);
static IN_FLIGHT: Mutex<Option<HashSet<String>>> = Mutex::new(None);

pub fn set_waker(waker: Waker) {
    let _ = WAKER.set(waker);
}

pub fn dir() -> PathBuf {
    let dir = std::env::var("XDG_CACHE_HOME").map(PathBuf::from).unwrap_or_else(|_| util::home().join(".cache")).join("echospot/images");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// True once since the last call if a download finished.
pub fn take_dirty() -> bool {
    DIRTY.swap(false, Ordering::SeqCst)
}

fn key(url: &str) -> String {
    // FNV-1a: a stable file name for a URL.
    let mut hash: u64 = 0xcbf29ce484222325;
    for b in url.bytes() {
        hash ^= b as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// The cached file for a URL; `None` while it downloads (or after it failed).
pub fn cached(url: &str) -> Option<PathBuf> {
    let path = dir().join(key(url));
    if path.is_file() {
        return std::fs::metadata(&path).is_ok_and(|m| m.len() > 0).then_some(path);
    }
    let mut guard = IN_FLIGHT.lock().ok()?;
    let in_flight = guard.get_or_insert_with(HashSet::new);
    if !in_flight.insert(url.to_string()) || in_flight.len() > 400 {
        return None;
    }
    let (url, target) = (url.to_string(), path);
    std::thread::spawn(move || {
        let tmp = target.with_extension("part");
        let ok = std::process::Command::new("curl")
            .args(["-sL", "--max-time", "20", "--max-filesize", "8000000", "-o"]).arg(&tmp).arg(&url)
            .status().is_ok_and(|s| s.success());
        if ok {
            let _ = std::fs::rename(&tmp, &target);
        } else {
            // An empty file marks the failure, so the URL isn't retried every frame.
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::write(&target, b"");
        }
        DIRTY.store(true, Ordering::SeqCst);
        if let Some(waker) = WAKER.get() {
            waker();
        }
    });
    None
}

/// Whether a downloaded file starts like a picture the image cache can decode, rather than the
/// HTML page a site answers a missing icon with.
fn is_picture(path: &std::path::Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 12];
    let Ok(n) = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)) else { return false };
    let head = &head[..n];
    head.starts_with(b"\x89PNG") || head.starts_with(&[0, 0, 1, 0]) || head.starts_with(&[0xff, 0xd8])
        || head.starts_with(b"GIF8") || (head.starts_with(b"RIFF") && head.ends_with(b"WEBP")) || head.starts_with(b"BM")
}

/// A site's icon, for a quicklink's row. The site itself is asked first, so a link's host is
/// only told to a third party (DuckDuckGo's icon service) when the site has no `favicon.ico`.
pub fn favicon(host: &str) -> Option<PathBuf> {
    let name = format!("favicon:{host}");
    let path = dir().join(key(&name));
    if path.is_file() {
        return std::fs::metadata(&path).is_ok_and(|m| m.len() > 0).then_some(path);
    }
    let mut guard = IN_FLIGHT.lock().ok()?;
    if !guard.get_or_insert_with(HashSet::new).insert(name) {
        return None;
    }
    let (host, target) = (host.to_string(), path);
    std::thread::spawn(move || {
        let tmp = target.with_extension("part");
        let sources = [format!("https://{host}/favicon.ico"), format!("https://icons.duckduckgo.com/ip3/{host}.ico")];
        let found = sources.iter().any(|url| {
            std::process::Command::new("curl")
                .args(["-sfL", "--max-time", "10", "--max-filesize", "2000000", "-A", "Mozilla/5.0 (X11; Linux x86_64) EchoSpot", "-o"]).arg(&tmp).arg(url)
                .status().is_ok_and(|s| s.success()) && is_picture(&tmp)
        });
        if found {
            let _ = std::fs::rename(&tmp, &target);
        } else {
            let _ = std::fs::remove_file(&tmp);
            let _ = std::fs::write(&target, b"");
        }
        DIRTY.store(true, Ordering::SeqCst);
        if let Some(waker) = WAKER.get() {
            waker();
        }
    });
    None
}

fn base64(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits) = (0u32, 0u32);
    for c in text.bytes().filter(|c| !c.is_ascii_whitespace() && *c != b'=') {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        };
        acc = (acc << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

pub fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let (mut out, mut i) = (Vec::with_capacity(bytes.len()), 0);
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len()
            && let Ok(v) = u8::from_str_radix(&text[i + 1..i + 3], 16) {
            out.push(v);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// A `data:` URI's picture as a file in the cache: the inline SVG or PNG an extension draws its
/// own icons with. Written once per distinct URI.
pub fn data_uri(uri: &str) -> Option<PathBuf> {
    let (meta, data) = uri.strip_prefix("data:")?.split_once(',')?;
    let mime = meta.split(';').next().unwrap_or("");
    let ext = match mime {
        "image/svg+xml" => "svg",
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => return None,
    };
    let path = dir().join(format!("{}.{ext}", key(uri)));
    if !path.is_file() {
        let bytes = if meta.ends_with(";base64") { base64(data)? } else { percent_decode(data) };
        std::fs::write(&path, bytes).ok()?;
    }
    Some(path)
}

/// GETs a URL and returns its body. Blocking: call from a worker.
pub fn get(url: &str) -> Option<String> {
    util::output("curl", &["-sL", "--max-time", "20", "-A", "Mozilla/5.0 (X11; Linux x86_64) EchoSpot", url])
}
