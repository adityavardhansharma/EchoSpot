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
    /// The see-through feed this window's frames go to (`seethrough`), when the backdrop is
    /// the live screen: the glass is drawn translucent and every frame is read back.
    pub through: Option<&'static str>,
}

/// A window's state that can stand on Liquid Glass. `None` draws the window as Rune would.
pub trait Liquid {
    fn pane(&self) -> Option<Pane<'_>>;
}

/// A frame copied out for reading back once the GPU has drawn it.
struct Readback {
    buffer: wgpu::Buffer,
    size: (u32, u32),
    bytes_per_row: u32,
    bgra: bool,
    submitted_ns: u64,
    feed: &'static str,
}

struct Gpu {
    device: DeviceKey,
    fx: FxRenderer,
    tex: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32))>,
    uploaded: u64,
    /// The frame drawn offscreen, to be both shown and read back.
    frame: Option<(wgpu::Texture, wgpu::TextureView, (u32, u32), wgpu::TextureFormat)>,
    blit: Option<(wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::TextureFormat)>,
    /// Buffers back from the reader, to copy the next frames into.
    free: std::sync::Arc<std::sync::Mutex<Vec<(wgpu::Buffer, u64)>>>,
    copied: Vec<Readback>,
    reader: Option<std::sync::mpsc::Sender<Readback>>,
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

/// Copies one texture to another, pixel for pixel.
const BLIT: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
struct V { @builtin(position) p: vec4f }
@vertex fn vs(@builtin(vertex_index) i: u32) -> V {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return V(vec4f(uv * 2.0 - 1.0, 0.0, 1.0));
}
@fragment fn fs(in: V) -> @location(0) vec4f {
    return textureLoad(src, vec2i(in.p.xy), 0);
}
"#;

fn blit_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> (wgpu::RenderPipeline, wgpu::BindGroupLayout, wgpu::TextureFormat) {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("echospot blit"), source: wgpu::ShaderSource::Wgsl(BLIT.into()) });
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("echospot blit"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0, visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture { sample_type: wgpu::TextureSampleType::Float { filterable: false }, view_dimension: wgpu::TextureViewDimension::D2, multisampled: false },
            count: None,
        }],
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some("echospot blit"), bind_group_layouts: &[Some(&layout)], immediate_size: 0 });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("echospot blit"),
        layout: Some(&pl),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), targets: &[Some(wgpu::ColorTargetState { format, blend: None, write_mask: wgpu::ColorWrites::ALL })], compilation_options: Default::default() }),
        primitive: Default::default(),
        depth_stencil: None,
        multisample: Default::default(),
        multiview_mask: None,
        cache: None,
    });
    (pipeline, layout, format)
}

/// Reads frames back as the GPU finishes them and hands them to their see-through feed. On its
/// own thread, so a frame's pixels arrive even when the window draws nothing after it.
fn spawn_reader(device: wgpu::Device, free: std::sync::Arc<std::sync::Mutex<Vec<(wgpu::Buffer, u64)>>>) -> std::sync::mpsc::Sender<Readback> {
    let (tx, rx) = std::sync::mpsc::channel::<Readback>();
    let _ = std::thread::Builder::new().name("echospot-glass-readback".into()).spawn(move || {
        while let Ok(r) = rx.recv() {
            let slice = r.buffer.slice(..);
            let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let flag = done.clone();
            slice.map_async(wgpu::MapMode::Read, move |res| flag.store(res.is_ok(), std::sync::atomic::Ordering::SeqCst));
            let _ = device.poll(wgpu::PollType::Wait { submission_index: None, timeout: Some(std::time::Duration::from_millis(500)) });
            if done.load(std::sync::atomic::Ordering::SeqCst) && let Ok(data) = slice.get_mapped_range() {
                let (w, h) = r.size;
                let mut rgba = vec![0u8; (w * h * 4) as usize];
                for y in 0..h as usize {
                    let src = &data[y * r.bytes_per_row as usize..y * r.bytes_per_row as usize + w as usize * 4];
                    let dst = &mut rgba[y * w as usize * 4..(y + 1) * w as usize * 4];
                    if r.bgra {
                        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                            d.copy_from_slice(&[s[2], s[1], s[0], s[3]]);
                        }
                    } else {
                        dst.copy_from_slice(src);
                    }
                }
                drop(data);
                crate::seethrough::push_ours(r.feed, r.submitted_ns, w, h, rgba);
            }
            r.buffer.unmap();
            let len = r.buffer.size();
            free.lock().unwrap_or_else(|p| p.into_inner()).push((r.buffer, len));
        }
    });
    tx
}

impl<S: Liquid + 'static> Compositor<S> for Glass<S> {
    fn draws_own(&self) -> bool {
        self.drew
    }

    fn submitted(&mut self, device: &wgpu::Device, _queue: &wgpu::Queue) {
        let Some(gpu) = self.gpu.as_mut() else { return };
        if gpu.copied.is_empty() {
            return;
        }
        let reader = gpu.reader.get_or_insert_with(|| spawn_reader(device.clone(), gpu.free.clone()));
        for r in gpu.copied.drain(..) {
            crate::seethrough::submitted(r.feed, r.submitted_ns);
            let _ = reader.send(r);
        }
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
            self.gpu = Some(Gpu {
                device: key, fx: FxRenderer::new(device, f.format, FxOptions { dither: true }), tex: None, uploaded: 0,
                frame: None, blit: None, free: Default::default(), copied: Vec::new(), reader: None,
            });
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

        // Drawn where it is shown, or, for see-through, offscreen first: what the window shows
        // has to be read back exactly.
        let through = pane.through;
        if through.is_some() && gpu.frame.as_ref().is_none_or(|fr| fr.2 != size || fr.3 != f.format) {
            let tex = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("echospot frame"),
                size: wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: f.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_SRC,
                view_formats: &[],
            });
            let v = tex.create_view(&Default::default());
            gpu.frame = Some((tex, v, size, f.format));
            if std::env::var_os("ECHOSPOT_DEBUG").is_some() {
                eprintln!("glass: see-through frame {size:?} {:?}", f.format);
            }
        }
        let target: &wgpu::TextureView = match (through, &gpu.frame) {
            (Some(_), Some((_, v, _, _))) => v,
            _ => f.target,
        };
        let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("echospot glass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: target,
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
        f.ui.draw_target_range_with(f.encoder, target, f.format, wgpu::LoadOp::Load, 0..n, &mut |_, _| {});

        let Some(feed) = through else { return };
        let Some((frame_tex, frame_view, _, _)) = &gpu.frame else { return };
        // Shown: the frame copied to the window, pixel for pixel.
        if gpu.blit.as_ref().is_none_or(|b| b.2 != f.format) {
            gpu.blit = Some(blit_pipeline(device, f.format));
        }
        let (pipeline, layout, _) = gpu.blit.as_ref().expect("made above");
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("echospot blit"), layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(frame_view) }],
        });
        let mut pass = f.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("echospot blit"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: f.target, resolve_target: None, depth_slice: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        // Read back: the frame, as the compositor gets it, for the next capture to be solved against.
        let bytes_per_row = (size.0 * 4).div_ceil(256) * 256;
        let len = (bytes_per_row * size.1) as u64;
        let buffer = {
            let mut free = gpu.free.lock().unwrap_or_else(|p| p.into_inner());
            free.retain(|(_, l)| *l == len);
            free.pop().map(|(b, _)| b)
        }.unwrap_or_else(|| device.create_buffer(&wgpu::BufferDescriptor { label: Some("echospot readback"), size: len, usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ, mapped_at_creation: false }));
        f.encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo { texture: frame_tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            wgpu::TexelCopyBufferInfo { buffer: &buffer, layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(bytes_per_row), rows_per_image: Some(size.1) } },
            wgpu::Extent3d { width: size.0, height: size.1, depth_or_array_layers: 1 },
        );
        let bgra = matches!(f.format, wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb);
        gpu.copied.push(Readback { buffer, size, bytes_per_row, bgra, submitted_ns: rune_glass::now_ns(), feed });
    }
}
