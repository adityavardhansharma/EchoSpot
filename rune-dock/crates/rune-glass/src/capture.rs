//! A region of one output, kept captured through `ext-image-copy-capture-v1`: a new frame each
//! time the output shows something new, stamped with when it was presented.

use std::os::fd::AsFd;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use wayland_client::protocol::{wl_buffer::WlBuffer, wl_output, wl_output::WlOutput, wl_registry::WlRegistry, wl_shm, wl_shm::WlShm, wl_shm_pool::WlShmPool};
use wayland_client::{Connection, Dispatch, QueueHandle, WEnum, delegate_noop, globals::{GlobalListContents, registry_queue_init}};
use wayland_protocols::ext::image_capture_source::v1::client::{
    ext_image_capture_source_v1::ExtImageCaptureSourceV1 as Source,
    ext_output_image_capture_source_manager_v1::ExtOutputImageCaptureSourceManagerV1 as SourceManager,
};
use wayland_protocols::ext::image_copy_capture::v1::client::{
    ext_image_copy_capture_frame_v1::{self as frame, ExtImageCopyCaptureFrameV1 as Frame},
    ext_image_copy_capture_manager_v1::{ExtImageCopyCaptureManagerV1 as Manager, Options},
    ext_image_copy_capture_session_v1::{self as session, ExtImageCopyCaptureSessionV1 as Session},
};

use crate::Region;

/// One capture of the region asked for.
#[derive(Clone)]
pub struct Captured {
    /// Counts up with each capture.
    pub seq: u64,
    /// Where it is on the output, in buffer pixels.
    pub region: Region,
    /// RGBA, 8-bit sRGB, `region.w * region.h * 4` bytes.
    pub rgba: Arc<Vec<u8>>,
    /// When the output showed it, in [`crate::now_ns`]'s clock.
    pub presented_ns: u64,
    /// The output's whole size in buffer pixels.
    pub output: (u32, u32),
}

#[derive(Default)]
struct Wanted {
    output: Option<String>,
    region: Option<Region>,
    stop: bool,
}

struct Shared {
    wanted: Mutex<Wanted>,
    wake: Condvar,
    latest: Mutex<Option<Captured>>,
    error: Mutex<Option<String>>,
    on_frame: Mutex<Option<Box<dyn Fn() + Send>>>,
}

/// Keeps a region of an output captured on its own thread while a region is set.
#[derive(Clone)]
pub struct OutputCapture(Arc<Shared>);

impl OutputCapture {
    /// Starts the capture thread. It connects to the compositor the environment names and
    /// captures nothing until [`OutputCapture::set`] gives it a region.
    pub fn start() -> Self {
        let shared = Arc::new(Shared { wanted: Mutex::new(Wanted::default()), wake: Condvar::new(), latest: Mutex::new(None), error: Mutex::new(None), on_frame: Mutex::new(None) });
        let me = shared.clone();
        let _ = std::thread::Builder::new().name("rune-glass-capture".into()).spawn(move || {
            if let Err(e) = run(&me) {
                *me.error.lock().unwrap_or_else(|p| p.into_inner()) = Some(e.to_string());
            }
        });
        OutputCapture(shared)
    }

    /// What to capture: a region of the output named `output` (`eDP-1`; `None` is the first),
    /// in its buffer pixels. `None` for the region pauses capturing.
    pub fn set(&self, output: Option<&str>, region: Option<Region>) {
        let mut w = self.0.wanted.lock().unwrap_or_else(|p| p.into_inner());
        w.output = output.map(String::from);
        w.region = region;
        self.0.wake.notify_all();
    }

    /// The newest capture.
    pub fn latest(&self) -> Option<Captured> {
        self.0.latest.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// Called on the capture thread after each new capture: wake whoever draws.
    pub fn on_frame(&self, f: impl Fn() + Send + 'static) {
        *self.0.on_frame.lock().unwrap_or_else(|p| p.into_inner()) = Some(Box::new(f));
    }

    /// Why capturing stopped, when it did: no compositor, no capture protocol.
    pub fn error(&self) -> Option<String> {
        self.0.error.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }
}


impl OutputCapture {
    pub fn stop(&self) {
        self.0.wanted.lock().unwrap_or_else(|p| p.into_inner()).stop = true;
        self.0.wake.notify_all();
    }
}

// ---- The Wayland side -------------------------------------------------------------------------

struct OutputInfo {
    output: WlOutput,
    name: String,
}

#[derive(Default)]
struct SessionState {
    size: Option<(u32, u32)>,
    formats: Vec<wl_shm::Format>,
    done: bool,
    stopped: bool,
}

#[derive(Default)]
struct FrameState {
    presented_ns: u64,
    ready: bool,
    failed: bool,
}

struct State {
    outputs: Vec<OutputInfo>,
    session: SessionState,
    frame: FrameState,
}

impl Dispatch<WlRegistry, GlobalListContents> for State {
    fn event(_: &mut Self, _: &WlRegistry, _: wayland_client::protocol::wl_registry::Event, _: &GlobalListContents, _: &Connection, _: &QueueHandle<Self>) {}
}

impl Dispatch<WlOutput, ()> for State {
    fn event(s: &mut Self, o: &WlOutput, event: wl_output::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        if let wl_output::Event::Name { name } = event
            && let Some(info) = s.outputs.iter_mut().find(|i| &i.output == o) {
            info.name = name;
        }
    }
}

delegate_noop!(State: ignore WlShm);
delegate_noop!(State: ignore WlShmPool);
delegate_noop!(State: ignore WlBuffer);
delegate_noop!(State: ignore Manager);
delegate_noop!(State: ignore SourceManager);
delegate_noop!(State: ignore Source);

impl Dispatch<Session, ()> for State {
    fn event(s: &mut Self, _: &Session, event: session::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            session::Event::BufferSize { width, height } => s.session.size = Some((width, height)),
            session::Event::ShmFormat { format: WEnum::Value(f) } => s.session.formats.push(f),
            session::Event::Done => s.session.done = true,
            session::Event::Stopped => s.session.stopped = true,
            _ => {}
        }
    }
}

impl Dispatch<Frame, ()> for State {
    fn event(s: &mut Self, f: &Frame, event: frame::Event, _: &(), _: &Connection, _: &QueueHandle<Self>) {
        match event {
            frame::Event::PresentationTime { tv_sec_hi, tv_sec_lo, tv_nsec } => {
                let secs = ((tv_sec_hi as u64) << 32) | tv_sec_lo as u64;
                s.frame.presented_ns = secs * 1_000_000_000 + tv_nsec as u64;
            }
            frame::Event::Ready => {
                s.frame.ready = true;
                f.destroy();
            }
            frame::Event::Failed { .. } => {
                s.frame.failed = true;
                f.destroy();
            }
            _ => {}
        }
    }
}

/// The shm buffer the compositor copies into, kept while the output keeps its size.
struct Shm {
    buffer: WlBuffer,
    map: *mut u8,
    len: usize,
    size: (u32, u32),
    format: wl_shm::Format,
}

impl Drop for Shm {
    fn drop(&mut self) {
        self.buffer.destroy();
        // Safety: `map` is the mapping made for this buffer, `len` long, and nothing else holds it.
        unsafe {
            let _ = rustix::mm::munmap(self.map.cast(), self.len);
        }
    }
}

fn run(shared: &Shared) -> Result<(), Box<dyn std::error::Error>> {
    let conn = Connection::connect_to_env()?;
    let (globals, mut queue) = registry_queue_init::<State>(&conn)?;
    let qh = queue.handle();
    let shm: WlShm = globals.bind(&qh, 1..=1, ())?;
    let manager: Manager = globals.bind(&qh, 1..=1, ()).map_err(|_| "the compositor has no ext-image-copy-capture")?;
    let sources: SourceManager = globals.bind(&qh, 1..=1, ()).map_err(|_| "the compositor has no ext-output-image-capture-source")?;
    let mut state = State { outputs: Vec::new(), session: SessionState::default(), frame: FrameState::default() };
    for g in globals.contents().clone_list() {
        if g.interface == "wl_output" {
            let output: WlOutput = globals.registry().bind(g.name, g.version.min(4), &qh, ());
            state.outputs.push(OutputInfo { output, name: String::new() });
        }
    }
    queue.roundtrip(&mut state)?;

    let mut current: Option<(String, Session)> = None;
    let mut shm_buf: Option<Shm> = None;
    let mut seq = 0u64;
    let min_gap = Duration::from_micros(1_000_000 / 144);
    let mut last = Instant::now() - min_gap;
    loop {
        // What is wanted; nothing captured while no region is.
        let (output_name, region) = {
            let mut w = shared.wanted.lock().unwrap_or_else(|p| p.into_inner());
            while w.region.is_none() && !w.stop {
                w = shared.wake.wait(w).unwrap_or_else(|p| p.into_inner());
            }
            if w.stop {
                return Ok(());
            }
            (w.output.clone().unwrap_or_default(), w.region.expect("checked"))
        };
        // A session on that output.
        if current.as_ref().is_none_or(|(n, _)| *n != output_name) || state.session.stopped {
            if let Some((_, old)) = current.take() {
                old.destroy();
            }
            let Some(out) = state.outputs.iter().find(|o| output_name.is_empty() || o.name == output_name).map(|o| o.output.clone()) else {
                return Err(format!("no output named {output_name:?}").into());
            };
            let source = sources.create_source(&out, &qh, ());
            state.session = SessionState::default();
            let sess = manager.create_session(&source, Options::empty(), &qh, ());
            source.destroy();
            while !state.session.done && !state.session.stopped {
                queue.blocking_dispatch(&mut state)?;
            }
            current = Some((output_name.clone(), sess));
            shm_buf = None;
        }
        let Some((width, height)) = state.session.size else { return Err("the compositor gave no buffer size".into()) };
        let format = [wl_shm::Format::Xrgb8888, wl_shm::Format::Argb8888, wl_shm::Format::Xbgr8888, wl_shm::Format::Abgr8888]
            .into_iter().find(|f| state.session.formats.contains(f)).ok_or("no 32-bit shm format offered")?;
        if shm_buf.as_ref().is_none_or(|b| b.size != (width, height) || b.format != format) {
            let stride = width * 4;
            let len = (stride * height) as usize;
            let fd = rustix::fs::memfd_create("rune-glass", rustix::fs::MemfdFlags::CLOEXEC)?;
            rustix::fs::ftruncate(&fd, len as u64)?;
            // Safety: a fresh shared mapping of the memfd just sized to `len`.
            let map = unsafe { rustix::mm::mmap(std::ptr::null_mut(), len, rustix::mm::ProtFlags::READ | rustix::mm::ProtFlags::WRITE, rustix::mm::MapFlags::SHARED, &fd, 0)? };
            let pool = shm.create_pool(fd.as_fd(), len as i32, &qh, ());
            let buffer = pool.create_buffer(0, width as i32, height as i32, stride as i32, format, &qh, ());
            pool.destroy();
            shm_buf = Some(Shm { buffer, map: map.cast(), len, size: (width, height), format });
        }
        let buf = shm_buf.as_ref().expect("made above");
        // Paced to the fastest displays: the compositor answers when the output has a new frame.
        if let Some(wait) = min_gap.checked_sub(last.elapsed()) {
            std::thread::sleep(wait);
        }
        last = Instant::now();
        let sess = &current.as_ref().expect("made above").1;
        state.frame = FrameState::default();
        let f = sess.create_frame(&qh, ());
        f.attach_buffer(&buf.buffer);
        f.damage_buffer(0, 0, width as i32, height as i32);
        f.capture();
        while !state.frame.ready && !state.frame.failed && !state.session.stopped {
            conn.flush()?;
            queue.blocking_dispatch(&mut state)?;
        }
        if !state.frame.ready {
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        // The region, clipped to the output, as RGBA.
        let x0 = region.x.max(0) as u32;
        let y0 = region.y.max(0) as u32;
        let x1 = ((region.x + region.w as i32).max(0) as u32).min(width);
        let y1 = ((region.y + region.h as i32).max(0) as u32).min(height);
        let mut rgba = vec![0u8; (region.w * region.h * 4) as usize];
        // Safety: the compositor has finished writing (ready), and the mapping is `len` long.
        let px = unsafe { std::slice::from_raw_parts(buf.map as *const u8, buf.len) };
        let (ri, bi) = match format {
            wl_shm::Format::Xbgr8888 | wl_shm::Format::Abgr8888 => (0, 2),
            _ => (2, 0),
        };
        for y in y0..y1 {
            let src = (y * width * 4) as usize;
            let dst = ((y as i32 - region.y) as u32 * region.w * 4) as usize;
            for x in x0..x1 {
                let s = src + (x * 4) as usize;
                let d = dst + ((x as i32 - region.x) as u32 * 4) as usize;
                rgba[d] = px[s + ri];
                rgba[d + 1] = px[s + 1];
                rgba[d + 2] = px[s + bi];
                rgba[d + 3] = 255;
            }
        }
        seq += 1;
        let presented_ns = if state.frame.presented_ns > 0 { state.frame.presented_ns } else { crate::now_ns() };
        *shared.latest.lock().unwrap_or_else(|p| p.into_inner()) = Some(Captured { seq, region, rgba: Arc::new(rgba), presented_ns, output: (width, height) });
        if let Some(f) = shared.on_frame.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            f();
        }
    }
}

// Safety: the raw mapping is only touched on the capture thread that owns it.
unsafe impl Send for Shm {}
