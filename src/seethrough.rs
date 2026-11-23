//! Liquid Glass's backdrop, live: what is really behind a window, every frame, through
//! rune-glass's see-through capture. The output is captured with the standard protocol, the
//! window's own frame is taken back out of it, and what is left is handed to the glass as the
//! old still capture used to be: the panel with `BLEED` around it, in the output's pixels.
//!
//! EchoSpot's part is what rune-glass leaves to the app: where its window is on which output
//! (Hyprland's IPC, `live::own_window`), and its own frames, which the glass compositor reads
//! back and hands over with `push_ours`.

use std::collections::HashMap;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use rune_glass::{OutputCapture, Ours, Region, SeeThrough};

use crate::backdrop::BLEED;
use crate::theme::size::WINDOW_MARGIN;

/// How opaque the glass is drawn: what it lets through is what makes the screen behind it
/// readable. The glass cancels what it last saw there, so only a change shows, for a frame or
/// two, at this sliver of its strength.
pub const OPACITY: f32 = 0.97;

enum Msg {
    Start,
    Stop,
    /// A frame of the window's: when it was submitted, its size in pixels, premultiplied RGBA.
    Ours(u64, u32, u32, Vec<u8>),
    /// A frame was submitted; its pixels are on their way.
    Submitted(u64),
    Captured,
}

struct Feed {
    tx: Sender<Msg>,
    live: std::sync::Arc<std::sync::atomic::AtomicBool>,
    failed: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

fn feeds() -> &'static Mutex<HashMap<&'static str, Feed>> {
    static FEEDS: OnceLock<Mutex<HashMap<&'static str, Feed>>> = OnceLock::new();
    FEEDS.get_or_init(Default::default)
}

/// Sets up the feed for one window: `window` is its size in points, `deliver` gets each new
/// picture of what is behind its panel, as `backdrop::capture` gives one.
pub fn spawn(name: &'static str, window: (f32, f32), deliver: impl Fn(u32, u32, Vec<u8>) + Send + 'static) {
    let (tx, rx) = channel();
    let live = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let capture = OutputCapture::start();
    let wake = tx.clone();
    capture.on_frame(move || {
        let _ = wake.send(Msg::Captured);
    });
    let failed = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (flag, fail) = (live.clone(), failed.clone());
    let _ = std::thread::Builder::new().name(format!("echospot-seethrough-{name}")).spawn(move || run(window, capture, rx, flag, fail, deliver));
    feeds().lock().unwrap_or_else(|p| p.into_inner()).insert(name, Feed { tx, live, failed });
}

fn send(name: &str, msg: Msg) -> bool {
    feeds().lock().unwrap_or_else(|p| p.into_inner()).get(name).is_some_and(|f| f.tx.send(msg).is_ok())
}

/// The window is showing: follow what is behind it. False when this feed can't (no feed, or
/// the compositor has no standard capture): the caller falls back to its own capture.
pub fn start(name: &str) -> bool {
    let usable = feeds().lock().unwrap_or_else(|p| p.into_inner()).get(name).is_some_and(|f| !f.failed.load(std::sync::atomic::Ordering::Relaxed));
    usable && send(name, Msg::Start)
}

pub fn stop(name: &str) {
    send(name, Msg::Stop);
}

/// Whether the feed is following its window now, with captures coming in.
pub fn live(name: &str) -> bool {
    feeds().lock().unwrap_or_else(|p| p.into_inner()).get(name).is_some_and(|f| f.live.load(std::sync::atomic::Ordering::Relaxed))
}

/// A frame was just submitted: captures that could show it wait for its pixels.
pub fn submitted(name: &str, submitted_ns: u64) {
    send(name, Msg::Submitted(submitted_ns));
}

/// A frame the window just handed to the compositor, as the glass compositor read it back.
pub fn push_ours(name: &str, submitted_ns: u64, w: u32, h: u32, rgba: Vec<u8>) {
    send(name, Msg::Ours(submitted_ns, w, h, rgba));
}

fn run(window: (f32, f32), capture: OutputCapture, rx: Receiver<Msg>, live: std::sync::Arc<std::sync::atomic::AtomicBool>, failed: std::sync::Arc<std::sync::atomic::AtomicBool>, deliver: impl Fn(u32, u32, Vec<u8>)) {
    let mut see = SeeThrough::new();
    // Bare glass is read; anything drawn over it (text, icons, controls) is too opaque to be.
    see.hold = (OPACITY * 255.0 + 1.5) / 255.0;
    let mut active = false;
    // Where the window is: its top-left pixel and the region captured, refreshed as it may move.
    let mut place: Option<((i32, i32), Region)> = None;
    let mut placed_at = Instant::now() - Duration::from_secs(1);
    let mut last_seq = 0u64;
    // A capture waiting for the pixels of a frame of ours that may be in it: solved as soon as
    // they come, rather than passed over for newer captures that would wait the same way.
    let mut waiting: Option<rune_glass::Captured> = None;
    let mut since_start = Instant::now();
    let mut queued: Vec<Msg> = Vec::new();
    loop {
        // Everything that has come in, before any capture is solved: a frame of ours waiting in
        // the queue behind a slow solve would be missing when the capture showing it is.
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(m) => queued.push(m),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return,
        }
        queued.extend(rx.try_iter());
        for msg in queued.drain(..) {
            match msg {
                Msg::Start => {
                    active = true;
                    see.reset();
                    place = None;
                    since_start = Instant::now();
                }
                Msg::Stop => {
                    active = false;
                    waiting = None;
                    capture.set(None, None);
                    live.store(false, std::sync::atomic::Ordering::Relaxed);
                    see.reset();
                }
                Msg::Ours(t, w, h, rgba) => {
                    if let Some((origin, _)) = place {
                        if debug() {
                            let mid = ((h / 2 * w + w / 2) * 4) as usize;
                            eprintln!("seethrough: ours {w}x{h} at {origin:?} t={} centre {:?}", t / 1_000_000, &rgba[mid..mid + 4]);
                        }
                        see.push_ours(Ours { submitted_ns: t, origin, w, h, rgba });
                    }
                }
                Msg::Submitted(t) => {
                    if place.is_some() {
                        see.submitted(t);
                    }
                }
                Msg::Captured => {}
            }
        }
        if !active {
            continue;
        }
        if let Some(e) = capture.error() {
            if std::env::var_os("ECHOSPOT_DEBUG").is_some() {
                eprintln!("echospot seethrough: {e}");
            }
            failed.store(true, std::sync::atomic::Ordering::Relaxed);
            active = false;
            continue;
        }
        // Where the window is, every so often: a summoned window can be moved.
        if place.is_none() || placed_at.elapsed() > Duration::from_millis(250) {
            placed_at = Instant::now();
            match crate::live::own_window(window) {
                Some(on) => {
                    let s = on.scale;
                    let px = |v: f32| (v * s).round() as i32;
                    let (wx, wy) = (on.at.0 - on.monitor.0, on.at.1 - on.monitor.1);
                    // The region reaches past the window by what the glass trims off again
                    // (`glass::Glass`), rounded the same way and taken from the window's own
                    // pixel origin, so the two line up to the pixel at any scale.
                    let origin = (px(wx), px(wy));
                    let out = ((BLEED - WINDOW_MARGIN) * s).round() as i32;
                    let (ww, wh) = ((window.0 * s).round() as i32, (window.1 * s).round() as i32);
                    let region = Region { x: origin.0 - out, y: origin.1 - out, w: (ww + out * 2).max(1) as u32, h: (wh + out * 2).max(1) as u32 };
                    if place != Some((origin, region)) {
                        place = Some((origin, region));
                        capture.set(Some(&on.output), Some(region));
                    }
                }
                None if since_start.elapsed() > Duration::from_secs(2) => {
                    // Gone: closed, or never shown.
                    active = false;
                    capture.set(None, None);
                    live.store(false, std::sync::atomic::Ordering::Relaxed);
                    continue;
                }
                None => continue,
            }
        }
        // What is behind, from the newest capture.
        let cap = match waiting.take() {
            Some(w) => w,
            None => {
                let Some(cap) = capture.latest() else { continue };
                cap
            }
        };
        if cap.seq == last_seq || place.is_none_or(|(_, r)| r != cap.region) {
            continue;
        }
        // A frame of ours that could be in this capture hasn't come back yet: wait for it.
        if !see.ready(cap.presented_ns) {
            waiting = Some(cap);
            continue;
        }
        last_seq = cap.seq;
        if debug() {
            let mid = ((cap.region.h / 2 * cap.region.w + cap.region.w / 2) * 4) as usize;
            eprintln!("seethrough: capture {} {:?} presented={} centre {:?}", cap.seq, cap.region, cap.presented_ns / 1_000_000, &cap.rgba[mid..mid + 4]);
        }
        live.store(true, std::sync::atomic::Ordering::Relaxed);
        let solving = Instant::now();
        see.recover(&cap);
        if !see.fresh {
            continue;
        }
        let behind = see.behind().to_vec();
        if debug() {
            eprintln!("seethrough: solved in {:?}", solving.elapsed());
        }
        if debug() {
            let mid = ((cap.region.h / 2 * cap.region.w + cap.region.w / 2) * 4) as usize;
            eprintln!("seethrough: recovered centre {:?} out {:.3} rejected {}", &behind[mid..mid + 4], see.last.0, see.last.1);
            // The recovered picture and the capture it came from, for looking at.
            if let Some(dir) = std::env::var_os("ECHOSPOT_DUMP") {
                let dir = std::path::PathBuf::from(dir);
                let ppm = |rgba: &[u8]| {
                    let mut out = format!("P6\n{} {}\n255\n", cap.region.w, cap.region.h).into_bytes();
                    for px in rgba.chunks(4) {
                        out.extend(&px[..3]);
                    }
                    out
                };
                let _ = std::fs::write(dir.join("recovered.ppm"), ppm(&behind));
                let _ = std::fs::write(dir.join("captured.ppm"), ppm(&cap.rgba));
                if let Some(o) = see.frame(see.last_frame) {
                    let mut out = format!("{} {} {} {}\n", o.w, o.h, o.origin.0, o.origin.1).into_bytes();
                    out.extend(&o.rgba);
                    let _ = std::fs::write(dir.join("matched.raw"), out);
                }
                let _ = std::fs::write(dir.join("captured.txt"), format!("{} {} {} {}", cap.region.x, cap.region.y, cap.region.w, cap.region.h));
            }
        }
        deliver(cap.region.w, cap.region.h, behind);
    }
}

fn debug() -> bool {
    std::env::var_os("ECHOSPOT_DEBUG").is_some()
}
