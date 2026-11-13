//! Liquid Glass, as EchoDock draws it. The compositor first draws a rune-fx lens in the shape of
//! the panel, refracting the screen captured behind it, then Rune's UI over the lens. The numbers
//! are EchoDock's own, so a palette summoned above the dock is made of the same glass.

use std::marker::PhantomData;

use rune::compose::{Compositor, DeviceKey, Frame};
use rune_fx::*;

use crate::backdrop::BLEED;
use crate::theme::{radius, size};

/// The glass itself, under EchoDock's own names (`[glass]` in its config). Everything is
/// EchoDock's default but the frost: a dock holds icons, a palette holds text, which needs what
/// is behind it softened to stay readable.
#[derive(serde::Serialize, serde::Deserialize, Clone, Copy)]
#[serde(default)]
pub struct Level {
    /// Blur of what is behind, 0 (clear) to 1 (frosted).
    pub frost: f32,
    pub bevel: f32,
    pub depth: f32,
    pub dispersion: f32,
    /// Tint over the glass, 0 to 1.
    pub tint: f32,
    pub rim: f32,
    pub shadow: f32,
}

impl Default for Level {
    fn default() -> Self {
        Level { frost: 0.5, bevel: 22.0, depth: 34.0, dispersion: 0.12, tint: 0.0, rim: 1.0, shadow: 0.5 }
    }
}

const MAGNIFICATION: f32 = 1.1;

/// The screen behind a panel, captured with `backdrop::BLEED` around it.
#[derive(Default)]
pub struct Shot {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    generation: u64,
}

impl Shot {
    pub fn set(&mut self, width: u32, height: u32, rgba: Vec<u8>) {
        if rgba.len() < (width * height * 4) as usize {
            return self.clear();
        }
        self.width = width;
        self.height = height;
        self.rgba = rgba;
        self.generation += 1;
    }

    pub fn clear(&mut self) {
        self.rgba = Vec::new();
    }

    pub fn ready(&self) -> bool {
        !self.rgba.is_empty()
    }
}

/// One pane of glass: a window's panel and what is behind it.
pub struct Pane<'a> {
    /// The window's size in points; its panel sits `WINDOW_MARGIN` in from each edge.
    pub window: (f32, f32),
    pub dark: bool,
    pub level: Level,
    pub shot: &'a Shot,
}

/// A window's state that can stand on Liquid Glass. `None` draws the window as Rune would.
pub trait Liquid {
    fn pane(&self) -> Option<Pane<'_>>;
}

struct Gpu {
    device: DeviceKey,
    fx: FxRenderer,
    tex: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    uploaded: u64,
}

pub struct Glass<S> {
    gpu: Option<Gpu>,
    drew: bool,
    state: PhantomData<fn(&S)>,
}

impl<S> Default for Glass<S> {
    fn default() -> Self {
        Glass { gpu: None, drew: false, state: PhantomData }
    }
}

impl<S: Liquid + 'static> Compositor<S> for Glass<S> {
    fn draws_own(&self) -> bool {
        self.drew
    }

    fn render(&mut self, f: &mut Frame<'_, S>) {
        let (device, queue, size) = (f.device, f.queue, f.size);
        let Some(pane) = f.rt.state.pane().filter(|p| p.shot.ready()) else {
            self.drew = false;
            self.gpu = None;
            return f.draw_ui();
        };
        self.drew = true;
        let key = f.device_key();
        if self.gpu.as_ref().is_none_or(|g| g.device != key) {
            self.gpu = Some(Gpu { device: key, fx: FxRenderer::new(device, f.format, FxOptions { dither: true }), tex: None, uploaded: 0 });
        }
        let gpu = self.gpu.as_mut().unwrap();
        let k = size.0 as f32 / pane.window.0.max(1.0);

        // The backdrop texture is the size of the surface, with the capture where it was taken.
        if gpu.tex.as_ref().is_none_or(|t| t.2 != size) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("echospot backdrop"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            let view = tex.create_view(&Default::default());
            gpu.tex = Some((tex, view, size));
            gpu.uploaded = 0;
        }
        let shot = pane.shot;
        let Some((tex, view, _)) = &gpu.tex else { return };
        if shot.generation != gpu.uploaded {
            // The capture reaches `BLEED` past the panel and the window `WINDOW_MARGIN`: the
            // difference is trimmed from each side.
            let trim = ((BLEED - size::WINDOW_MARGIN) * k).round().max(0.0) as u32;
            let w = shot.width.saturating_sub(trim).min(size.0);
            let h = shot.height.saturating_sub(trim).min(size.1);
            if w > 0 && h > 0 {
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    &shot.rgba[((trim * shot.width + trim) * 4) as usize..],
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(shot.width * 4), rows_per_image: Some(shot.height) },
                    wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                );
            }
            gpu.uploaded = shot.generation;
        }

        let m = size::WINDOW_MARGIN;
        let (w, h) = (pane.window.0 - m * 2.0, pane.window.1 - m * 2.0);
        let g = pane.level;
        let tint = if pane.dark { rgb(0x1a1c22) } else { rgb(0xffffff) };
        let layers = [Fx::lens(LensParams {
            frost: g.frost.clamp(0.0, 1.0),
            // The bevel can't be deeper than the glass is thick.
            bevel: g.bevel.min(h * 0.45),
            depth: g.depth,
            dispersion: g.dispersion,
            magnification: MAGNIFICATION,
            rim: 0.2 + g.rim.clamp(0.0, 1.0) * 0.9,
            shadow: 0.25 + 0.4 * g.shadow.clamp(0.0, 1.0),
            tint,
            tint_amount: g.tint.clamp(0.0, 1.0),
            ..Default::default()
        }).on(Shape::rounded(m, m, w, h, radius::PANEL)).at(f.rt.t as f32)];
        let fx = &mut gpu.fx;
        fx.set_srgb_blend(f.srgb_blend());
        let slots = [FxSlot::default().scale(k).backdrop_srgb(false)];
        fx.prepare_with_slots(device, queue, &FxFrame { viewport: [size.0, size.1], scale: k, layers: &layers, backdrop: None, sources: &[] }, &slots);
        fx.blur_backdrop(device, queue, f.encoder, view, 0);
        let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("echospot glass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: f.target,
                resolve_target: None,
                depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        fx.draw_layer(&mut pass, 0);
        drop(pass);
        let n = f.rt.instances.len() as u32;
        f.ui.prepare(device, queue, &mut f.rt.text.atlas, &f.rt.instances, size);
        f.ui.encode_layers(f.encoder);
        f.ui.draw_target_range_with(f.encoder, f.target, f.format, wgpu::LoadOp::Load, 0..n, &mut |_, _| {});
    }
}
