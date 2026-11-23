//! See-through capture: the live screen behind a translucent window, with no help from the
//! compositor beyond the standard capture protocol.
//!
//! A Wayland client can't read what is behind its own surface. It can capture the whole output
//! (`ext-image-copy-capture-v1`), but that capture has the client's own window in it. This crate
//! takes it back out. The compositor blends a window over what is behind it as
//!
//! ```text
//! shown = ours + (1 - ours.alpha) * behind        (premultiplied, 8-bit sRGB)
//! ```
//!
//! and the client knows `ours` exactly: it drew it. Wherever its own alpha is below one,
//! `behind = (shown - ours) / (1 - ours.alpha)` gives the real pixels behind the window, live:
//! the wallpaper, a video, a workspace sliding past. Where it drew something opaque (text), the
//! last good value is held. Liquid Glass then bends what is really there.
//!
//! Two pieces, so any app and any compositor can use them:
//!
//! - [`OutputCapture`] keeps a region of one output captured, a new frame whenever the output
//!   shows something new, each stamped with when it was presented.
//! - [`SeeThrough`] matches each capture to the frame of the app's that is in it (by
//!   presentation time) and recovers what is behind.
//!
//! Where the window is on the output is the app's to say ([`Region`]): compositors don't share
//! window positions through a common protocol, so the app asks its own (Hyprland's IPC, Sway's…).

mod capture;
mod recover;

pub use capture::{Captured, OutputCapture};
pub use recover::{Ours, SeeThrough};

/// A rectangle in an output's buffer pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Region {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// Now, on the clock Wayland presentation times use (CLOCK_MONOTONIC), in nanoseconds.
pub fn now_ns() -> u64 {
    let t = rustix::time::clock_gettime(rustix::time::ClockId::Monotonic);
    t.tv_sec as u64 * 1_000_000_000 + t.tv_nsec as u64
}
