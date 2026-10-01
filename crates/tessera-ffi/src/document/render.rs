//! The document session's render thread, GPU presentation (resident
//! compositor → straight-alpha RGBA8 IOSurface), the CPU fallback and
//! thumbnails.

use super::{DocFrameInfo, DocRect, Shared, find, layer_revision, raster_from_rgba};
use crate::{Result, failure, surface::Surface};
use compositor::{
    BlendMode, Compositor, DocState, Document, Fill, Knockout, Layer, LayerKind, Rect,
    gpu::GpuCompositor, resident::ResidentRenderer,
};
use engine_api::{
    EngineError, EngineResult,
    jobs::{CancellationToken, Job, JobContext, Priority, Scheduler},
    tile::{Extent, TILE_SIZE, Tile},
};
use std::{
    collections::{BTreeSet, HashMap, VecDeque},
    sync::{
        Arc, Condvar, Mutex, OnceLock,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
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
#[derive(Clone)]
pub(crate) struct View {
    pub surfaces: Vec<Arc<Surface>>,
    /// Ring position of the next frame.
    pub next: usize,
    pub viewport: Option<Viewport>,
    pub display_headroom: f32,
    /// B5-14: bumped when the surface ring changes; a frame rendered for an
    /// older ring is dropped instead of published.
    pub generation: u64,
}

impl Default for View {
    fn default() -> Self {
        Self {
            surfaces: Vec::new(),
            next: 0,
            viewport: None,
            display_headroom: 1.0,
            generation: 0,
        }
    }
}

// B5-14 begin: viewport-only composition, frame records, resource policy.

/// Level pixels composited around the visible region on the viewport path
/// (one 16² resident block, the compositor's granularity; COMPOSITOR.md
/// §12.2). Pointwise programs need no halo for correctness; spatial ones
/// (positive-radius Shadows/Highlights, HDR Toning) take the full level.
pub(crate) const VIEWPORT_HALO: u32 = 16;

/// How a frame was composited.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DocRenderPath {
    /// `ResidentRenderer::render_viewport`: the visible region plus
    /// [`VIEWPORT_HALO`].
    Viewport,
    /// `ResidentRenderer::render`: the whole level (spatial adjustments need
    /// a full-level halo, or viewport rendering is off).
    FullLevel,
    /// The CPU compositor (layer styles, or no Metal).
    Cpu,
}

/// One frame of the render thread (tests, benches and `TESSERA_DOC_RENDER_LOG`).
#[derive(Clone, Debug)]
pub struct DocRenderRecord {
    pub path: DocRenderPath,
    pub level: u8,
    /// Presented region, level coordinates.
    pub visible: Rect,
    /// Region handed to the compositor (visible + halo, clipped), level
    /// coordinates; the whole level on the full-level path.
    pub requested: Rect,
    /// Union of the resident blocks dispatched (empty: nothing changed).
    pub dispatched: Rect,
    pub blocks: u32,
    pub epoch: u64,
    /// The document changed while the frame was rendered (a newer frame is
    /// queued; this one was still published).
    pub superseded: bool,
    /// Dropped before publication: cancelled, superseded, or its surface ring
    /// was replaced or the session closed.
    pub dropped: bool,
    /// Spans (ms): waiting for the live-state lock, holding it (snapshot),
    /// filter preparation, composition (CPU side, or the whole CPU
    /// fallback), GPU presentation until completion, and the frame total.
    pub lock_wait_ms: f64,
    pub lock_held_ms: f64,
    pub prep_ms: f64,
    pub composite_ms: f64,
    pub gpu_ms: f64,
    pub total_ms: f64,
}

/// B5-15: stage-cache bytes kept after a filter interaction ends.
const FILTER_CACHE_KEEP: u64 = 512 << 20;

/// Smart objects with filters (the compositor never evaluates them).
fn has_smart_filters(state: &DocState) -> bool {
    fn go(v: &[Arc<Layer>]) -> bool {
        v.iter().any(|l| match &l.kind {
            LayerKind::SmartObject(so) => !so.filters.is_empty(),
            _ => l.children().is_some_and(go),
        })
    }
    go(&state.root)
}

/// Needs a full-level halo: the resident program runs spatial passes over
/// the whole level (COMPOSITOR.md §4.3), as `resident::render_region`
/// decides for positive-radius Shadows/Highlights and HDR Toning.
fn needs_full_halo(state: &DocState) -> bool {
    fn spatial(l: &Layer) -> bool {
        match &l.kind {
            LayerKind::Adjustment(compositor::Adjustment::ShadowsHighlights { settings }) => {
                settings.needs_neighbourhood()
            }
            LayerKind::Adjustment(compositor::Adjustment::HdrToning { settings }) => {
                settings.needs_neighbourhood()
            }
            _ => l.children().is_some_and(|c| c.iter().any(|l| spatial(l))),
        }
    }
    state.root.iter().any(|l| spatial(l))
}

/// What document work registers with the process-wide interactive pressure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PressureKind {
    Render = 0,
    Filters = 1,
}

static PRESSURE_ACTIVE: [AtomicUsize; 2] = [AtomicUsize::new(0), AtomicUsize::new(0)];

/// A process-wide pool whose Viewport-priority "hold" jobs stand for
/// document work in `jobs::pressure` (the only public way to register it;
/// NEEDS.md asks for a direct guard). Replaced after `RECYCLE` holds because
/// a scheduler keeps every completed job's status for its lifetime.
struct HoldPool {
    pool: Arc<jobs::ThreadPoolScheduler>,
    submitted: usize,
}

const RECYCLE: usize = 4096;

fn hold_pool() -> Arc<jobs::ThreadPoolScheduler> {
    static POOL: OnceLock<Mutex<HoldPool>> = OnceLock::new();
    let m = POOL.get_or_init(|| {
        Mutex::new(HoldPool {
            pool: Arc::new(jobs::ThreadPoolScheduler::new(4)),
            submitted: 0,
        })
    });
    let mut p = m.lock().unwrap_or_else(|e| e.into_inner());
    p.submitted += 1;
    if p.submitted > RECYCLE {
        // The old pool is dropped (joined) with its last hold.
        p.pool = Arc::new(jobs::ThreadPoolScheduler::new(4));
        p.submitted = 1;
    }
    p.pool.clone()
}

type Gate = Arc<(Mutex<bool>, Condvar)>;

struct Hold(Gate, PressureKind);

impl Job for Hold {
    fn label(&self) -> &str {
        match self.1 {
            PressureKind::Render => "document frame",
            PressureKind::Filters => "document filters",
        }
    }
    fn priority(&self) -> Priority {
        Priority::Viewport
    }
    fn run(self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        let (m, cv) = &*self.0;
        let mut done = m.lock().unwrap_or_else(|e| e.into_inner());
        while !*done {
            ctx.check_cancelled()?;
            done = cv
                .wait_timeout(done, Duration::from_millis(50))
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
        Ok(())
    }
}

/// Registers document work with `jobs::pressure` while alive: photo export
/// bands wait for it (`jobs::yield_to_interactive`, bounded by the export's
/// maximum yield so export keeps progressing). Separate GPU queues stay
/// separate; only scheduling order changes.
pub(crate) struct Pressure {
    gate: Gate,
    token: CancellationToken,
    kind: PressureKind,
    // Keeps the pool alive until the hold ends (recycling).
    _pool: Arc<jobs::ThreadPoolScheduler>,
}

impl Pressure {
    pub(crate) fn begin(kind: PressureKind) -> Self {
        let gate: Gate = Arc::new((Mutex::new(false), Condvar::new()));
        let pool = hold_pool();
        let handle = pool.submit(Box::new(Hold(gate.clone(), kind)), None);
        PRESSURE_ACTIVE[kind as usize].fetch_add(1, Ordering::Relaxed);
        Self {
            gate,
            token: handle.cancellation,
            kind,
            _pool: pool,
        }
    }
}

impl Drop for Pressure {
    fn drop(&mut self) {
        let (m, cv) = &*self.gate;
        *m.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_all();
        // A hold still queued ends without running.
        self.token.cancel();
        PRESSURE_ACTIVE[self.kind as usize].fetch_sub(1, Ordering::Relaxed);
    }
}
// B5-14 end

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
    // B5-07 begin: CPU fallback for documents the resident program refuses
    // (layer styles). Created on first use; `cpu_frames` logs switches.
    cpu: Option<Box<Compositor>>,
    cpu_frames: bool,
    // B5-07 end
}

// B5-07 begin
impl GpuBackend {
    /// Renders `level` on the GPU, or `None` when the resident program does
    /// not support the document (layer styles need the CPU compositor).
    /// B5-14: `visible` (level coordinates) renders only that region plus
    /// [`VIEWPORT_HALO`] (`render_viewport`); `None` renders the whole level.
    fn render_or_refuse(
        &mut self,
        doc: &Document,
        level: u8,
        visible: Option<Rect>,
    ) -> EngineResult<Option<compositor::resident::FrameReport>> {
        let rendered = match visible {
            Some(v) => self.resident.render_viewport(doc, level, v, VIEWPORT_HALO),
            None => self.resident.render(doc, level),
        };
        let cpu = match rendered {
            Ok(report) => Some(report),
            Err(EngineError::Unsupported { what }) => {
                if !self.cpu_frames {
                    eprintln!("document: frames composited on the CPU ({what})");
                }
                None
            }
            Err(e) => return Err(e),
        };
        if cpu.is_some() && self.cpu_frames {
            eprintln!("document: frames composited on the GPU again");
        }
        self.cpu_frames = cpu.is_none();
        Ok(cpu)
    }

    fn cpu(&mut self) -> &Compositor {
        self.cpu
            .get_or_insert_with(|| Box::new(super::fonts::compositor(256 << 20))) // B5-10b
    }
}
// B5-07 end

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
    active_frame: Option<Arc<CancellationToken>>,
}

impl Signal {
    /// B5-22 latest-wins: a draft never cancels the frame in flight. It only
    /// marks one coalesced frame pending; that frame snapshots the newest
    /// live state when the worker starts it, right after the current one.
    fn request_frame(&mut self) {
        self.frame = true;
        self.since.get_or_insert_with(Instant::now);
    }

    /// Requests queued before the worker captures document/view state are
    /// included in that snapshot. Consume only those, while the session state
    /// lock still excludes edits. Requests after snapshotting remain pending.
    fn snapshot_taken(&mut self, cancel: &CancellationToken) {
        if !cancel.is_cancelled()
            && self
                .active_frame
                .as_deref()
                .is_some_and(|active| std::ptr::eq(active, cancel))
        {
            self.frame = false;
            self.since = None;
        }
    }

    /// Real invalidation (surface ring replaced or released, session
    /// stopping): the frame in flight can no longer be published, so its
    /// work is abandoned. Ownership stays so completion is still rejected.
    fn cancel_frame(&mut self) {
        if let Some(cancel) = &self.active_frame {
            cancel.cancel();
        }
    }

    /// The worker starts a frame only after the previous one finished, so
    /// this cancels nothing in production; it keeps a stray owner from
    /// publishing if that ever changes.
    fn begin_frame(&mut self) -> Arc<CancellationToken> {
        if let Some(previous) = &self.active_frame {
            previous.cancel();
        }
        let cancel = Arc::new(CancellationToken::new());
        self.active_frame = Some(cancel.clone());
        cancel
    }

    /// Publication is accepted while holding Signal, before invoking callbacks.
    /// Callbacks run unlocked because hosts may synchronously request another frame.
    fn finish_frame(&mut self, cancel: &Arc<CancellationToken>) -> bool {
        let owns = self
            .active_frame
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, cancel));
        if owns {
            self.active_frame = None;
        }
        owns && !self.stop && !cancel.is_cancelled()
    }

    fn stop_frames(&mut self) {
        self.stop = true;
        self.cancel_frame();
    }

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
    #[cfg(test)]
    read_level_entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    signal: Mutex<Signal>,
    cv: Condvar,
    thumbs: Mutex<ThumbCache>,
    thumb_renders: AtomicU64,
    // B5-14 begin
    records: Mutex<VecDeque<DocRenderRecord>>,
    viewport_rendering: AtomicBool,
    /// Frames per path (Viewport, FullLevel, Cpu) and dropped frames.
    counts: [AtomicU64; 4],
    last_path: Mutex<Option<DocRenderPath>>,
    last_resources: Mutex<Option<Instant>>,
    /// Composite thumbnails of the committed document (cache kept).
    thumb_comp: OnceLock<Compositor>,
    // B5-14 end
    /// B5-15: drop the resident smart-filter stage cache at the next frame
    /// (set when a filter interaction ends; its per-tick results are dead).
    trim_filters: AtomicBool,
}

impl Renderer {
    pub(crate) fn new(gpu: Option<Arc<DocGpu>>) -> Self {
        let (name, backend) = match gpu.map(|g| ResidentRenderer::new(&g.comp).map(|r| (g, r))) {
            Some(Ok((gpu, mut resident))) => (
                // B5-10 begin: the shared font snapshot (Type tool layout = rendering).
                {
                    super::fonts::install(&mut resident); // B5-10b
                    super::filtering::install_resident(&mut resident); // B5-15 (P19)
                    gpu.name.clone()
                },
                // B5-10 end
                Backend::Gpu(Box::new(GpuBackend {
                    gpu,
                    resident,
                    scratch: None,
                    targets: HashMap::new(),
                    cpu: None,         // B5-07
                    cpu_frames: false, // B5-07
                })),
            ),
            other => {
                if let Some(Err(e)) = other {
                    eprintln!("document: resident renderer unavailable, using CPU: {e}");
                }
                (
                    "CPU".to_owned(),
                    Backend::Cpu(Box::new(super::fonts::compositor(256 << 20))), // B5-10: shared fonts
                )
            }
        };
        Self {
            name,
            backend: Mutex::new(backend),
            #[cfg(test)]
            read_level_entered: Mutex::new(None),
            signal: Mutex::new(Signal::default()),
            cv: Condvar::new(),
            thumbs: Mutex::new(HashMap::new()),
            thumb_renders: AtomicU64::new(0),
            // B5-14 begin
            records: Mutex::new(VecDeque::new()),
            viewport_rendering: AtomicBool::new(
                std::env::var_os("TESSERA_DOC_FULL_LEVEL").is_none(),
            ),
            counts: Default::default(),
            last_path: Mutex::new(None),
            last_resources: Mutex::new(None),
            thumb_comp: OnceLock::new(),
            // B5-14 end
            trim_filters: AtomicBool::new(false), // B5-15
        }
    }

    // B5-14 begin
    pub(crate) fn records(&self) -> Vec<DocRenderRecord> {
        self.records
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect()
    }

    pub(crate) fn set_viewport_rendering(&self, enabled: bool) {
        self.viewport_rendering.store(enabled, Ordering::SeqCst);
    }

    /// GPU pages/bytes, CPU cache, pressure registrations and frame counts.
    pub(crate) fn resources(&self) -> String {
        let backend = match self.backend.try_lock() {
            Ok(b) => match &*b {
                Backend::Gpu(g) => {
                    let st = g.resident.stats();
                    let cpu = g.cpu.as_ref().map(|c| c.stats());
                    format!(
                        "GPU {} live pages, {:.1} MiB resident; smart filters {} GPU stages, {} CPU fallbacks, {:.1} MiB stage cache; CPU fallback {}",
                        st.live_pages,
                        st.resident_bytes as f64 / (1 << 20) as f64,
                        g.resident.filter_evaluations(), // B5-15 (P19)
                        g.resident.filter_fallbacks(),
                        g.resident.filter_cache_bytes() as f64 / (1 << 20) as f64,
                        cpu.map_or("unused".into(), |c| format!(
                            "{} cache hits / {} misses",
                            c.cache_hits, c.cache_misses
                        ))
                    )
                }
                Backend::Cpu(c) => {
                    let c = c.stats();
                    format!(
                        "CPU {} cache hits / {} misses",
                        c.cache_hits, c.cache_misses
                    )
                }
                Backend::Stopped => "stopped".into(),
            },
            Err(_) => "backend busy".into(),
        };
        let n = |i: usize| self.counts[i].load(Ordering::Relaxed);
        format!(
            "{backend}; pressure render {} filters {} (interactive jobs {}); frames viewport {} full-level {} cpu {} dropped {}",
            PRESSURE_ACTIVE[0].load(Ordering::Relaxed),
            PRESSURE_ACTIVE[1].load(Ordering::Relaxed),
            jobs::interactive_pending(),
            n(0),
            n(1),
            n(2),
            n(3),
        )
    }

    fn record(&self, rec: DocRenderRecord) {
        let i = if rec.dropped { 3 } else { rec.path as usize };
        self.counts[i].fetch_add(1, Ordering::Relaxed);
        {
            let mut last = self.last_path.lock().unwrap_or_else(|e| e.into_inner());
            if *last != Some(rec.path) {
                eprintln!(
                    "document: frames on the {:?} path (L{}, requested {:?})",
                    rec.path, rec.level, rec.requested
                );
                *last = Some(rec.path);
            }
        }
        if log_frames() {
            eprintln!(
                "doc-render: {:?} L{} visible {}x{} requested {}x{} dispatched {}x{} blocks {} epoch {}{}{} lock wait {:.2} held {:.2} prep {:.2} composite {:.2} gpu {:.2} total {:.2} ms",
                rec.path,
                rec.level,
                rec.visible.width(),
                rec.visible.height(),
                rec.requested.width(),
                rec.requested.height(),
                rec.dispatched.width(),
                rec.dispatched.height(),
                rec.blocks,
                rec.epoch,
                if rec.superseded { " superseded" } else { "" },
                if rec.dropped { " DROPPED" } else { "" },
                rec.lock_wait_ms,
                rec.lock_held_ms,
                rec.prep_ms,
                rec.composite_ms,
                rec.gpu_ms,
                rec.total_ms,
            );
        }
        let mut r = self.records.lock().unwrap_or_else(|e| e.into_inner());
        if r.len() >= 512 {
            r.pop_front();
        }
        r.push_back(rec);
        drop(r);
        if log_resources() {
            let mut last = self
                .last_resources
                .lock()
                .unwrap_or_else(|e| e.into_inner());
            if last.is_none_or(|t| t.elapsed() >= Duration::from_secs(2)) {
                *last = Some(Instant::now());
                drop(last);
                eprintln!("document-resources: {}", self.resources());
            }
        }
    }
    // B5-14 end

    pub(crate) fn backend_name(&self) -> String {
        self.name.clone()
    }

    fn signal(&self) -> std::sync::MutexGuard<'_, Signal> {
        self.signal.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Wakes the render thread: a frame, changed rows and/or history.
    pub(crate) fn request(&self, layers: Vec<u64>, history: bool, _epoch: u64) {
        let mut s = self.signal();
        s.request_frame();
        s.layers.extend(layers);
        s.history |= history;
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
        s.stop_frames();
        self.cv.notify_all();
    }

    /// Cancels the frame in flight because its surface ring changed
    /// (`View::generation`); it would be dropped at publication anyway.
    /// Never waits for the backend. Schedules nothing: callers that still
    /// have surfaces request the replacement frame.
    pub(crate) fn invalidate_frame(&self) {
        self.signal().cancel_frame();
    }

    pub(crate) fn wait_idle(&self) {
        let mut s = self.signal();
        while !s.stop && (s.busy || s.pending()) {
            s = self.cv.wait(s).unwrap_or_else(|e| e.into_inner());
        }
    }

    /// B5-14: the whole of `level` of `doc` rendered by the resident renderer
    /// and read back (straight RGBA f32); `None` without Metal or for
    /// documents it refuses (layer styles).
    fn resident_level(&self, doc: &Document, level: u8) -> Result<Option<(Extent, Vec<f32>)>> {
        fn styled(l: &Layer) -> bool {
            !l.props.styles.effects.is_empty()
                || l.children().is_some_and(|c| c.iter().any(|l| styled(l)))
        }
        if doc.state().root.iter().any(|l| styled(l)) {
            return Ok(None);
        }
        let mut backend = self.backend.lock().map_err(failure)?;
        let Backend::Gpu(g) = &mut *backend else {
            return Ok(None);
        };
        match g.resident.render(doc, level) {
            Ok(_) => Ok(Some(g.resident.read_level(level, false)?)),
            Err(EngineError::Unsupported { .. }) => Ok(None),
            Err(e) => Err(e.into()),
        }
    }

    /// B5-15 (P19): the resident renderer's smart-filter trace: GPU stages
    /// executed, layer-local CPU fallbacks and stage-cache bytes (`None`
    /// without Metal).
    pub(crate) fn smart_filter_stats(&self) -> Option<(u64, u64, u64)> {
        match &*self.backend.lock().ok()? {
            Backend::Gpu(g) => Some((
                g.resident.filter_evaluations(),
                g.resident.filter_fallbacks(),
                g.resident.filter_cache_bytes(),
            )),
            _ => None,
        }
    }

    /// B5-15: asks the render thread to release the resident smart-filter
    /// stage cache before its next frame when it holds more than
    /// [`FILTER_CACHE_KEEP`] (a filter drag leaves one full-resolution result
    /// per tick, up to the renderer's 2 GiB budget).
    pub(crate) fn trim_smart_filter_cache(&self) {
        self.trim_filters.store(true, Ordering::Relaxed);
    }

    pub(crate) fn thumbnail_mip_stats(&self) -> (u64, u64) {
        let gpu = match &*self.backend.lock().expect("render backend") {
            Backend::Gpu(g) => g.resident.mip_cache_probe(),
            _ => (0, 0),
        };
        let cpu = self
            .thumb_comp
            .get_or_init(|| super::fonts::compositor(128 << 20))
            .mip_cache_probe();
        (gpu.0 + cpu.0, gpu.1 + cpu.1)
    }

    pub(crate) fn thumbnail_renders(&self) -> u64 {
        self.thumb_renders.load(Ordering::SeqCst)
    }

    /// Renders `level` of `doc` and reads it back (straight RGBA f32).
    pub(crate) fn read_level(&self, doc: &Document, level: u8) -> Result<(u32, u32, Vec<f32>)> {
        #[cfg(test)]
        if let Some(entered) = self.read_level_entered.lock().unwrap().take() {
            entered.send(()).unwrap();
        }
        let mut backend = self.backend.lock().map_err(failure)?;
        let (e, v) = match &mut *backend {
            Backend::Gpu(g) => {
                // B5-07 begin: styled documents read back from the CPU compositor.
                if g.render_or_refuse(doc, level, None)?.is_none() {
                    g.cpu().render_level_rgba(doc, level)?
                } else {
                    g.resident.read_level(level, false)?
                }
                // B5-07 end
            }
            Backend::Cpu(c) => c.render_level_rgba(doc, level)?,
            Backend::Stopped => return Err(failure("document is closed")),
        };
        Ok((e.width, e.height, v))
    }
}

struct FrameAttempt<T> {
    result: Result<Option<T>>,
    record: Option<DocRenderRecord>,
}

/// The Signal decision happens before this call. Record exactly once, then let
/// the worker invoke accepted callbacks without holding the Signal lock.
fn finalize_frame<T>(
    renderer: &Renderer,
    attempt: FrameAttempt<T>,
    accepted: bool,
) -> Option<Result<Option<T>>> {
    let completed = accepted && matches!(&attempt.result, Ok(Some(_)));
    if let Some(mut record) = attempt.record {
        record.dropped |= !accepted;
        // Preserve the old policy for ordinary render errors and an idle
        // backend: neither produced a frame record. Cancelled work does.
        if completed || record.dropped {
            renderer.record(record);
        }
    }
    accepted.then_some(attempt.result)
}

pub(crate) fn worker_loop(shared: Arc<Shared>) {
    let r = &shared.render;
    run_frames(
        r,
        |since, cancel| present_frame(&shared, since, cancel),
        |result, layers, history| {
            let Some(listener) = shared.listener() else {
                return;
            };
            if let Some(result) = result {
                match result {
                    Ok(Some(info)) => listener.on_frame(info),
                    Ok(None) => {}
                    Err(e) => listener.on_render_failed(e.to_string()),
                }
            }
            if !layers.is_empty() {
                listener.on_layers_changed(layers.into_iter().collect());
            }
            if history && let Ok(st) = shared.lock() {
                let head = st.doc.history().current();
                drop(st);
                listener.on_history_changed(head);
            }
        },
    );
    // Free GPU memory as soon as the session stops.
    *r.backend.lock().unwrap_or_else(|e| e.into_inner()) = Backend::Stopped;
    r.thumbs.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let mut s = r.signal();
    s.busy = false;
    r.cv.notify_all();
}

/// The render thread's scheduling loop until `stop`. `present` renders one
/// frame (production: [`present_frame`]; tests inject a slow renderer) and
/// `deliver` receives the accepted result plus the changed rows and history
/// flag, called without the Signal lock.
fn run_frames<T>(
    r: &Renderer,
    mut present: impl FnMut(Instant, &CancellationToken) -> FrameAttempt<T>,
    mut deliver: impl FnMut(Option<Result<Option<T>>>, BTreeSet<u64>, bool),
) {
    loop {
        let (layers, history, since, cancel) = {
            let mut s = r.signal();
            while !s.stop && !s.pending() {
                s = r.cv.wait(s).unwrap_or_else(|e| e.into_inner());
            }
            if s.stop {
                break;
            }
            s.busy = true;
            let since = if s.frame { s.since.take() } else { None };
            let cancel = s.frame.then(|| s.begin_frame());
            s.frame = false;
            (
                std::mem::take(&mut s.layers),
                std::mem::take(&mut s.history),
                since,
                cancel,
            )
        };
        let attempt = if let Some(cancel) = &cancel {
            present(since.unwrap_or_else(Instant::now), cancel)
        } else {
            FrameAttempt {
                result: Ok(None),
                record: None,
            }
        };
        let publish = cancel
            .as_ref()
            .is_some_and(|cancel| r.signal().finish_frame(cancel));
        let result = finalize_frame(r, attempt, publish);
        deliver(result, layers, history);
        let mut s = r.signal();
        s.busy = false;
        r.cv.notify_all();
    }
}

// B5-14 begin
fn log_frames() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("TESSERA_DOC_RENDER_LOG").is_some())
}

fn log_resources() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| log_frames() || std::env::var_os("TESSERA_DOC_PERF_LOG").is_some())
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn union_of(rects: &[Rect]) -> Rect {
    rects.iter().fold(Rect::default(), |a, r| a.union(r))
}

/// Renders and presents one frame into the next surface of the ring (WP
/// B5-14, P13/P14/P17). The live-state lock is held only to snapshot the
/// document (an `Arc`, copy-on-write) and the view; filter preparation,
/// composition (GPU or the CPU style fallback) and presentation run from
/// the snapshot. The result is published only if its surface ring is still
/// the session's. `None` without a surface, or when dropped.
fn present_frame(
    shared: &Arc<Shared>,
    since: Instant,
    cancel: &CancellationToken,
) -> FrameAttempt<DocFrameInfo> {
    if let Err(error) = cancel.check() {
        return FrameAttempt {
            result: Err(error.into()),
            record: None,
        };
    }
    let r = &shared.render;
    let started = Instant::now();
    let st = match shared.lock() {
        Ok(st) => st,
        Err(error) => {
            return FrameAttempt {
                result: Err(error),
                record: None,
            };
        }
    };
    let locked = Instant::now();
    if st.closed || st.view.surfaces.is_empty() {
        return FrameAttempt {
            result: Ok(None),
            record: None,
        };
    }
    let index = st.view.next % st.view.surfaces.len();
    let surface = st.view.surfaces[index].clone();
    let attached: Vec<u32> = st.view.surfaces.iter().map(|s| s.id()).collect();
    let snapshot = st.live_shared();
    let canvas = snapshot.state().canvas;
    let (level, src, zoom) = resolve(st.view.viewport, canvas, surface.width(), surface.height());
    let epoch = st.epoch;
    let generation = st.view.generation;
    r.signal().snapshot_taken(cancel);
    drop(st);
    let unlocked = Instant::now();
    let le = canvas.at_level(level);
    let mut report = compositor::resident::FrameReport::default();
    let mut rec = DocRenderRecord {
        path: DocRenderPath::Cpu,
        level,
        visible: src,
        requested: src,
        dispatched: Rect::default(),
        blocks: 0,
        epoch,
        superseded: false,
        dropped: false,
        lock_wait_ms: ms(locked - started),
        lock_held_ms: ms(unlocked - locked),
        prep_ms: 0.0,
        composite_ms: 0.0,
        gpu_ms: 0.0,
        total_ms: 0.0,
    };
    let result = (|| -> Result<Option<DocFrameInfo>> {
        if !src.is_empty() {
            let _pressure = Pressure::begin(PressureKind::Render);
            // Smart filters baked and a filter preview shown (WP B5-05).
            // Pass the resolved canvas level: all Camera Raw stages omit detail
            // above level 0, including saved stacks without an active edit (B5-34).
            let t = Instant::now();
            let doc: Arc<Document> =
                super::filtering::presented(shared, &snapshot, level, src).unwrap_or(snapshot);
            rec.prep_ms = ms(t.elapsed());
            cancel.check()?;
            let mut backend = r.backend.lock().map_err(failure)?;
            cancel.check()?;
            match &mut *backend {
                Backend::Gpu(g) => {
                    g.targets.retain(|id, _| attached.contains(id));
                    // B5-15: stage results of a finished filter interaction.
                    if r.trim_filters.swap(false, Ordering::Relaxed)
                        && g.resident.filter_cache_bytes() > FILTER_CACHE_KEEP
                    {
                        super::filtering::install_resident(&mut g.resident);
                    }
                    let viewport = r.viewport_rendering.load(Ordering::Relaxed)
                        && !needs_full_halo(doc.state());
                    rec.path = if viewport {
                        DocRenderPath::Viewport
                    } else {
                        DocRenderPath::FullLevel
                    };
                    let m = i64::from(VIEWPORT_HALO);
                    rec.requested = if viewport {
                        Rect::new(src.x0 - m, src.y0 - m, src.x1 + m, src.y1 + m)
                            .intersect(&Rect::of_extent(le))
                    } else {
                        Rect::of_extent(le)
                    };
                    let t = Instant::now();
                    // B5-07: frames the resident program refuses (layer styles)
                    // are composited on the CPU, as the Cpu arm does.
                    let rendered = g.render_or_refuse(&doc, level, viewport.then_some(src));
                    rec.composite_ms = ms(t.elapsed());
                    match rendered? {
                        Some(fr) => {
                            // The GPU mirror holds what it needs: release the
                            // snapshot before waiting, so edits during the GPU
                            // work do not copy the document.
                            drop(doc);
                            rec.dispatched = union_of(&fr.damage);
                            rec.blocks = fr.blocks;
                            report = fr;
                            let t = Instant::now();
                            cancel.check()?;
                            let presented = g.present(level, src, &surface);
                            rec.gpu_ms = ms(t.elapsed());
                            presented?;
                            let waited = g.resident.wait();
                            rec.gpu_ms = ms(t.elapsed());
                            waited?;
                            cancel.check()?;
                        }
                        None => {
                            rec.path = DocRenderPath::Cpu;
                            rec.requested = src;
                            let copied = cpu_present(g.cpu(), &doc, level, src, &surface, cancel);
                            rec.composite_ms = ms(t.elapsed());
                            copied?;
                            report.full = true;
                        }
                    }
                }
                Backend::Cpu(c) => {
                    let t = Instant::now();
                    let copied = cpu_present(c, &doc, level, src, &surface, cancel);
                    rec.composite_ms = ms(t.elapsed());
                    copied?;
                    report.full = true;
                }
                Backend::Stopped => return Ok(None),
            }
        }
        // Cancelled partial CPU output is not published. Its ring slot is reused;
        // cpu_present clears aborted copies and successful copies overwrite all src.
        cancel.check()?;
        // Publish only into the ring this frame was rendered for.
        {
            let mut st = shared.lock()?;
            cancel.check()?;
            if st.closed
                || st.view.generation != generation
                || !st.view.surfaces.iter().any(|s| s.id() == surface.id())
            {
                let current = !st.closed && !st.view.surfaces.is_empty();
                drop(st);
                rec.dropped = true;
                if current {
                    // The ring that replaced this frame's gets a frame of its own.
                    r.request(Vec::new(), false, epoch);
                }
                return Ok(None);
            }
            st.view.next = index + 1;
            rec.superseded = st.epoch != epoch;
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
    })();
    rec.total_ms = ms(started.elapsed());
    FrameAttempt {
        result,
        record: Some(rec),
    }
}
// B5-14 end

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
    cancel: &CancellationToken,
) -> Result<()> {
    cancel.check()?;
    let tiles = c.render_region(doc, level, src, cancel)?;
    cancel.check()?;
    copy_cpu_tiles(&tiles, src, surface, || cancel.check())
}

/// The check callback is a deterministic row-boundary seam for cancellation tests.
/// Production always supplies the current frame token's check, never a snapshot.
fn copy_cpu_tiles(
    tiles: &[Tile],
    src: Rect,
    surface: &Surface,
    mut check: impl FnMut() -> EngineResult<()>,
) -> Result<()> {
    surface
        .with_pixels(|px, stride| -> EngineResult<()> {
            let copied = (|| -> EngineResult<()> {
                check()?;
                for t in tiles {
                    check()?;
                    let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
                    let l = t.layout();
                    let n = l.plane_len();
                    let s = t.samples::<f32>()?;
                    for y in 0..l.extent.height as i64 {
                        check()?;
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
                check()?;
                Ok(())
            })();
            // Never leave an aborted partial copy to be mistaken for a complete
            // frame when this unpublished ring slot is later recycled.
            if copied.is_err() {
                px.fill(0);
            }
            copied
        })
        .map_err(failure)??;
    Ok(())
}

// ─────────────────────────────── thumbnails ───────────────────────────────

/// A one-layer document showing `layer`'s own content (Normal, opaque,
/// unmasked, unclipped).
fn solo(state: &DocState, layer: &Layer) -> Document {
    // Smart filters are baked by the session, never by the compositor.
    let mut l = super::filtering::unfiltered(layer);
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
    // B5-14: `stable` documents keep their cache key across calls, so the
    // persistent thumbnail compositor reuses their layers' mips.
    let (rev, doc, stable) = {
        let st = shared.lock()?;
        st.open()?;
        let live = st.live();
        let s = live.state();
        match kind {
            ThumbKind::Layer(id) => {
                let l = find(s, id)?;
                (layer_revision(l), Arc::new(solo(s, l)), false)
            }
            ThumbKind::Mask(id) => {
                let l = find(s, id)?;
                let m = l
                    .mask
                    .as_ref()
                    .ok_or_else(|| failure(format!("layer {id} has no mask")))?;
                (
                    m.raster.max_rev().max(l.content_rev),
                    Arc::new(mask_doc(s, l)?),
                    false,
                )
            }
            // B5-14: the live document itself when it has no smart filters.
            // The resident renderer (below) keeps its mip pages across
            // edits; without Metal or with layer styles the persistent CPU
            // compositor reuses mips while the document key is stable. Before,
            // every call re-reduced every layer from level 0 (seconds on the
            // main thread per edit for 60 × 18 MP layers).
            ThumbKind::Composite => {
                if has_smart_filters(s) {
                    let d = Document::new(super::filtering::unfiltered_state(s));
                    (s.rev, Arc::new(d), false)
                } else {
                    (s.rev, st.live_shared(), true)
                }
            }
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
    let e = canvas.at_level(level);
    let surface = Surface::create_rgba8(e.width, e.height).map_err(failure)?;
    // B5-14: a composite without styles comes from the resident renderer,
    // whose mip pages are content-addressed (they survive edits, drags and
    // document keys); the CPU compositors below re-reduce layers per key.
    let resident = if matches!(kind, ThumbKind::Composite) {
        r.resident_level(&doc, level)?
    } else {
        None
    };
    if let Some((re, rgba)) = resident {
        let w = re.width as usize;
        surface
            .with_pixels(|px, stride| {
                for y in 0..re.height as usize {
                    for x in 0..w {
                        let (i, o) = ((y * w + x) * 4, y * stride + x * 4);
                        for c in 0..4 {
                            px[o + c] = quantize(rgba[i + c]);
                        }
                    }
                }
            })
            .map_err(failure)?;
    } else {
        let tiles = if stable {
            // B5-14: persistent, so mips survive between calls (styled documents).
            r.thumb_comp
                .get_or_init(|| super::fonts::compositor(128 << 20))
                .render_level(&doc, level, &CancellationToken::new())?
        } else {
            super::fonts::compositor(64 << 20).render_level(
                &doc,
                level,
                &CancellationToken::new(),
            )? // B5-10
        };
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
    }
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
    // Smart filters are baked here (the compositor never evaluates them).
    let doc = &super::filtering::for_output(Document::new((**doc.state()).clone()))?;
    let (e, mut rgba) = super::fonts::compositor(128 << 20).render_level_rgba(doc, 0)?; // B5-10
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

/// SOURCE-ONLY tests, UNRUN on B. No app/window/GPU setup or large fixtures.
#[cfg(test)]
mod frame_cancellation_tests {
    use super::*;

    fn sample_record() -> DocRenderRecord {
        DocRenderRecord {
            path: DocRenderPath::Cpu,
            level: 0,
            visible: Rect::new(0, 0, 2, 2),
            requested: Rect::new(0, 0, 2, 2),
            dispatched: Rect::default(),
            blocks: 0,
            epoch: 1,
            superseded: false,
            dropped: false,
            lock_wait_ms: 0.0,
            lock_held_ms: 0.0,
            prep_ms: 0.0,
            composite_ms: 1.0,
            gpu_ms: 0.0,
            total_ms: 1.0,
        }
    }

    #[test]
    fn cancelled_in_progress_attempt_records_one_drop_and_no_publication() {
        let renderer = Renderer::new(None);
        let cancel = renderer.signal().begin_frame();
        cancel.cancel();
        let accepted = renderer.signal().finish_frame(&cancel);
        let attempt: FrameAttempt<()> = FrameAttempt {
            result: Err(EngineError::Cancelled.into()),
            record: Some(sample_record()),
        };
        assert!(finalize_frame(&renderer, attempt, accepted).is_none());
        let records = renderer.records();
        assert_eq!(records.len(), 1);
        assert!(records[0].dropped);
        assert_eq!(records[0].composite_ms, 1.0);
        assert_eq!(renderer.counts[3].load(Ordering::Relaxed), 1);
    }

    #[test]
    fn final_owner_gate_records_exactly_once_and_callbacks_remain_unlocked() {
        let renderer = Renderer::new(None);
        let old = renderer.signal().begin_frame();
        let old_attempt = FrameAttempt {
            result: Ok(Some(())),
            record: Some(sample_record()),
        };
        renderer.invalidate_frame();
        let old_accepted = renderer.signal().finish_frame(&old);
        assert!(!old_accepted);
        assert!(finalize_frame(&renderer, old_attempt, old_accepted).is_none());

        let fresh = renderer.signal().begin_frame();
        let fresh_accepted = renderer.signal().finish_frame(&fresh);
        assert!(fresh_accepted);
        // Cancellation after acceptance cannot retroactively revoke this callback.
        renderer.invalidate_frame();
        let fresh_attempt = FrameAttempt {
            result: Ok(Some(())),
            record: Some(sample_record()),
        };
        assert!(matches!(
            finalize_frame(&renderer, fresh_attempt, fresh_accepted),
            Some(Ok(Some(())))
        ));
        let records = renderer.records();
        assert_eq!(records.len(), 2);
        assert!(records[0].dropped);
        assert!(!records[1].dropped);
        assert_eq!(renderer.counts[3].load(Ordering::Relaxed), 1);
        assert_eq!(
            renderer.counts[DocRenderPath::Cpu as usize].load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn supersession_and_late_completion_preserve_fresh_frame_owner() {
        let mut signal = Signal::default();
        let first = signal.begin_frame();
        signal.request_frame();
        assert!(!first.is_cancelled(), "drafts never cancel (B5-22)");
        let next = signal.begin_frame();
        assert!(first.is_cancelled());
        assert!(!signal.finish_frame(&first));
        assert!(Arc::ptr_eq(signal.active_frame.as_ref().unwrap(), &next));
        assert!(!next.is_cancelled());
        assert!(signal.finish_frame(&next));
        assert!(signal.active_frame.is_none());
        assert!(
            !signal.finish_frame(&next),
            "publication is accepted only once"
        );
    }

    #[test]
    fn layer_notification_preserves_frame_but_stop_cancels_it() {
        let renderer = Renderer::new(None);
        let current = renderer.signal().begin_frame();
        renderer.notify_layers(vec![1], 0);
        assert!(!current.is_cancelled());
        assert!(!renderer.signal().frame);
        renderer.stop();
        assert!(current.is_cancelled());
        assert!(!renderer.signal().finish_frame(&current));
    }

    #[test]
    fn eng2_snapshot_coalescing_preserves_later_and_replacement_requests() {
        let mut signal = Signal::default();
        let first = signal.begin_frame();
        signal.request_frame();
        signal.snapshot_taken(&first);
        assert!(!signal.frame);
        // An edit after the snapshot must still render when this frame ends.
        signal.request_frame();
        assert!(signal.finish_frame(&first));
        assert!(signal.frame);
        let next = signal.begin_frame();
        signal.cancel_frame();
        // A replaced ring cancels this token and queues its replacement.
        // The cancelled worker must not consume that replacement request.
        signal.snapshot_taken(&next);
        assert!(signal.frame);
        assert!(!signal.finish_frame(&next));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn eng2b_presented_frame_does_not_republish_model_or_retain_surfaces() {
        let dir = tempfile::tempdir().unwrap();
        let engine =
            crate::Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        let session = engine.adopt_document(tiny_document(), "synthetic".into());
        session.wait_idle();
        let surface = Arc::new(Surface::create_rgba8(3, 2).unwrap());
        {
            let mut st = session.shared.lock().unwrap();
            st.view.surfaces.push(surface.clone());
        }
        let before = session.shared.read().unwrap();
        session.shared.render.request(Vec::new(), false, 0);
        session.wait_idle();
        assert_eq!(session.shared.render.records().len(), 1);
        assert!(
            Arc::ptr_eq(&before, &session.shared.read().unwrap()),
            "ring cursor advancement republished the whole model"
        );
        assert_eq!(
            Arc::strong_count(&surface),
            2,
            "reader publication retains an IOSurface"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn eng2_requests_before_snapshot_do_not_render_twice() {
        let dir = tempfile::tempdir().unwrap();
        let engine =
            crate::Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        let session = engine.adopt_document(tiny_document(), "synthetic".into());
        let mut state = session.shared.lock().unwrap();
        state
            .view
            .surfaces
            .push(Arc::new(Surface::create_rgba8(3, 2).unwrap()));
        let renderer = &session.shared.render;
        renderer.request(Vec::new(), false, 0);
        // The worker has claimed the first request but cannot snapshot until
        // this edit guard is released. The second request is already covered
        // by that future snapshot, so it must not cause a duplicate frame.
        let deadline = Instant::now() + Duration::from_secs(30);
        while renderer.signal().active_frame.is_none() {
            assert!(Instant::now() < deadline, "worker did not claim frame");
            std::thread::sleep(Duration::from_millis(1));
        }
        renderer.request(Vec::new(), false, 0);
        drop(state);
        session.wait_idle();
        assert_eq!(
            renderer.records().len(),
            1,
            "duplicate frame for requests already captured by one snapshot"
        );
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn eng2_getters_do_not_wait_for_inflight_render() {
        use std::sync::mpsc;
        let dir = tempfile::tempdir().unwrap();
        let engine =
            crate::Engine::open(dir.path().join("support").to_string_lossy().into()).unwrap();
        let session = engine.adopt_document(tiny_document(), "synthetic".into());
        // Inject a stalled backend: read_presented_level must release session
        // state before it waits here, just as it must during a slow render.
        let backend = session.shared.render.backend.lock().unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        *session.shared.render.read_level_entered.lock().unwrap() = Some(entered_tx);
        let rendering = session.clone();
        let worker = std::thread::spawn(move || rendering.read_presented_level(0).unwrap());
        entered_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        let (tx, rx) = mpsc::channel();
        let reading = session.clone();
        let reader = std::thread::spawn(move || {
            let start = Instant::now();
            reading.info().unwrap();
            reading.layers().unwrap();
            reading.history_items().unwrap();
            reading.document_state().unwrap();
            reading
                .begin_export_flat(
                    "synthetic.png".into(),
                    crate::ExportFormat::Png,
                    90,
                    crate::ExportColor::Srgb,
                )
                .unwrap();
            tx.send(start.elapsed()).unwrap();
        });
        let result = rx.recv_timeout(Duration::from_millis(250));
        drop(backend);
        worker.join().unwrap();
        reader.join().unwrap();
        let elapsed = result.expect("session getters waited on the stalled render");
        eprintln!("ENG-2 getter batch: {elapsed:?}");
        assert!(elapsed < Duration::from_millis(250));
    }

    #[test]
    fn invalidation_does_not_wait_for_render_backend_lock() {
        use std::sync::mpsc;
        let renderer = Arc::new(Renderer::new(None));
        let current = renderer.signal().begin_frame();
        let backend = renderer.backend.lock().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let other = renderer.clone();
        let worker = std::thread::spawn(move || {
            other.invalidate_frame();
            done_tx.send(()).unwrap();
        });
        let completed = done_rx.recv_timeout(Duration::from_secs(5));
        drop(backend); // release even on failure so the worker can exit
        worker.join().unwrap();
        completed.expect("cancellation must not wait on backend rendering");
        assert!(current.is_cancelled());
        assert!(!renderer.signal().finish_frame(&current));
    }

    #[cfg(target_os = "macos")]
    fn tiny_document() -> Document {
        let mut state = DocState::new(Extent::new(3, 2), compositor::Depth::F32);
        state.root.push(Arc::new(Layer::new(
            "red",
            LayerKind::Fill(Fill::Solid {
                color: [1.0, 0.0, 0.0],
            }),
        )));
        Document::new(state)
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn cancelled_cpu_region_never_reaches_surface_or_publication() {
        let compositor = Compositor::new(1 << 20);
        let document = tiny_document();
        let surface = Surface::create_rgba8(2, 2).unwrap();
        surface.with_pixels(|px, _| px.fill(0x5a)).unwrap();
        let mut signal = Signal::default();
        let cancel = signal.begin_frame();
        signal.cancel_frame();
        assert!(
            cpu_present(
                &compositor,
                &document,
                0,
                Rect::new(1, 0, 3, 2),
                &surface,
                &cancel
            )
            .is_err()
        );
        surface
            .with_pixels(|px, _| assert!(px.iter().all(|v| *v == 0x5a)))
            .unwrap();
        assert!(!signal.finish_frame(&cancel));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn aborted_row_copy_is_cleared_and_fresh_region_fully_overwrites_it() {
        let compositor = Compositor::new(1 << 20);
        let document = tiny_document();
        let region = Rect::new(1, 0, 3, 2);
        let surface = Surface::create_rgba8(2, 2).unwrap();
        let mut signal = Signal::default();
        let cancel = signal.begin_frame();
        let tiles = compositor
            .render_region(&document, 0, region, &cancel)
            .unwrap();
        let mut checks = 0;
        let copied = copy_cpu_tiles(&tiles, region, &surface, || {
            checks += 1;
            if checks == 4 {
                cancel.cancel();
            } // after first row, before second
            cancel.check()
        });
        assert!(copied.is_err());
        assert!(!signal.finish_frame(&cancel));
        surface
            .with_pixels(|px, _| assert!(px.iter().all(|v| *v == 0)))
            .unwrap();
        let fresh = signal.begin_frame();
        cpu_present(&compositor, &document, 0, region, &surface, &fresh).unwrap();
        assert!(signal.finish_frame(&fresh));
        surface
            .with_pixels(|px, stride| {
                for y in 0..2 {
                    for x in 0..2 {
                        assert_eq!(
                            &px[y * stride + x * 4..y * stride + x * 4 + 4],
                            &[255, 0, 0, 255]
                        );
                    }
                }
            })
            .unwrap();
    }

    // B5-22: latest-wins coalescing. A draft never cancels the frame in
    // flight; the worker renders the newest state as soon as it completes.

    #[test]
    fn draft_request_keeps_in_flight_frame_and_coalesces_the_next() {
        let renderer = Renderer::new(None);
        let current = renderer.signal().begin_frame();
        renderer.request(Vec::new(), false, 0);
        renderer.request(Vec::new(), false, 0);
        assert!(
            !current.is_cancelled(),
            "a new draft must not cancel the frame in flight"
        );
        let mut s = renderer.signal();
        assert!(s.finish_frame(&current), "the in-flight frame publishes");
        assert!(s.frame, "one coalesced frame stays pending");
    }

    #[test]
    fn drafts_faster_than_frame_time_keep_publishing_latest_wins() {
        use std::thread;
        const FRAME: Duration = Duration::from_millis(20);
        const GAP: Duration = Duration::from_millis(4);
        const DRAFTS: u64 = 100;
        let renderer = Arc::new(Renderer::new(None));
        let draft = Arc::new(AtomicU64::new(0));
        let published = Arc::new(Mutex::new(Vec::<u64>::new()));
        let worker = {
            let (renderer, draft, published) = (renderer.clone(), draft.clone(), published.clone());
            thread::spawn(move || {
                run_frames(
                    &renderer,
                    |_, cancel| {
                        // Snapshot at frame start, like present_frame.
                        let seen = draft.load(Ordering::SeqCst);
                        let deadline = Instant::now() + FRAME;
                        while Instant::now() < deadline {
                            if let Err(e) = cancel.check() {
                                return FrameAttempt {
                                    result: Err(e.into()),
                                    record: None,
                                };
                            }
                            thread::sleep(Duration::from_millis(1));
                        }
                        FrameAttempt {
                            result: Ok(Some(seen)),
                            record: None,
                        }
                    },
                    |result, _, _| {
                        if let Some(Ok(Some(seen))) = result {
                            published.lock().unwrap().push(seen);
                        }
                    },
                )
            })
        };
        let started = Instant::now();
        for i in 1..=DRAFTS {
            draft.store(i, Ordering::SeqCst);
            renderer.request(Vec::new(), false, 0);
            thread::sleep(GAP);
        }
        let dragged = started.elapsed();
        renderer.wait_idle();
        renderer.stop();
        worker.join().unwrap();
        let frames = published.lock().unwrap().clone();
        // At least one frame per two frame times while drafts stream in.
        let floor = (dragged.as_millis() / (2 * FRAME.as_millis())).max(2) as usize;
        assert!(
            frames.len() >= floor,
            "frame starvation: {} frames for {DRAFTS} drafts over {dragged:?} (want >= {floor})",
            frames.len()
        );
        assert_eq!(
            frames.last(),
            Some(&DRAFTS),
            "final frame shows the last draft"
        );
        assert!(
            // `<=`: a microsecond race can publish the last draft twice.
            frames.windows(2).all(|w| w[0] <= w[1]),
            "frames advance through newer drafts: {frames:?}"
        );
    }

    /// Drives the real session paths: a frame is held in flight (the test owns
    /// the backend lock the worker needs), then `attach_surface` replaces the
    /// ring and `detach_surfaces` releases it. Each must cancel that frame at
    /// once. The generation gate alone would also keep it from publishing, so
    /// the cancellation assertion fails if either path drops `invalidate_frame`.
    #[test]
    #[cfg(target_os = "macos")]
    fn ring_replacement_and_detach_cancel_the_in_flight_frame() {
        use crate::surface::testing::create_rgba8;
        #[derive(Default)]
        struct Frames(Mutex<Vec<u32>>);
        impl super::super::DocumentListener for Frames {
            fn on_frame(&self, frame: DocFrameInfo) {
                self.0.lock().unwrap().push(frame.surface_id);
            }
            fn on_layers_changed(&self, _: Vec<u64>) {}
            fn on_history_changed(&self, _: u64) {}
            fn on_render_failed(&self, _: String) {}
        }
        let dir = tempfile::tempdir().unwrap();
        let engine =
            crate::Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
        let session = engine.adopt_document(tiny_document(), "t".into());
        let frames = Arc::new(Frames::default());
        session.set_listener(Some(frames.clone()));
        let render = &session.shared.render;
        let in_flight = || {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                if let Some(cancel) = render.signal().active_frame.clone() {
                    return cancel;
                }
                assert!(Instant::now() < deadline, "frame never started");
                std::thread::sleep(Duration::from_millis(1));
            }
        };

        let old = create_rgba8(2, 2);
        session.attach_surface(old, 2, 2).unwrap();
        session.wait_idle();
        assert_eq!(*frames.0.lock().unwrap(), vec![old], "first paint");

        // Ring replaced (a different size) while a frame is in flight.
        let backend = render.backend.lock().unwrap();
        session.refresh().unwrap();
        let held = in_flight();
        assert!(!held.is_cancelled());
        let new = create_rgba8(3, 2);
        session.attach_surface(new, 3, 2).unwrap();
        let cancelled = held.is_cancelled();
        drop(backend); // release even on failure so the worker can finish
        assert!(
            cancelled,
            "attach_surface replacing the ring cancels the frame"
        );
        session.wait_idle();
        assert_eq!(
            *frames.0.lock().unwrap(),
            vec![old, new],
            "the cancelled frame never publishes; the new ring gets its own"
        );

        // Surfaces released while a frame is in flight.
        let backend = render.backend.lock().unwrap();
        session.refresh().unwrap();
        let held = in_flight();
        assert!(!held.is_cancelled());
        session.detach_surfaces();
        let cancelled = held.is_cancelled();
        drop(backend);
        assert!(cancelled, "detach_surfaces cancels the frame");
        session.wait_idle();
        assert_eq!(
            *frames.0.lock().unwrap(),
            vec![old, new],
            "nothing publishes after detach"
        );
        session.close();
    }

    #[test]
    fn invalidation_and_stop_still_cancel_the_in_flight_frame_promptly() {
        use std::sync::mpsc;
        use std::thread;
        // A frame that would take a minute unless cancelled.
        const FRAME: Duration = Duration::from_secs(60);
        let renderer = Arc::new(Renderer::new(None));
        let (started_tx, started_rx) = mpsc::channel();
        let (ended_tx, ended_rx) = mpsc::channel();
        let published = Arc::new(AtomicU64::new(0));
        let worker = {
            let (renderer, published) = (renderer.clone(), published.clone());
            thread::spawn(move || {
                run_frames(
                    &renderer,
                    |_, cancel| {
                        started_tx.send(()).unwrap();
                        let deadline = Instant::now() + FRAME;
                        let result = loop {
                            if let Err(e) = cancel.check() {
                                break Err(e.into());
                            }
                            if Instant::now() >= deadline {
                                break Ok(Some(()));
                            }
                            thread::sleep(Duration::from_millis(1));
                        };
                        ended_tx.send(()).unwrap();
                        FrameAttempt {
                            result,
                            record: None,
                        }
                    },
                    |result, _, _| {
                        if result.is_some() {
                            published.fetch_add(1, Ordering::SeqCst);
                        }
                    },
                )
            })
        };
        let bound = Duration::from_secs(5);
        // Generation change (ring replaced or released).
        renderer.request(Vec::new(), false, 0);
        started_rx.recv_timeout(bound).expect("frame started");
        renderer.request(Vec::new(), false, 0); // a draft: must not cancel
        assert!(ended_rx.recv_timeout(Duration::from_millis(50)).is_err());
        renderer.invalidate_frame();
        ended_rx
            .recv_timeout(bound)
            .expect("invalidation cancels the in-flight frame promptly");
        // The pending draft frame starts next; closing cancels it.
        started_rx
            .recv_timeout(bound)
            .expect("coalesced frame started");
        renderer.stop();
        ended_rx
            .recv_timeout(bound)
            .expect("stop cancels the in-flight frame promptly");
        worker.join().unwrap();
        assert_eq!(
            published.load(Ordering::SeqCst),
            0,
            "cancelled frames never publish"
        );
    }
}
