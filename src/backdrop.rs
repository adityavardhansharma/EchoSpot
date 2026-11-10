//! The panel's surface. Tinycast's is the system's behind-window blur; Omarchy ships Hyprland with
//! blur off, so the palette brings its own: the patch of screen it is about to cover is captured
//! as it is summoned and drawn blurred under the scrim. Nothing global is changed, and the look
//! does not depend on how the compositor is configured.

use std::process::{Command, Stdio};

use serde::Deserialize;

use crate::theme::size;

/// Extra capture around the panel, so the blur has real pixels to pull in at its edges.
pub const BLEED: f32 = 48.0;

#[derive(Deserialize, Default)]
struct Monitor {
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    width: f32,
    #[serde(default)]
    height: f32,
    #[serde(default)]
    scale: f32,
    #[serde(default)]
    transform: i32,
    #[serde(default)]
    focused: bool,
}

/// The focused monitor in layout points: `(x, y, w, h)`.
pub fn monitor() -> Option<(f32, f32, f32, f32)> {
    let json = crate::util::output("hyprctl", &["monitors", "-j"])?;
    let monitors: Vec<Monitor> = serde_json::from_str(&json).ok()?;
    let m = monitors.iter().find(|m| m.focused).or(monitors.first())?;
    let scale = if m.scale > 0.0 { m.scale } else { 1.0 };
    let (mut w, mut h) = (m.width / scale, m.height / scale);
    if m.transform % 2 == 1 {
        std::mem::swap(&mut w, &mut h);
    }
    Some((m.x, m.y, w, h))
}

/// Where the panel's top-left lands, mirroring the window rule: centred, `top` of the way down.
pub fn panel_origin(top: f32) -> Option<(f32, f32)> {
    let (x, y, w, h) = monitor()?;
    Some((x + ((w - size::WINDOW_W) / 2.0).floor() + size::WINDOW_MARGIN, y + (h * top).floor()))
}

fn parse_ppm(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    // "P6\n<w> <h>\n<max>\n" then RGB triples.
    let mut pos = 0;
    let mut fields = Vec::new();
    while fields.len() < 4 {
        while bytes.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        let start = pos;
        while !bytes.get(pos)?.is_ascii_whitespace() {
            pos += 1;
        }
        fields.push(std::str::from_utf8(&bytes[start..pos]).ok()?);
    }
    pos += 1;
    if fields[0] != "P6" || fields[3] != "255" {
        return None;
    }
    let (w, h): (u32, u32) = (fields[1].parse().ok()?, fields[2].parse().ok()?);
    let rgb = bytes.get(pos..pos + (w * h * 3) as usize)?;
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for px in rgb.chunks_exact(3) {
        rgba.extend_from_slice(&[px[0], px[1], px[2], 255]);
    }
    Some((w, h, rgba))
}

/// Captures what the panel will cover, with its bleed. About 35 ms; `None` without `grim`.
pub fn capture(top: f32) -> Option<(u32, u32, Vec<u8>)> {
    let (px, py) = panel_origin(top)?;
    let geometry = format!("{},{} {}x{}", (px - BLEED).round(), (py - BLEED).round(),
        (size::PANEL_W + BLEED * 2.0).round(), (size::PANEL_H + BLEED * 2.0).round());
    let out = Command::new("grim").args(["-g", &geometry, "-t", "ppm", "-"])
        .stdin(Stdio::null()).stderr(Stdio::null()).output().ok()?;
    out.status.success().then(|| parse_ppm(&out.stdout)).flatten()
}
