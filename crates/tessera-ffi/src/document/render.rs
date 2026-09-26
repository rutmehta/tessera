//! The document session's render thread, GPU presentation (resident
//! compositor → straight-alpha RGBA8 IOSurface), the CPU fallback and
//! thumbnails.

use super::{DocFrameInfo, DocRect, Shared, State, find, layer_revision, raster_from_rgba};
use crate::{Result, failure, surface::Surface};
use compositor::{
    BlendMode, Compositor, DocState, Document, Fill, Knockout, Layer, LayerKind, Rect,
    gpu::GpuCompositor, resident::ResidentRenderer,
};
use engine_api::{
    EngineError, EngineResult,
    jobs::CancellationToken,
    tile::{Extent, TILE_SIZE, TileCoord},
};
use std::{
    collections::{BTreeSet, HashMap},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::Instant,
};
use wgpu::util::DeviceExt;

/// Levels a viewport may show (the compositor's `MAX_LEVEL`).
pub(crate) const MAX_VIEW_LEVEL: u8 = compositor::render::MAX_LEVEL;

/// The region the host asked for (`set_viewport`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Viewport {
    pub level: u8,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub zoom: f64,
}

/// Presentation state kept with the document state.
pub(crate) struct View {
    pub surfaces: Vec<Arc<Surface>>,
    /// Ring position of the next frame.
    pub next: usize,
    pub viewport: Option<Viewport>,
    pub display_headroom: f32,
}

impl Default for View {
    fn default() -> Self {
        Self {
            surfaces: Vec::new(),
            next: 0,
            viewport: None,
            display_headroom: 1.0,
        }
    }
}

/// `(level, region in level coordinates, zoom)` to present into a surface of
/// `sw × sh`: the requested viewport clipped to the level and the surface,
/// or the whole canvas at the finest level that fits.
fn resolve(vp: Option<Viewport>, canvas: Extent, sw: u32, sh: u32) -> (u8, Rect, f64) {
    match vp {
        Some(v) => {
            let le = canvas.at_level(v.level);
            let r = Rect::new(
                i64::from(v.x),
                i64::from(v.y),
                i64::from(v.x) + i64::from(v.width.min(sw)),
                i64::from(v.y) + i64::from(v.height.min(sh)),
            )
            .intersect(&Rect::of_extent(le));
            (v.level, r, v.zoom)
        }
        None => {
            let level = (0..MAX_VIEW_LEVEL)
                .find(|&l| {
                    let e = canvas.at_level(l);
                    e.width <= sw && e.height <= sh
                })
                .unwrap_or(MAX_VIEW_LEVEL - 1);
            let le = canvas.at_level(level);
            let r = Rect::of_extent(le).intersect(&Rect::new(0, 0, i64::from(sw), i64::from(sh)));
            let zoom = f64::from(le.width) / f64::from(canvas.width.max(1));
            (level, r, zoom)
        }
    }
}

// ─────────────────────────────── GPU ───────────────────────────────

/// Unpremultiplies the resident presentation (premultiplied RGBA8) into the
/// host surface, which takes straight alpha.
const UNPREMULTIPLY: &str = r#"
struct Size { w: u32, h: u32, _a: u32, _b: u32 }
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var dst: texture_storage_2d<rgba8unorm, write>;
@group(0) @binding(2) var<uniform> size: Size;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) g: vec3<u32>) {
    if (g.x >= size.w || g.y >= size.h) { return; }
    let c = textureLoad(src, vec2<i32>(g.xy), 0);
    var o = vec4<f32>(0.0);
    if (c.a > 0.0) {
        o = vec4<f32>(min(c.rgb / c.a, vec3<f32>(1.0)), c.a);
    }
    textureStore(dst, vec2<i32>(g.xy), o);
}
"#;

/// The compositor's GPU pipelines on the engine's shared device, shared by
/// every document session.
pub(crate) struct DocGpu {
    comp: GpuCompositor,
    unpremultiply: wgpu::ComputePipeline,
    name: String,
}

impl DocGpu {
    pub(crate) fn new(device: &gpu_core::GpuDevice) -> EngineResult<Self> {
        let comp = GpuCompositor::from_shared(device)?;
        let (d, _) = comp.handles();
        let module = d.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("document unpremultiply"),
            source: wgpu::ShaderSource::Wgsl(UNPREMULTIPLY.into()),
        });
        let unpremultiply = d.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("document unpremultiply"),
            layout: None,
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Ok(Self {
            comp,
            unpremultiply,
            name: format!("Metal ({})", device.adapter_info.name),
        })
    }
}

struct GpuBackend {
    gpu: Arc<DocGpu>,
    resident: ResidentRenderer,
    /// Intermediate premultiplied target, sized like the surfaces.
    scratch: Option<wgpu::Texture>,
    /// IOSurface textures by surface id (imported once per attached surface).
    targets: HashMap<u32, wgpu::Texture>,
}

impl GpuBackend {
    /// Presents `src` of a rendered level top-left into `surface`, straight
    /// alpha, and submits.
    fn present(&mut self, level: u8, src: Rect, surface: &Surface) -> EngineResult<()> {
        let (device, queue) = self.gpu.comp.handles();
        let (device, queue) = (device.clone(), queue.clone());
        let (sw, sh) = (surface.width(), surface.height());
        if self
            .scratch
            .as_ref()
            .is_none_or(|t| t.width() != sw || t.height() != sh)
        {
            self.scratch = Some(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("document present"),
                size: wgpu::Extent3d {
                    width: sw,
                    height: sh,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8Unorm,
                usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            }));
        }
        let scratch = self.scratch.as_ref().expect("scratch target");
        self.resident.present(level, scratch, src, (0, 0), None)?;
        let target = match self.targets.entry(surface.id()) {
            std::collections::hash_map::Entry::Occupied(e) => e.into_mut(),
            std::collections::hash_map::Entry::Vacant(e) => {
                let (texture, format) = gpu_core::write_to_iosurface(&device, surface.id())?;
                if format != gpu_core::SurfaceFormat::Rgba8 {
                    return Err(EngineError::Unsupported {
                        what: "document frames are RGBA8 surfaces".into(),
                    });
                }
                e.insert(texture)
            }
        };
        let (w, h) = (src.width() as u32, src.height() as u32);
        let size = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("document unpremultiply size"),
            contents: bytemuck::cast_slice(&[w, h, 0u32, 0u32]),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let src_view = scratch.create_view(&Default::default());
        let dst_view = target.create_view(&Default::default());
        let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("document unpremultiply"),
            layout: &self.gpu.unpremultiply.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&dst_view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: size.as_entire_binding(),
                },
            ],
        });
        let mut enc = device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.gpu.unpremultiply);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }
        queue.submit([enc.finish()]);
        Ok(())
    }
}

enum Backend {
    Gpu(Box<GpuBackend>),
    Cpu(Box<Compositor>),
    Stopped,
}

// ─────────────────────────────── renderer ───────────────────────────────

#[derive(Default)]
struct Signal {
    frame: bool,
    layers: BTreeSet<u64>,
    history: bool,
    /// First change since the last frame.
    since: Option<Instant>,
    busy: bool,
    stop: bool,
}

impl Signal {
    fn pending(&self) -> bool {
        self.frame || self.history || !self.layers.is_empty()
    }
}

/// What a thumbnail shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ThumbKind {
    Layer(u64),
    Mask(u64),
    Composite,
}

/// `(kind, max_px)` → `(revision, surface)`.
type ThumbCache = HashMap<(ThumbKind, u32), (u64, Arc<Surface>)>;

pub(crate) struct Renderer {
    name: String,
    backend: Mutex<Backend>,
    signal: Mutex<Signal>,
    cv: Condvar,
    thumbs: Mutex<ThumbCache>,
    thumb_renders: AtomicU64,
}

impl Renderer {
    pub(crate) fn new(gpu: Option<Arc<DocGpu>>) -> Self {
        let (name, backend) = match gpu.map(|g| ResidentRenderer::new(&g.comp).map(|r| (g, r))) {
            Some(Ok((gpu, resident))) => (
                gpu.name.clone(),
                Backend::Gpu(Box::new(GpuBackend {
                    gpu,
                    resident,
                    scratch: None,
                    targets: HashMap::new(),
                })),
            ),
            other => {
                if let Some(Err(e)) = other {
                    eprintln!("document: resident renderer unavailable, using CPU: {e}");
                }
                (
                    "CPU".to_owned(),
                    Backend::Cpu(Box::new(Compositor::new(256 << 20))),
                )
            }
        };
        Self {
            name,
            backend: Mutex::new(backend),
            signal: Mutex::new(Signal::default()),
            cv: Condvar::new(),
            thumbs: Mutex::new(HashMap::new()),
            thumb_renders: AtomicU64::new(0),
        }
    }

    pub(crate) fn backend_name(&self) -> String {
        self.name.clone()
    }

    fn signal(&self) -> std::sync::MutexGuard<'_, Signal> {
        self.signal.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wakes the render thread: a frame, changed rows and/or history.
    pub(crate) fn request(&self, layers: Vec<u64>, history: bool, _epoch: u64) {
        let mut s = self.signal();
        s.frame = true;
        s.layers.extend(layers);
        s.history |= history;
        s.since.get_or_insert_with(Instant::now);
        self.cv.notify_all();
    }

    /// Changed rows without a new frame.
    pub(crate) fn notify_layers(&self, layers: Vec<u64>, _epoch: u64) {
        let mut s = self.signal();
        s.layers.extend(layers);
        self.cv.notify_all();
    }

    pub(crate) fn stop(&self) {
        let mut s = self.signal();
        s.stop = true;
        self.cv.notify_all();
    }

    pub(crate) fn wait_idle(&self) {
        let mut s = self.signal();
        while !s.stop && (s.busy || s.pending()) {
            s = self.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    pub(crate) fn thumbnail_renders(&self) -> u64 {
        self.thumb_renders.load(Ordering::SeqCst)
    }

    /// Renders `level` of `doc` and reads it back (straight RGBA f32).
    pub(crate) fn read_level(&self, doc: &Document, level: u8) -> Result<(u32, u32, Vec<f32>)> {
        let mut backend = self.backend.lock().map_err(failure)?;
        let (e, v) = match &mut *backend {
            Backend::Gpu(g) => {
                g.resident.render(doc, level)?;
                g.resident.read_level(level, false)?
            }
            Backend::Cpu(c) => c.render_level_rgba(doc, level)?,
            Backend::Stopped => return Err(failure("document is closed")),
        };
        Ok((e.width, e.height, v))
    }
}

pub(crate) fn worker_loop(shared: Arc<Shared>) {
    let r = &shared.render;
    loop {
        let (frame, layers, history, since) = {
            let mut s = r.signal();
            while !s.stop && !s.pending() {
                s = r.cv.wait(s).unwrap_or_else(|e| e.into_inner());
            }
            if s.stop {
                break;
            }
            s.busy = true;
            let since = if s.frame { s.since.take() } else { None };
            (
                std::mem::take(&mut s.frame),
                std::mem::take(&mut s.layers),
                std::mem::take(&mut s.history),
                since,
            )
        };
        let result = if frame {
            present_frame(&shared, since.unwrap_or_else(Instant::now))
        } else {
            Ok(None)
        };
        if let Some(listener) = shared.listener() {
            match result {
                Ok(Some(info)) => listener.on_frame(info),
                Ok(None) => {}
                Err(e) => listener.on_render_failed(e.to_string()),
            }
            if !layers.is_empty() {
                listener.on_layers_changed(layers.into_iter().collect());
            }
            if history && let Ok(st) = shared.lock() {
                let head = st.doc.history().current();
                drop(st);
                listener.on_history_changed(head);
            }
        }
        let mut s = r.signal();
        s.busy = false;
        r.cv.notify_all();
    }
    // Free GPU memory as soon as the session stops.
    *r.backend.lock().unwrap_or_else(|e| e.into_inner()) = Backend::Stopped;
    r.thumbs.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let mut s = r.signal();
    s.busy = false;
    r.cv.notify_all();
}

/// Renders and presents one frame of the live state into the next surface
/// of the ring. `None` without a surface.
fn present_frame(shared: &Arc<Shared>, since: Instant) -> Result<Option<DocFrameInfo>> {
    let r = &shared.render;
    let st = shared.lock()?;
    if st.closed || st.view.surfaces.is_empty() {
        return Ok(None);
    }
    let index = st.view.next % st.view.surfaces.len();
    let surface = st.view.surfaces[index].clone();
    let attached: Vec<u32> = st.view.surfaces.iter().map(|s| s.id()).collect();
    let canvas = st.live().state().canvas;
    let (level, src, zoom) = resolve(st.view.viewport, canvas, surface.width(), surface.height());
    let epoch = st.epoch;
    let le = canvas.at_level(level);
    let mut report = compositor::resident::FrameReport::default();
    if !src.is_empty() {
        // Smart filters baked and a filter preview shown (WP M5-12).
        let overlay = super::filtering::presented(shared, st.live(), level, src);
        let doc: &Document = overlay.as_deref().unwrap_or(st.live());
        let mut backend = r.backend.lock().map_err(failure)?;
        match &mut *backend {
            Backend::Gpu(g) => {
                g.targets.retain(|id, _| attached.contains(id));
                report = g.resident.render(doc, level)?;
                drop(st);
                g.present(level, src, &surface)?;
                g.resident.wait()?;
            }
            Backend::Cpu(c) => {
                cpu_present(c, doc, level, src, &surface)?;
                report.full = true;
                drop(st);
            }
            Backend::Stopped => return Ok(None),
        }
    } else {
        drop(st);
    }
    if let Ok(mut st) = shared.lock()
        && st.view.surfaces.iter().any(|s| s.id() == surface.id())
    {
        st.view.next = index + 1;
    }
    let canvas_rect = src.to_level0(level).intersect(&Rect::of_extent(canvas));
    Ok(Some(DocFrameInfo {
        surface_id: surface.id(),
        level,
        x: src.x0.max(0) as u32,
        y: src.y0.max(0) as u32,
        width: src.width() as u32,
        height: src.height() as u32,
        canvas_rect: DocRect::of(canvas_rect).unwrap_or(DocRect {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
        }),
        level_width: le.width,
        level_height: le.height,
        zoom,
        epoch,
        render_ms: since.elapsed().as_secs_f64() * 1000.0,
        full_recomposite: report.full,
        blocks: report.blocks,
    }))
}

fn quantize(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

/// CPU fallback: the tiles covering `src`, straight RGBA8, into `surface`.
fn cpu_present(
    c: &Compositor,
    doc: &Document,
    level: u8,
    src: Rect,
    surface: &Surface,
) -> Result<()> {
    let ts = i64::from(TILE_SIZE);
    let mut tiles = Vec::new();
    for ty in (src.y0 / ts)..=((src.y1 - 1) / ts) {
        for tx in (src.x0 / ts)..=((src.x1 - 1) / ts) {
            tiles.push(c.render_tile(doc, TileCoord::new(level, tx as u32, ty as u32))?);
        }
    }
    surface
        .with_pixels(|px, stride| -> EngineResult<()> {
            for t in &tiles {
                let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
                let l = t.layout();
                let n = l.plane_len();
                let s = t.samples::<f32>()?;
                for y in 0..l.extent.height as i64 {
                    let gy = i64::from(oy) + y;
                    if gy < src.y0 || gy >= src.y1 {
                        continue;
                    }
                    for x in 0..l.extent.width as i64 {
                        let gx = i64::from(ox) + x;
                        if gx < src.x0 || gx >= src.x1 {
                            continue;
                        }
                        let i = y as usize * l.stride() + x as usize;
                        let o = (gy - src.y0) as usize * stride + (gx - src.x0) as usize * 4;
                        for ch in 0..4 {
                            px[o + ch] = quantize(s[ch * n + i]);
                        }
                    }
                }
            }
            Ok(())
        })
        .map_err(failure)??;
    Ok(())
}

// ─────────────────────────────── thumbnails ───────────────────────────────

/// A one-layer document showing `layer`'s own content (Normal, opaque,
/// unmasked, unclipped).
fn solo(state: &DocState, layer: &Layer) -> Document {
    let mut l = layer.clone();
    l.props.visible = true;
    l.props.opacity = 1.0;
    l.props.fill_opacity = 1.0;
    l.props.blend_mode = BlendMode::Normal;
    l.props.clipped = false;
    l.props.knockout = Knockout::None;
    l.props.background = false;
    l.props.blend_if = Default::default();
    l.mask = None;
    let mut s = DocState::new(state.canvas, state.depth);
    s.next_id = state.next_id;
    s.root = vec![Arc::new(l)];
    Document::new(s)
}

/// A document whose alpha is `layer`'s mask (white fill through the mask).
fn mask_doc(state: &DocState, layer: &Layer) -> Result<Document> {
    let mut mask = layer
        .mask
        .clone()
        .ok_or_else(|| failure(format!("layer {} has no mask", layer.id.0)))?;
    mask.enabled = true;
    mask.density = 1.0;
    let mut l = Layer::new("mask", LayerKind::Fill(Fill::Solid { color: [1.0; 3] }));
    l.id = layer.id;
    l.mask = Some(mask);
    let mut s = DocState::new(state.canvas, state.depth);
    s.next_id = state.next_id;
    s.root = vec![Arc::new(l)];
    Ok(Document::new(s))
}

pub(crate) fn thumbnail(shared: &Shared, kind: ThumbKind, max_px: u32) -> Result<u32> {
    if max_px == 0 || max_px > 4096 {
        return Err(failure("max_px must be 1…4096"));
    }
    let r = &shared.render;
    let (rev, doc) = {
        let st: std::sync::MutexGuard<'_, State> = shared.lock()?;
        st.open()?;
        let live = st.live();
        let s = live.state();
        match kind {
            ThumbKind::Layer(id) => {
                let l = find(s, id)?;
                (layer_revision(l), solo(s, l))
            }
            ThumbKind::Mask(id) => {
                let l = find(s, id)?;
                let m = l
                    .mask
                    .as_ref()
                    .ok_or_else(|| failure(format!("layer {id} has no mask")))?;
                (m.raster.max_rev().max(l.content_rev), mask_doc(s, l)?)
            }
            ThumbKind::Composite => (s.rev, live.clone()),
        }
    };
    let key = (kind, max_px);
    if let Some((cached, surface)) = r.thumbs.lock().map_err(failure)?.get(&key)
        && *cached == rev
    {
        return Ok(surface.id());
    }
    let canvas = doc.state().canvas;
    let level = (0..MAX_VIEW_LEVEL)
        .find(|&l| {
            let e = canvas.at_level(l);
            e.width.max(e.height) <= max_px
        })
        .unwrap_or(MAX_VIEW_LEVEL - 1);
    let tiles = Compositor::new(64 << 20).render_level(&doc, level, &CancellationToken::new())?;
    let e = canvas.at_level(level);
    let surface = Surface::create_rgba8(e.width, e.height).map_err(failure)?;
    let mask = matches!(kind, ThumbKind::Mask(_));
    surface
        .with_pixels(|px, stride| -> EngineResult<()> {
            for t in &tiles {
                let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
                let l = t.layout();
                let n = l.plane_len();
                let s = t.samples::<f32>()?;
                for y in 0..l.extent.height as usize {
                    for x in 0..l.extent.width as usize {
                        let i = y * l.stride() + x;
                        let o = (oy as usize + y) * stride + (ox as usize + x) * 4;
                        if mask {
                            let v = quantize(s[3 * n + i]);
                            px[o..o + 4].copy_from_slice(&[v, v, v, 255]);
                        } else {
                            for c in 0..4 {
                                px[o + c] = quantize(s[c * n + i]);
                            }
                        }
                    }
                }
            }
            Ok(())
        })
        .map_err(failure)??;
    r.thumb_renders.fetch_add(1, Ordering::SeqCst);
    let id = surface.id();
    r.thumbs
        .lock()
        .map_err(failure)?
        .insert(key, (rev, Arc::new(surface)));
    Ok(id)
}

/// Straight RGBA of `doc` at level 0 as a raster (merge down, flatten).
pub(crate) fn composite_raster(
    doc: &Document,
    over: Option<[f32; 3]>,
    skip_transparent: bool,
) -> Result<compositor::Raster> {
    let (e, mut rgba) = Compositor::new(128 << 20).render_level_rgba(doc, 0)?;
    if let Some(bg) = over {
        for p in rgba.as_chunks_mut::<4>().0 {
            let a = p[3];
            for c in 0..3 {
                p[c] = p[c] * a + bg[c] * (1.0 - a);
            }
            p[3] = 1.0;
        }
    }
    raster_from_rgba(e, doc.state().depth, &rgba, skip_transparent)
}
