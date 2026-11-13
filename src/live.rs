//! The live backdrop. A screen capture taken while a panel is up would include the panel, so what
//! is behind it is built instead: the wallpaper, then every window on the monitor's current
//! workspace that reaches behind the panel, each read on its own through Hyprland's toplevel
//! export and laid down in stacking order. It runs only while its window is open, so the panel's
//! blur or glass follows a workspace switch, a window moving and a video playing, and a closed
//! window costs nothing. The still capture taken as a window is summoned covers the first frames.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::AsFd;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};

use serde::Deserialize;
use wayland_client::protocol::{wl_buffer::WlBuffer, wl_registry::WlRegistry, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop, globals::{GlobalListContents, registry_queue_init}};

use crate::backdrop::BLEED;
use crate::theme::size::WINDOW_MARGIN;

#[allow(unused_imports, dead_code, non_camel_case_types, non_snake_case, clippy::all)]
mod export {
    use wayland_client;
    use wayland_client::protocol::*;
    use wayland_protocols_wlr::foreign_toplevel::v1::client::*;
    pub mod __interfaces {
        use wayland_client::protocol::__interfaces::*;
        use wayland_protocols_wlr::foreign_toplevel::v1::client::__interfaces::*;
        wayland_scanner::generate_interfaces!("protocols/hyprland-toplevel-export-v1.xml");
    }
    use self::__interfaces::*;
    wayland_scanner::generate_client_code!("protocols/hyprland-toplevel-export-v1.xml");
}
use export::hyprland_toplevel_export_frame_v1::{self as frame, HyprlandToplevelExportFrameV1 as Frame};
use export::hyprland_toplevel_export_manager_v1::HyprlandToplevelExportManagerV1 as Export;

/// How often an open window looks at how the screen behind it is arranged. That is one small
/// read of Hyprland's socket; the windows themselves are read again only when the arrangement
/// changed (a workspace switch, a window opened, moved or closed), or `REFRESH` has passed.
const FPS: f32 = 30.0;
/// How often the windows behind are re-read while nothing is rearranged, for what they show.
const REFRESH: Duration = Duration::from_millis(400);
/// How long a summoned window may take to appear before its backdrop stops waiting for it.
const APPEAR: Duration = Duration::from_secs(2);

// ---- Hyprland ---------------------------------------------------------------------------

#[derive(Default, Deserialize)]
struct WorkspaceRef {
    #[serde(default)]
    id: i64,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Client {
    address: String,
    #[serde(default)]
    mapped: bool,
    #[serde(default)]
    hidden: bool,
    #[serde(default)]
    at: [i32; 2],
    #[serde(default)]
    size: [i32; 2],
    #[serde(default)]
    workspace: WorkspaceRef,
    #[serde(default)]
    floating: bool,
    #[serde(default)]
    monitor: i64,
    #[serde(default)]
    pid: i64,
    #[serde(default)]
    fullscreen: i64,
    #[serde(default, rename = "focusHistoryID")]
    focus_history_id: i64,
}

#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Monitor {
    #[serde(default)]
    id: i64,
    #[serde(default)]
    width: i64,
    #[serde(default)]
    height: i64,
    #[serde(default)]
    x: i64,
    #[serde(default)]
    y: i64,
    #[serde(default)]
    scale: f64,
    #[serde(default)]
    active_workspace: WorkspaceRef,
    #[serde(default)]
    special_workspace: WorkspaceRef,
}

/// One request on Hyprland's command socket, its reply as text: no `hyprctl` process per frame.
fn request(command: &str) -> Option<String> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let mut s = UnixStream::connect(crate::util::runtime_dir().join("hypr").join(sig).join(".socket.sock")).ok()?;
    s.set_read_timeout(Some(Duration::from_secs(2))).ok();
    s.write_all(command.as_bytes()).ok()?;
    let mut out = String::new();
    s.read_to_string(&mut out).ok()?;
    Some(out)
}

fn hypr<T: for<'de> Deserialize<'de>>(what: &str) -> Option<T> {
    serde_json::from_str(&request(&format!("j/{what}"))?).ok()
}

/// EchoSpot's windows are a panel inside a clear margin, where the panel's own shadow falls. A
/// shadow from Hyprland is drawn around the whole window instead, as a rectangle standing off
/// the panel. The window rules ask for none, but a rule added later wins: Omarchy's floating
/// workspaces turn shadows back on for every window on them. A property set on the window itself
/// outranks any rule, so each window EchoSpot opens is told so as it appears.
pub fn own_shadows_off() {
    static WATCHING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    static DONE: Mutex<Vec<String>> = Mutex::new(Vec::new());
    // One watcher at a time; a window summoned while it runs is seen by it.
    if WATCHING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    let _ = std::thread::Builder::new().name("echospot-shadow".into()).spawn(|| {
        let pid = std::process::id() as i64;
        let until = Instant::now() + APPEAR;
        while Instant::now() < until {
            let clients: Vec<Client> = hypr("clients").unwrap_or_default();
            let mut done = DONE.lock().unwrap();
            done.retain(|address| clients.iter().any(|c| &c.address == address));
            for c in clients.iter().filter(|c| c.pid == pid) {
                if !done.contains(&c.address) && request(&format!("/dispatch hl.dsp.window.set_prop({{ window = \"address:{}\", prop = \"no_shadow\", value = \"1\" }})", c.address)).is_some() {
                    done.push(c.address.clone());
                }
            }
            drop(done);
            std::thread::sleep(Duration::from_millis(8));
        }
        WATCHING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

// ---- The handle a window holds -----------------------------------------------------------

#[derive(Default)]
struct LiveState {
    active: bool,
    /// Counts summons, so a window shown again is waited for afresh.
    summons: u64,
}

/// One window's live backdrop: asleep until the window is summoned.
#[derive(Clone)]
pub struct Live(Arc<(Mutex<LiveState>, Condvar)>);

impl Live {
    /// The window is about to show: start following what is behind it.
    pub fn start(&self) {
        let (m, cv) = &*self.0;
        let mut s = m.lock().unwrap();
        s.active = true;
        s.summons += 1;
        cv.notify_all();
    }

    fn stop(&self, summons: u64) {
        let mut s = self.0.0.lock().unwrap();
        if s.summons == summons {
            s.active = false;
        }
    }
}

static PALETTE: OnceLock<Live> = OnceLock::new();
static SETTINGS: OnceLock<Live> = OnceLock::new();

/// Starts the palette's backdrop following the screen. Nothing without `spawn_palette`.
pub fn start_palette() {
    if let Some(live) = PALETTE.get() {
        live.start();
    }
}

pub fn start_settings() {
    if let Some(live) = SETTINGS.get() {
        live.start();
    }
}

/// `window` is the palette window's size in points; `deliver` is handed each new picture of the
/// screen behind its panel (with `BLEED` around it), as `backdrop::capture` returns one.
pub fn spawn_palette(window: (f32, f32), deliver: impl Fn(u32, u32, Vec<u8>) + Send + 'static) {
    let _ = PALETTE.set(spawn("palette", window, deliver));
}

pub fn spawn_settings(window: (f32, f32), deliver: impl Fn(u32, u32, Vec<u8>) + Send + 'static) {
    let _ = SETTINGS.set(spawn("settings", window, deliver));
}

fn spawn(name: &str, window: (f32, f32), deliver: impl Fn(u32, u32, Vec<u8>) + Send + 'static) -> Live {
    let live = Live(Arc::new((Mutex::new(LiveState::default()), Condvar::new())));
    let me = live.clone();
    let _ = std::thread::Builder::new().name(format!("echospot-live-{name}")).spawn(move || {
        if let Err(e) = run(window, &deliver, &me) {
            debug(&format!("unavailable ({e}); panels keep the capture taken as they open"));
        }
    });
    live
}

// ---- Capture ----------------------------------------------------------------------------

/// One shm buffer, reused while the window keeps its size.
struct Shm {
    buffer: WlBuffer,
    map: *mut u8,
    len: usize,
    key: (u32, u32, u32, u32),
}

unsafe impl Send for Shm {}

impl Drop for Shm {
    fn drop(&mut self) {
        self.buffer.destroy();
        unsafe { let _ = rustix::mm::munmap(self.map.cast(), self.len); }
    }
}

#[derive(Default)]
struct Grab {
    offer: Option<(wl_shm::Format, u32, u32, u32)>,
    buffer_done: bool,
    inverted: bool,
    ready: bool,
    failed: bool,
}

struct State {
    grab: Grab,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: wayland_client::protocol::wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}
delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlBuffer);
delegate_noop!(State: ignore Export);

impl Dispatch<Frame, ()> for State {
    fn event(s: &mut Self, f: &Frame, event: frame::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            frame::Event::Buffer { format: WEnum::Value(format), width, height, stride } => {
                if s.grab.offer.is_none() { s.grab.offer = Some((format, width, height, stride)); }
            }
            frame::Event::BufferDone => s.grab.buffer_done = true,
            frame::Event::Flags { flags: WEnum::Value(flags) } => s.grab.inverted = flags.contains(frame::Flags::YInvert),
            frame::Event::Ready { .. } => { s.grab.ready = true; f.destroy(); }
            frame::Event::Failed => { s.grab.failed = true; f.destroy(); }
            _ => {}
        }
    }
}

struct Wallpaper {
    path: PathBuf,
    size: (u32, u32),
    rgba: Vec<u8>,
    checked: Instant,
}

fn wallpaper_path() -> Option<PathBuf> {
    std::fs::canonicalize(crate::util::home().join(".local/state/omarchy/current/background")).ok()
}

/// The wallpaper scaled to cover the monitor, as Omarchy draws it.
fn load_wallpaper(path: &PathBuf, w: u32, h: u32) -> Option<Vec<u8>> {
    let img = image::open(path).ok()?;
    Some(img.resize_to_fill(w, h, image::imageops::FilterType::Triangle).to_rgba8().into_raw())
}

fn run(window: (f32, f32), deliver: &dyn Fn(u32, u32, Vec<u8>), live: &Live) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
    let export: Export = globals.bind(&qh, 1..=2, ()).map_err(|_| "Hyprland has no toplevel export")?;
    let mut state = State { grab: Grab::default() };
    let mut buffers: HashMap<String, Shm> = HashMap::new();
    let mut wall: Option<Wallpaper> = None;
    let mut band: Vec<u8> = Vec::new();
    let mut sent: Vec<u8> = Vec::new();
    // When the windows behind were last read, and how they were arranged then.
    let mut read: Option<(Instant, (i64, i64, [i32; 2], Vec<(String, [i32; 2], [i32; 2])>))> = None;
    let pid = std::process::id() as i64;
    // What this summons has seen of its own window: when it was asked for, and whether it showed.
    let mut following = (0u64, Instant::now(), false);
    let frame = Duration::from_secs_f32(1.0 / FPS);
    loop {
        // Sleep until the window is summoned.
        let summons = {
            let (m, cv) = &*live.0;
            let mut s = m.lock().unwrap();
            while !s.active { s = cv.wait(s).unwrap(); }
            s.summons
        };
        if following.0 != summons {
            following = (summons, Instant::now(), false);
            sent.clear();
            read = None;
        }
        let started = Instant::now();
        let clients: Vec<Client> = hypr::<Vec<Client>>("clients").unwrap_or_default().into_iter().filter(|c| c.mapped).collect();
        // This process's window of this size is the one whose backdrop this is.
        let own = clients.iter().find(|c| c.pid == pid && (c.size[0] as f32 - window.0).abs() <= 2.0 && (c.size[1] as f32 - window.1).abs() <= 2.0);
        let Some(own) = own else {
            // Gone after showing: closed. Never shown: still opening, for a while.
            if following.2 || following.1.elapsed() > APPEAR {
                live.stop(summons);
                buffers.clear();
            } else {
                std::thread::sleep(frame);
            }
            continue;
        };
        following.2 = true;
        let Some(m) = hypr::<Vec<Monitor>>("monitors").unwrap_or_default().into_iter().find(|m| m.id == own.monitor) else {
            std::thread::sleep(Duration::from_millis(500));
            continue;
        };
        let scale = if m.scale > 0.0 { m.scale as f32 } else { 1.0 };
        let (mw, mh) = (m.width.max(1) as u32, m.height.max(1) as u32);
        // Wallpaper, reloaded when Omarchy switches it.
        let stale = wall.as_ref().is_none_or(|w| w.size != (mw, mh) || (w.checked.elapsed() > Duration::from_secs(2) && wallpaper_path().as_ref() != Some(&w.path)));
        if stale {
            if let Some(p) = wallpaper_path() && let Some(rgba) = load_wallpaper(&p, mw, mh) {
                wall = Some(Wallpaper { path: p, size: (mw, mh), rgba, checked: Instant::now() });
            }
        } else if let Some(w) = &mut wall && w.checked.elapsed() > Duration::from_secs(2) {
            w.checked = Instant::now();
        }
        // The panel with its bleed: in layout points, then in the monitor's pixels.
        let inset = WINDOW_MARGIN - BLEED;
        let rect = [own.at[0] as f32 + inset, own.at[1] as f32 + inset, window.0 - inset * 2.0, window.1 - inset * 2.0];
        let bx = ((rect[0] - m.x as f32) * scale).round() as i64;
        let by = ((rect[1] - m.y as f32) * scale).round() as i64;
        let bw = (rect[2] * scale).round().max(1.0) as u32;
        let bh = (rect[3] * scale).round().max(1.0) as u32;
        band.clear();
        band.resize((bw * bh * 4) as usize, 0);
        if let Some(w) = &wall {
            let x0 = bx.clamp(0, mw as i64);
            let x1 = (bx + bw as i64).clamp(0, mw as i64);
            for ty in 0..bh as i64 {
                let my = by + ty;
                if my < 0 || my >= mh as i64 || x1 <= x0 { continue; }
                let src = ((my * mw as i64 + x0) * 4) as usize;
                let dst = ((ty * bw as i64 + (x0 - bx)) * 4) as usize;
                let n = ((x1 - x0) * 4) as usize;
                band[dst..dst + n].copy_from_slice(&w.rgba[src..src + n]);
            }
        }
        // Windows behind the panel, bottom to top; never EchoSpot's own.
        let mut wins: Vec<&Client> = clients.iter().filter(|c| {
            let on = c.workspace.id == m.active_workspace.id || (m.special_workspace.id != 0 && c.workspace.id == m.special_workspace.id);
            let (x0, y0) = (c.at[0] as f32, c.at[1] as f32);
            on && !c.hidden && c.pid != pid
                && x0 < rect[0] + rect[2] && x0 + c.size[0] as f32 > rect[0]
                && y0 < rect[1] + rect[3] && y0 + c.size[1] as f32 > rect[1]
        }).collect();
        wins.sort_by_key(|c| (c.workspace.id < 0, c.fullscreen > 0, c.floating, -c.focus_history_id));
        debug(&format!("ws {} rect {rect:?}: {} windows", m.active_workspace.id, wins.len()));
        // Reading a window back costs the compositor a render: only when there is a reason to.
        let arrangement = (m.active_workspace.id, m.special_workspace.id, own.at, wins.iter().map(|c| (c.address.clone(), c.at, c.size)).collect::<Vec<_>>());
        if read.as_ref().is_some_and(|(at, was)| *was == arrangement && at.elapsed() < REFRESH) {
            if let Some(left) = frame.checked_sub(started.elapsed()) { std::thread::sleep(left); }
            continue;
        }
        read = Some((Instant::now(), arrangement));
        let mut seen = Vec::new();
        for c in &wins {
            seen.push(c.address.clone());
            if let Some((px, fmt, stride, inverted)) = grab(&conn, &mut queue, &qh, &shm, &export, &mut state, &mut buffers, c)? {
                composite(&mut band, (bx, by, bw, bh), &m, c, px, fmt, stride, inverted, scale);
            }
        }
        buffers.retain(|k, _| seen.contains(k));
        // A still screen delivers nothing, and the window draws nothing.
        if band != sent {
            sent.clone_from(&band);
            deliver(bw, bh, band.clone());
        }
        if let Some(left) = frame.checked_sub(started.elapsed()) { std::thread::sleep(left); }
    }
}

/// Captures one window; the pixels stay in its shm buffer.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
fn grab<'a>(
    conn: &Connection, queue: &mut wayland_client::EventQueue<State>, qh: &QueueHandle<State>, shm: &WlShm, export: &Export,
    state: &mut State, buffers: &'a mut HashMap<String, Shm>, c: &Client,
) -> Result<Option<(&'a [u8], (wl_shm::Format, u32, u32, u32), u32, bool)>, Box<dyn std::error::Error>> {
    let addr = u64::from_str_radix(c.address.trim_start_matches("0x"), 16).unwrap_or(0);
    state.grab = Grab::default();
    let f = export.capture_toplevel(0, addr as u32, qh, ());
    let deadline = Instant::now() + Duration::from_millis(400);
    while !(state.grab.buffer_done || state.grab.failed) {
        if Instant::now() > deadline { f.destroy(); return Ok(None); }
        conn.flush()?;
        queue.blocking_dispatch(state)?;
    }
    let Some((format, w, h, stride)) = state.grab.offer.filter(|_| !state.grab.failed) else { return Ok(None) };
    let key = (format as u32, w, h, stride);
    if buffers.get(&c.address).is_none_or(|b| b.key != key) {
        let len = (stride * h) as usize;
        let fd = rustix::fs::memfd_create("echospot-live", rustix::fs::MemfdFlags::CLOEXEC)?;
        rustix::fs::ftruncate(&fd, len as u64)?;
        let map = unsafe { rustix::mm::mmap(std::ptr::null_mut(), len, rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE, rustix::mm::MapFlags::SHARED, &fd, 0)? };
        let pool = shm.create_pool(fd.as_fd(), len as i32, qh, ());
        let buffer = pool.create_buffer(0, w as i32, h as i32, stride as i32, format, qh, ());
        pool.destroy();
        buffers.insert(c.address.clone(), Shm { buffer, map: map.cast(), len, key });
    }
    let b = &buffers[&c.address];
    f.copy(&b.buffer, 1);
    while !(state.grab.ready || state.grab.failed) {
        if Instant::now() > deadline { return Ok(None); }
        conn.flush()?;
        queue.blocking_dispatch(state)?;
    }
    if state.grab.failed { return Ok(None); }
    let px = unsafe { std::slice::from_raw_parts(b.map as *const u8, b.len) };
    Ok(Some((px, (format, w, h, stride), stride, state.grab.inverted)))
}

fn debug(msg: &str) {
    if std::env::var_os("ECHOSPOT_DEBUG").is_some() { eprintln!("echospot live: {msg}"); }
}

/// Lays a window's pixels over the band (premultiplied source-over), only where it covers it.
/// `band` is `(x, y, w, h)` in the monitor's pixels.
#[allow(clippy::too_many_arguments)]
fn composite(dst: &mut [u8], band: (i64, i64, u32, u32), m: &Monitor, c: &Client, px: &[u8], fmt: (wl_shm::Format, u32, u32, u32), stride: u32, inverted: bool, scale: f32) {
    let (bx, by, bw, bh) = band;
    let (format, fw, fh, _) = fmt;
    let (ri, bi, has_alpha) = match format {
        wl_shm::Format::Argb8888 => (2, 0, true),
        wl_shm::Format::Xrgb8888 => (2, 0, false),
        wl_shm::Format::Abgr8888 => (0, 2, true),
        wl_shm::Format::Xbgr8888 => (0, 2, false),
        _ => return,
    };
    // Where the window lands, in monitor pixels.
    let x0 = ((c.at[0] as f32 - m.x as f32) * scale).round() as i64;
    let y0 = ((c.at[1] as f32 - m.y as f32) * scale).round() as i64;
    let ww = ((c.size[0] as f32) * scale).round().max(1.0) as i64;
    let wh = ((c.size[1] as f32) * scale).round().max(1.0) as i64;
    let xa = x0.max(bx);
    let xb = (x0 + ww).min(bx + bw as i64);
    for ty in 0..bh as i64 {
        let wy = by + ty - y0;
        if wy < 0 || wy >= wh { continue; }
        let mut sy = (wy * fh as i64 / wh) as u32;
        if inverted { sy = fh - 1 - sy; }
        let src_row = (sy * stride) as usize;
        let dst_row = (ty as u32 * bw * 4) as usize;
        for mx in xa..xb {
            let sx = ((mx - x0) * fw as i64 / ww) as usize;
            let s = src_row + sx * 4;
            let d = dst_row + (mx - bx) as usize * 4;
            if s + 3 >= px.len() { break; }
            let a = if has_alpha { px[s + 3] as u32 } else { 255 };
            if a == 255 {
                dst[d] = px[s + ri];
                dst[d + 1] = px[s + 1];
                dst[d + 2] = px[s + bi];
            } else if a > 0 {
                let inv = 255 - a;
                dst[d] = (px[s + ri] as u32 + dst[d] as u32 * inv / 255).min(255) as u8;
                dst[d + 1] = (px[s + 1] as u32 + dst[d + 1] as u32 * inv / 255).min(255) as u8;
                dst[d + 2] = (px[s + bi] as u32 + dst[d + 2] as u32 * inv / 255).min(255) as u8;
            }
            dst[d + 3] = 255;
        }
    }
}
