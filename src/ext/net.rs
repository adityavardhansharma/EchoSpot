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

/// GETs a URL and returns its body. Blocking: call from a worker.
pub fn get(url: &str) -> Option<String> {
    util::output("curl", &["-sL", "--max-time", "20", "-A", "Mozilla/5.0 (X11; Linux x86_64) EchoSpot", url])
}
