//! What is behind a window, recovered from a capture that has the window in it.

use std::collections::VecDeque;

use crate::{Captured, Region};

/// A frame the app showed: its pixels as it handed them to the compositor.
#[derive(Clone)]
pub struct Ours {
    /// When it was submitted, in [`crate::now_ns`]'s clock.
    pub submitted_ns: u64,
    /// Where its top-left pixel lands on the output, in buffer pixels.
    pub origin: (i32, i32),
    pub w: u32,
    pub h: u32,
    /// Premultiplied RGBA, 8-bit sRGB: exactly what the window's buffer held.
    pub rgba: Vec<u8>,
}

/// Turns captures of the output into what is behind the window.
pub struct SeeThrough {
    ours: VecDeque<Ours>,
    /// Frames submitted whose pixels haven't come back yet, by submission time.
    pending: Vec<u64>,
    behind: Vec<u8>,
    region: Region,
    /// Above this alpha a pixel of ours lets too little through to be read: what is behind it is
    /// taken from readable glass beside it, or the last value is held.
    pub hold: f32,
    /// How the last capture went, for diagnostics: the share of samples out of range with the
    /// best frame, and whether it was passed over.
    pub last: (f32, bool),
    /// The frame of ours the last capture was solved against (or would have been), by
    /// submission time.
    pub last_frame: u64,
    /// The last capture solved: one exactly like it, against the same frame, changes nothing.
    solved: Option<std::sync::Arc<Vec<u8>>>,
    /// Whether the last [`SeeThrough::recover`] changed the picture: when it didn't, there is no
    /// new frame to draw.
    pub fresh: bool,
}

impl Default for SeeThrough {
    fn default() -> Self {
        SeeThrough { ours: VecDeque::new(), pending: Vec::new(), behind: Vec::new(), region: Region::default(), hold: 0.975, last: (0.0, false), last_frame: 0, solved: None, fresh: false }
    }
}

/// How many of the app's frames are kept to match captures against.
const RING: usize = 8;
/// A frame of ours can't be on screen sooner than this after it was submitted.
const LATENCY_NS: u64 = 1_000_000;

impl SeeThrough {
    pub fn new() -> Self {
        Self::default()
    }

    /// A frame has been submitted; its pixels follow with `push_ours`. Until they do, a capture
    /// that could show it can't be solved: [`SeeThrough::ready`] says to wait.
    pub fn submitted(&mut self, submitted_ns: u64) {
        self.pending.push(submitted_ns);
        // A frame whose pixels never came (a lost readback) mustn't stall everything.
        let now = crate::now_ns();
        self.pending.retain(|t| now.saturating_sub(*t) < 250_000_000);
    }

    /// Whether a capture presented at `presented_ns` can be solved now: every frame that could
    /// be in it has its pixels here.
    pub fn ready(&self, presented_ns: u64) -> bool {
        !self.pending.iter().any(|t| t + LATENCY_NS <= presented_ns)
    }

    /// A frame the app has just handed to the compositor.
    pub fn push_ours(&mut self, ours: Ours) {
        self.pending.retain(|t| *t != ours.submitted_ns);
        if self.ours.len() == RING {
            self.ours.pop_front();
        }
        self.ours.push_back(ours);
    }

    /// What is behind, from a capture taken before the window covered it: where the window is
    /// opaque this is all there is to go on.
    pub fn seed(&mut self, region: Region, rgba: Vec<u8>) {
        if rgba.len() == (region.w * region.h * 4) as usize {
            self.region = region;
            self.behind = rgba;
        }
    }

    /// Forgets everything: the window was hidden.
    pub fn reset(&mut self) {
        self.ours.clear();
        self.pending.clear();
        self.behind.clear();
        self.solved = None;
    }

    /// The frame of ours that is in a capture presented at `presented_ns`: the newest one
    /// submitted before then.
    /// What is behind, as the last [`SeeThrough::recover`] left it.
    pub fn behind(&self) -> &[u8] {
        &self.behind
    }

    /// A frame of ours still kept, by submission time.
    pub fn frame(&self, submitted_ns: u64) -> Option<&Ours> {
        self.ours.iter().find(|o| o.submitted_ns == submitted_ns)
    }

    #[cfg(test)]
    fn ours_in(&self, presented_ns: u64) -> Option<&Ours> {
        self.ours.iter().rev().find(|o| o.submitted_ns + LATENCY_NS <= presented_ns).or_else(|| self.ours.front())
    }

    /// How implausible solving `cap` against `ours` is, on a sparse grid: the share of samples
    /// whose solution falls outside what a pixel can be (the right frame keeps every solution in
    /// range, give or take rounding; a wrong one throws many far out, its difference amplified by
    /// 255 / (255 - a)), then how far the rest moved from what was behind last.
    fn implausibility(&self, cap: &Captured, ours: &Ours) -> (f32, f32) {
        let r = cap.region;
        let hold = (self.hold * 255.0) as i32;
        let (mut samples, mut out, mut moved) = (0u32, 0u32, 0f32);
        let have_prev = self.behind.len() == (r.w * r.h * 4) as usize;
        for y in (0..r.h as i32).step_by(11) {
            let oy = y + r.y - ours.origin.1;
            if oy < 0 || oy as u32 >= ours.h {
                continue;
            }
            for x in (0..r.w as i32).step_by(11) {
                let ox = x + r.x - ours.origin.0;
                if ox < 0 || ox as u32 >= ours.w {
                    continue;
                }
                let j = ((oy as u32 * ours.w + ox as u32) * 4) as usize;
                let a = ours.rgba[j + 3] as i32;
                if a == 0 || a > hold {
                    continue;
                }
                let i = ((y * r.w as i32 + x) * 4) as usize;
                let through = 255 - a;
                let slack = 255 / through + 3;
                samples += 1;
                let mut bad = false;
                for k in 0..3 {
                    let v = (cap.rgba[i + k] as i32 - ours.rgba[j + k] as i32) * 255 / through;
                    if v < -slack * 2 || v > 255 + slack * 2 {
                        bad = true;
                    }
                    if have_prev {
                        moved += (v.clamp(0, 255) - self.behind[i + k] as i32).abs().min(64) as f32;
                    }
                }
                out += bad as u32;
            }
        }
        if samples == 0 {
            return (0.0, 0.0);
        }
        (out as f32 / samples as f32, moved / (samples * 3) as f32)
    }

    /// What is behind the window across the capture's region, RGBA. `shown = ours + (1 - a) *
    /// behind`, solved for `behind` wherever ours lets anything through. The frame of ours it is
    /// solved against is the one that makes the capture most plausible, among those that could
    /// be in it; a capture no frame explains is passed over, the last good picture kept.
    pub fn recover(&mut self, cap: &Captured) -> &[u8] {
        let r = cap.region;
        let n = (r.w * r.h * 4) as usize;
        self.fresh = true;
        if self.region != r || self.behind.len() != n {
            // Nothing to hold yet: start from the capture itself.
            self.region = r;
            self.behind = cap.rgba.as_ref().clone();
        }
        if self.ours.is_empty() {
            self.behind.copy_from_slice(&cap.rgba);
            return &self.behind;
        }
        // Every frame of ours that could be on screen in it, newest first; timing alone can't say
        // which, as a frame lands at the compositor's next refresh or the one after.
        let mut best: Option<(f32, f32, usize)> = None;
        for (idx, o) in self.ours.iter().enumerate().rev() {
            if o.submitted_ns > cap.presented_ns {
                continue;
            }
            let (out, moved) = self.implausibility(cap, o);
            if best.is_none_or(|(bo, bm, _)| out < bo - 0.002 || (out <= bo + 0.002 && moved < bm)) {
                best = Some((out, moved, idx));
            }
            // Explained outright (as the newest frame nearly always does): no need to look on.
            if out < 0.003 {
                break;
            }
        }
        let Some((out, _, idx)) = best else { return &self.behind };
        // Nothing explains it: a frame of ours we never saw. Better the last good picture than noise.
        self.last = (out, out > 0.04);
        let same = self.last_frame == self.ours[idx].submitted_ns;
        self.last_frame = self.ours[idx].submitted_ns;
        if out > 0.04 {
            self.fresh = false;
            return &self.behind;
        }
        // Nothing moved, on screen or in ours: the picture already is what this would make.
        if same && self.solved.as_ref().is_some_and(|s| **s == *cap.rgba) {
            self.fresh = false;
            return &self.behind;
        }
        self.solved = Some(cap.rgba.clone());
        let ours = &self.ours[idx];
        let shown: &[u8] = &cap.rgba;
        let hold = (self.hold * 255.0) as u32;
        // 255 / through in 16.16, so a solve is a multiply rather than a divide.
        let mut recip = [0i32; 256];
        for (t, r) in recip.iter_mut().enumerate().skip(1) {
            *r = ((255 << 16) + t as i32 / 2) / t as i32;
        }
        // Bands of rows on every core: each row stands alone.
        let row = (r.w * 4) as usize;
        let threads = std::thread::available_parallelism().map_or(4, |n| n.get()).min(16);
        let band = (r.h as usize).div_ceil(threads).max(1);
        std::thread::scope(|scope| {
            for (b, rows) in self.behind.chunks_mut(row * band).enumerate() {
                let recip = &recip;
                scope.spawn(move || {
                    for (dy, out) in rows.chunks_exact_mut(row).enumerate() {
                        let y = (b * band + dy) as i32;
                        solve_row(out, &shown[y as usize * row..(y as usize + 1) * row], ours, r, y, hold, recip);
                    }
                });
            }
        });
        &self.behind
    }
}

/// One row of [`SeeThrough::recover`]: `out` is what was behind along it, updated in place from
/// `shown`, the capture's row `y`.
fn solve_row(out: &mut [u8], shown: &[u8], ours: &Ours, r: Region, y: i32, hold: u32, recip: &[i32; 256]) {
    let oy = y + r.y - ours.origin.1;
    let row_in = oy >= 0 && (oy as u32) < ours.h;
    // The nearest pixel to the left that could be read, and where: what is under a stroke of
    // text or an icon is taken from just beside it, so it moves with the scene rather than
    // holding what was there when the window opened.
    let mut beside: Option<([u8; 3], i32)> = None;
    for x in 0..r.w as i32 {
        let i = (x * 4) as usize;
        let ox = x + r.x - ours.origin.0;
        if !row_in || ox < 0 || ox as u32 >= ours.w {
            out[i..i + 3].copy_from_slice(&shown[i..i + 3]);
            beside = Some(([shown[i], shown[i + 1], shown[i + 2]], x));
            continue;
        }
        let j = ((oy as u32 * ours.w + ox as u32) * 4) as usize;
        let a = ours.rgba[j + 3] as u32;
        if a == 0 {
            out[i..i + 3].copy_from_slice(&shown[i..i + 3]);
            beside = Some(([shown[i], shown[i + 1], shown[i + 2]], x));
        } else if shown[i + 3] == 0 {
            // Off the output: nothing was captured there.
        } else if a <= hold {
            // Solving amplifies the screen's 8-bit rounding by 255 / (255 - a): where nothing
            // really changed, the new value is eased in, so a still scene stays still; a real
            // change (past the rounding's reach) lands at once.
            let through = (255 - a) as i32;
            let k16 = recip[through as usize];
            let reach = (255 / through).max(1) + 2;
            for k in 0..3 {
                let v = (((shown[i + k] as i32 - ours.rgba[j + k] as i32) * k16 + (1 << 15)) >> 16).clamp(0, 255);
                let was = out[i + k] as i32;
                out[i + k] = if (v - was).abs() <= reach { (was * 3 + v + 2) / 4 } else { v } as u8;
            }
            beside = Some(([out[i], out[i + 1], out[i + 2]], x));
        } else if let Some((c, _)) = beside.filter(|(_, at)| x - at <= 48) {
            // Too opaque to read (text, an icon, a control over the glass), but readable glass
            // is close: what is under it is what is beside it. Solving it anyway would amplify
            // every error by fifty and more.
            out[i..i + 3].copy_from_slice(&c);
        }
        // Opaque and nothing readable near: what was behind it last is kept.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// What the compositor does: premultiplied source-over, rounded to 8 bits.
    fn blend(ours: [u8; 4], behind: [u8; 3]) -> [u8; 3] {
        let a = ours[3] as u32;
        let mut out = [0u8; 3];
        for k in 0..3 {
            out[k] = (ours[k] as u32 + (behind[k] as u32 * (255 - a) + 127) / 255).min(255) as u8;
        }
        out
    }

    #[test]
    fn what_is_behind_comes_back_from_a_capture_with_the_window_in_it() {
        let region = Region { x: 10, y: 20, w: 4, h: 1 };
        let behind = [[200u8, 30, 90], [10, 250, 120], [0, 0, 0], [255, 255, 255]];
        // Glass at 88%, a dark tint; then an opaque pixel of text; then the window's edge.
        let glass = |c: [u8; 3]| [(c[0] as u32 * 224 / 255) as u8, (c[1] as u32 * 224 / 255) as u8, (c[2] as u32 * 224 / 255) as u8, 224];
        let ours_px = [glass([26, 28, 34]), glass([240, 240, 240]), [255, 255, 255, 255], [0, 0, 0, 0]];
        let mut shown = Vec::new();
        let mut mine = Vec::new();
        for (o, b) in ours_px.iter().zip(behind) {
            shown.extend(blend(*o, b));
            shown.push(255);
            mine.extend(o);
        }
        let mut st = SeeThrough::new();
        st.seed(region, vec![9; 16]);
        st.push_ours(Ours { submitted_ns: 100, origin: (10, 20), w: 4, h: 1, rgba: mine });
        let cap = Captured { seq: 1, region, rgba: Arc::new(shown), presented_ns: 50_000_000, output: (100, 100) };
        let got = st.recover(&cap).to_vec();
        for (i, b) in behind.iter().enumerate() {
            let px = &got[i * 4..i * 4 + 3];
            if i == 2 {
                assert_eq!(px, &got[4..7], "under opaque text is what is beside it");
            } else {
                for k in 0..3 {
                    assert!((px[k] as i32 - b[k] as i32).abs() <= 5, "pixel {i}: {px:?} vs {b:?}");
                }
            }
        }
    }

    #[test]
    fn a_capture_is_matched_to_the_frame_that_was_on_screen() {
        let mut st = SeeThrough::new();
        for (t, v) in [(10_000_000u64, 1u8), (26_000_000, 2), (42_000_000, 3)] {
            st.push_ours(Ours { submitted_ns: t, origin: (0, 0), w: 1, h: 1, rgba: vec![v; 4] });
        }
        assert_eq!(st.ours_in(30_000_000).unwrap().rgba[0], 2);
        assert_eq!(st.ours_in(50_000_000).unwrap().rgba[0], 3);
        assert_eq!(st.ours_in(5_000_000).unwrap().rgba[0], 1);
    }
}
