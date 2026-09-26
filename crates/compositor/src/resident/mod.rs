//! GPU-resident document rendering: the compositor's interactive path
//! (COMPOSITOR.md §10).
//!
//! A [`ResidentRenderer`] mirrors one open document on the GPU:
//!
//! - **Pages.** Every stored level-0 tile of every layer and mask is
//!   uploaded once, byte for byte, into a page of a GPU page pool, keyed by
//!   the tile's buffer identity, so copy-on-write duplicates share pages and
//!   an edit uploads only the tiles it replaced. Mip tiles are pages too,
//!   hash-consed by `(level, tile, child pages)` and computed on the GPU the
//!   first time they are needed; a brush dab creates one new page per level
//!   above the dab and nothing else.
//! - **Program.** The layer tree is flattened into a step list (blend,
//!   adjustment, group push/pop, knockout snapshot) plus per-raster page
//!   tables, held in persistent GPU buffers that are rewritten only when
//!   they change.
//! - **Frames.** One dispatch composites a whole level, or only the 16²
//!   blocks damaged since the level was last rendered, into a resident
//!   premultiplied f32 level buffer. No per-tile uploads, no CPU pixel work.
//!   Adjustment layers run on the GPU.
//! - **Output.** [`ResidentRenderer::present`] writes a region into an
//!   RGBA8 storage texture (an IOSurface via
//!   [`ResidentRenderer::present_iosurface`]); readback is explicit
//!   ([`ResidentRenderer::read_level`], [`ResidentRenderer::read_tiles`]).
//!
//! Smart objects are the one CPU-assisted source: their resampled tiles
//! are rendered by the CPU compositor and uploaded as f32 pages, once per
//! child revision and tile.

mod pool;
mod program;

use std::collections::HashMap;
use std::sync::Arc;

use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord, TileFormat, TileLayout};
use engine_api::{EngineError, EngineResult};
use wgpu::util::DeviceExt;

use crate::document::{Layer, LayerId, LayerKind};
use crate::edit::Document;
use crate::geom::Rect;
use crate::gpu::{GpuCompositor, internal, shader};
use crate::raster::{Depth, Raster, buffer_addr};
use crate::render::{Compositor, DocRef, MAX_LEVEL};
use pool::{Pool, SLABS};
pub use program::MAX_NESTING;
use program::{Part, Program, TableRef};

const NONE: u32 = u32::MAX;
/// Composite blocks are 16² pixels (one workgroup).
const BLOCK: u32 = 16;
/// Words per f32 RGBA page (smart-object pages).
const F32_PAGE_WORDS: u64 = 256 * 256 * 4;

/// Compute pipelines of the resident path, compiled once per device.
pub(crate) struct Pipelines {
    doc: wgpu::ComputePipeline,
    mip: wgpu::ComputePipeline,
    present: wgpu::ComputePipeline,
}

/// Storage bindings the document shader needs.
const STORAGE_BINDINGS: u32 = 14;

impl Pipelines {
    pub(crate) fn new(device: &wgpu::Device) -> EngineResult<Self> {
        let have = device.limits().max_storage_buffers_per_shader_stage;
        if have < STORAGE_BINDINGS {
            return Err(EngineError::Unsupported {
                what: format!(
                    "resident compositor needs {STORAGE_BINDINGS} storage buffers per stage, device has {have} (use gpu_core::GpuDevice)"
                ),
            });
        }
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let pages = |access: &str| include_str!("pages.wgsl").replace("ACCESS", access);
        let make = |label: &str, src: String| {
            let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some(label),
                source: wgpu::ShaderSource::Wgsl(src.into()),
            });
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(label),
                layout: None,
                module: &module,
                entry_point: Some("main"),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let doc = make(
            "resident document",
            shader(&format!("{}\n{}", pages("read"), include_str!("doc.wgsl"))),
        );
        let mip = make(
            "resident mip",
            format!("{}\n{}", pages("read_write"), include_str!("mip.wgsl")),
        );
        let present = make("resident present", include_str!("present.wgsl").into());
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(internal(e));
        }
        Ok(Self { doc, mip, present })
    }
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PoolUniform {
    depth: u32,
    page_words: u32,
    inv16: f32,
    _p: u32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct FrameUniform {
    lw: u32,
    lh: u32,
    cols: u32,
    nsteps: u32,
    blocks: u32,
    bcols: u32,
    brows: u32,
    clamp: u32,
    level: u32,
    _p: [u32; 3],
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Job {
    dst: u32,
    kids: [u32; 4],
    cw: u32,
    ch: u32,
    tx: u32,
    ty: u32,
    chans: u32,
    def_code: u32,
    def_f: f32,
}

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct PresentUniform {
    lw: u32,
    sx: u32,
    sy: u32,
    dx: u32,
    dy: u32,
    w: u32,
    h: u32,
    flatten: u32,
    bg: [f32; 4],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct MipKey {
    level: u8,
    tx: u32,
    ty: u32,
    kids: [u64; 4],
    chans: u8,
    default: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct SmartKey {
    doc: u64,
    layer: u64,
    stamp: u64,
    coord: TileCoord,
}

#[derive(Debug, Clone, Copy)]
enum NodeKey {
    L0(usize),
    Mip(MipKey),
    Smart(SmartKey),
}

struct Node {
    page: u32,
    last: u64,
    key: NodeKey,
}

impl Node {
    fn smart(&self) -> bool {
        matches!(self.key, NodeKey::Smart(_))
    }
}

/// Per-layer resolved page tables, reused while the layer's `Arc` is
/// unchanged.
struct LayerCache {
    layer: Arc<Layer>,
    generation: u64,
    seen: u64,
    content: Vec<Vec<u64>>,
    mask: Vec<Vec<u64>>,
    smart: HashMap<u8, Vec<u64>>,
}

/// A GPU buffer that persists across frames, grows by powers of two and is
/// written only when its contents change.
struct Persistent {
    buffer: Option<wgpu::Buffer>,
    shadow: Vec<u8>,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl Persistent {
    fn new(label: &'static str, usage: wgpu::BufferUsages) -> Self {
        Self {
            buffer: None,
            shadow: Vec::new(),
            usage: usage | wgpu::BufferUsages::COPY_DST,
            label,
        }
    }

    /// Returns true when the GPU copy was rewritten.
    fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, bytes: &[u8]) -> bool {
        let len = (bytes.len().max(16) as u64).next_power_of_two().max(256);
        if self.buffer.as_ref().is_none_or(|b| b.size() < len) {
            self.buffer = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(self.label),
                size: len,
                usage: self.usage,
                mapped_at_creation: false,
            }));
            self.shadow.clear();
        }
        if self.shadow == bytes {
            return false;
        }
        if let Some(b) = &self.buffer {
            let mut padded;
            let data = if bytes.len().is_multiple_of(4) {
                bytes
            } else {
                padded = bytes.to_vec();
                padded.resize(bytes.len().next_multiple_of(4), 0);
                &padded
            };
            if !data.is_empty() {
                queue.write_buffer(b, 0, data);
            }
        }
        self.shadow.clear();
        self.shadow.extend_from_slice(bytes);
        true
    }

    fn binding(&self) -> wgpu::BindingResource<'_> {
        self.buffer
            .as_ref()
            .expect("written before binding")
            .as_entire_binding()
    }
}

/// What a level buffer was last rendered from.
struct Rendered {
    key: u64,
    epoch: u64,
    rev: u64,
    program: Vec<u8>,
    nodes: Vec<u64>,
}

struct LevelState {
    out: wgpu::Buffer,
    extent: Extent,
    last: Option<Rendered>,
}

/// What one [`ResidentRenderer::render`] call did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FrameReport {
    /// The level was (re)composited in full.
    pub full: bool,
    /// 16² blocks composited.
    pub blocks: u32,
    /// Damaged rectangles in level coordinates (empty: nothing changed).
    pub damage: Vec<Rect>,
    /// Level-0 tile pages uploaded.
    pub uploaded_pages: u32,
    /// Bytes uploaded for them.
    pub uploaded_bytes: u64,
    /// Mip pages computed on the GPU.
    pub mip_pages: u32,
    /// Smart-object pages rendered on the CPU and uploaded.
    pub smart_pages: u32,
    /// Pages evicted to make room.
    pub evicted_pages: u32,
}

/// Cumulative counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ResidentStats {
    /// Frames rendered.
    pub frames: u64,
    /// Frames composited in full.
    pub full_frames: u64,
    /// Frames that composited only damaged blocks.
    pub partial_frames: u64,
    /// Frames with no damage (no dispatch).
    pub idle_frames: u64,
    /// 16² blocks composited.
    pub blocks: u64,
    /// Level-0 pages uploaded.
    pub uploaded_pages: u64,
    /// Bytes uploaded for them.
    pub uploaded_bytes: u64,
    /// Mip pages computed on the GPU.
    pub mip_pages: u64,
    /// Smart-object pages uploaded.
    pub smart_pages: u64,
    /// Pages evicted.
    pub evicted_pages: u64,
    /// Pages holding content.
    pub live_pages: u64,
    /// Bytes of GPU memory in page slabs and level buffers.
    pub resident_bytes: u64,
}

/// GPU-resident renderer for one document (see the module docs).
pub struct ResidentRenderer {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pipes: Arc<Pipelines>,
    depth: Option<Depth>,
    canvas: Extent,
    main: Pool,
    smart: Pool,
    nodes: HashMap<u64, Node>,
    next_id: u64,
    l0: HashMap<usize, (u64, Tile)>,
    mips: HashMap<MipKey, u64>,
    smarts: HashMap<SmartKey, u64>,
    layers: HashMap<LayerId, LayerCache>,
    generation: u64,
    frame: u64,
    levels: HashMap<u8, LevelState>,
    steps: Persistent,
    tables: Persistent,
    aux: Persistent,
    blocks: Persistent,
    jobs: Persistent,
    dummies: Vec<wgpu::Buffer>,
    cpu: Compositor,
    budget: u64,
    stats: ResidentStats,
    pending_l0: Vec<(u64, Tile)>,
    pending_mips: Vec<u64>,
    pending_smart: Vec<(u64, Tile)>,
    report: FrameReport,
}

fn page_bytes(depth: Depth) -> u64 {
    256 * 256 * 4 * depth.bytes() as u64
}

/// A tile's bytes in page layout: RGBA interleaved per texel, one-channel
/// tiles as stored.
fn page_bytes_of(t: &Tile) -> EngineResult<Vec<u8>> {
    fn interleave<T: Copy + Default>(s: &[T], planes: usize) -> Vec<T> {
        if planes == 1 {
            return s.to_vec();
        }
        let n = s.len() / planes;
        let mut out = vec![T::default(); s.len()];
        for (c, plane) in s.chunks_exact(n).enumerate() {
            for (i, v) in plane.iter().enumerate() {
                out[i * planes + c] = *v;
            }
        }
        out
    }
    let planes = t.layout().channels as usize;
    let mut v = match t.format() {
        TileFormat::U8 => interleave(t.samples::<u8>()?, planes),
        TileFormat::U16 => bytemuck::cast_slice(&interleave(t.samples::<u16>()?, planes)).to_vec(),
        TileFormat::F32Planar => {
            bytemuck::cast_slice(&interleave(t.samples::<f32>()?, planes)).to_vec()
        }
        TileFormat::F16Planar => {
            return Err(EngineError::Unsupported {
                what: "f16 rasters on the resident compositor".into(),
            });
        }
    };
    v.resize(v.len().next_multiple_of(4), 0);
    Ok(v)
}

/// A planar tile's bytes (smart-object pages), padded to words.
fn planar_bytes(t: &Tile) -> EngineResult<Vec<u8>> {
    let mut v: Vec<u8> = bytemuck::cast_slice(t.samples::<f32>()?).to_vec();
    v.resize(v.len().next_multiple_of(4), 0);
    Ok(v)
}

impl ResidentRenderer {
    /// A renderer on `gpu`'s device with a 2 GiB page budget.
    pub fn new(gpu: &GpuCompositor) -> EngineResult<Self> {
        Self::with_budget(gpu, 2 << 30)
    }

    /// A renderer whose page pool prefers eviction of pages not used by the
    /// current state (undo history) over growing past `budget` bytes.
    pub fn with_budget(gpu: &GpuCompositor, budget: u64) -> EngineResult<Self> {
        let device = gpu.device.clone();
        let pipes = gpu.resident_pipelines()?;
        let dummies = (0..SLABS + 1)
            .map(|_| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("resident unused slab"),
                    size: 16,
                    usage: wgpu::BufferUsages::STORAGE,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let storage = wgpu::BufferUsages::STORAGE;
        Ok(Self {
            main: Pool::new(&device, page_bytes(Depth::U8), "resident pages", SLABS)?,
            smart: Pool::new(&device, F32_PAGE_WORDS * 4, "resident smart pages", 1)?,
            queue: gpu.queue.clone(),
            device,
            pipes,
            depth: None,
            canvas: Extent::new(0, 0),
            nodes: HashMap::new(),
            next_id: 1,
            l0: HashMap::new(),
            mips: HashMap::new(),
            smarts: HashMap::new(),
            layers: HashMap::new(),
            generation: 0,
            frame: 0,
            levels: HashMap::new(),
            steps: Persistent::new("resident steps", storage),
            tables: Persistent::new("resident page tables", storage),
            aux: Persistent::new("resident aux", storage),
            blocks: Persistent::new("resident blocks", storage),
            jobs: Persistent::new("resident mip jobs", storage),
            dummies,
            cpu: Compositor::new(256 << 20),
            budget,
            stats: ResidentStats::default(),
            pending_l0: Vec::new(),
            pending_mips: Vec::new(),
            pending_smart: Vec::new(),
            report: FrameReport::default(),
        })
    }

    /// Cumulative counters.
    pub fn stats(&self) -> ResidentStats {
        let mut s = self.stats;
        s.live_pages = self.nodes.len() as u64;
        s.resident_bytes = self.main.bytes()
            + self.smart.bytes()
            + self.levels.values().map(|l| l.out.size()).sum::<u64>();
        s
    }

    fn reset(&mut self, depth: Depth, canvas: Extent) -> EngineResult<()> {
        self.main = Pool::new(&self.device, page_bytes(depth), "resident pages", SLABS)?;
        self.smart = Pool::new(&self.device, F32_PAGE_WORDS * 4, "resident smart pages", 1)?;
        self.nodes.clear();
        self.l0.clear();
        self.mips.clear();
        self.smarts.clear();
        self.layers.clear();
        self.levels.clear();
        self.depth = Some(depth);
        self.canvas = canvas;
        Ok(())
    }

    fn id(&mut self, key: NodeKey) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.insert(
            id,
            Node {
                page: NONE,
                last: self.frame,
                key,
            },
        );
        id
    }

    fn touch(&mut self, id: u64) {
        if let Some(n) = self.nodes.get_mut(&id) {
            n.last = self.frame;
        }
    }

    fn check_raster(&self, r: &Raster) -> EngineResult<()> {
        let depth = self.depth.unwrap_or_default();
        if r.depth() != depth || r.extent() != self.canvas {
            return Err(EngineError::Unsupported {
                what: "resident compositor: raster depth or extent differs from the document"
                    .into(),
            });
        }
        if depth != Depth::F32 && r.default_value() != 0.0 && r.default_value() != 1.0 {
            return Err(EngineError::Unsupported {
                what: "resident compositor: integer raster default other than 0 or 1".into(),
            });
        }
        Ok(())
    }

    fn intern_l0(&mut self, t: &Tile) -> u64 {
        let addr = buffer_addr(t);
        if let Some(&(id, _)) = self.l0.get(&addr) {
            self.touch(id);
            return id;
        }
        let id = self.id(NodeKey::L0(addr));
        self.l0.insert(addr, (id, t.clone()));
        self.pending_l0.push((id, t.clone()));
        id
    }

    fn intern_mip(&mut self, key: MipKey) -> u64 {
        if let Some(&id) = self.mips.get(&key) {
            self.touch(id);
            return id;
        }
        let id = self.id(NodeKey::Mip(key));
        self.mips.insert(key, id);
        self.pending_mips.push(id);
        id
    }

    /// Node ids of every tile of `raster` at levels `have..=level`.
    fn raster_levels(
        &mut self,
        raster: &Raster,
        levels: &mut Vec<Vec<u64>>,
        level: u8,
    ) -> EngineResult<()> {
        if levels.is_empty() {
            self.check_raster(raster)?;
            let (cols, rows) = self.canvas.tile_grid(TILE_SIZE);
            let mut v = vec![0u64; (cols * rows) as usize];
            for ((tx, ty), slot) in raster.slots() {
                if let Some(t) = &slot.tile {
                    v[(ty * cols + tx) as usize] = self.intern_l0(t);
                }
            }
            levels.push(v);
        }
        let chans = raster.channels();
        let default = raster.default_value().to_bits();
        while levels.len() <= level as usize {
            let l = levels.len() as u8;
            let (pc, pr) = self.canvas.at_level(l - 1).tile_grid(TILE_SIZE);
            let (cols, rows) = self.canvas.at_level(l).tile_grid(TILE_SIZE);
            let mut v = vec![0u64; (cols * rows) as usize];
            for ty in 0..rows {
                for tx in 0..cols {
                    let prev = &levels[l as usize - 1];
                    let kid = |x: u32, y: u32| {
                        if x < pc && y < pr {
                            prev[(y * pc + x) as usize]
                        } else {
                            0
                        }
                    };
                    let kids = [
                        kid(2 * tx, 2 * ty),
                        kid(2 * tx + 1, 2 * ty),
                        kid(2 * tx, 2 * ty + 1),
                        kid(2 * tx + 1, 2 * ty + 1),
                    ];
                    if kids != [0; 4] {
                        v[(ty * cols + tx) as usize] = self.intern_mip(MipKey {
                            level: l,
                            tx,
                            ty,
                            kids,
                            chans,
                            default,
                        });
                    }
                }
            }
            levels.push(v);
        }
        Ok(())
    }

    fn smart_level(&mut self, doc: &Document, layer: &Layer, level: u8) -> EngineResult<Vec<u64>> {
        let LayerKind::SmartObject(so) = &layer.kind else {
            return Err(EngineError::internal("smart table of a non-smart layer"));
        };
        let (cols, rows) = self.canvas.at_level(level).tile_grid(TILE_SIZE);
        let dref = DocRef {
            state: doc.state(),
            key: doc.key(),
        };
        let mut v = Vec::with_capacity((cols * rows) as usize);
        for ty in 0..rows {
            for tx in 0..cols {
                let coord = TileCoord::new(level, tx, ty);
                let key = SmartKey {
                    doc: doc.key(),
                    layer: layer.id.0,
                    stamp: so.state.rev.max(layer.content_rev),
                    coord,
                };
                let id = match self.smarts.get(&key) {
                    Some(&id) => {
                        self.touch(id);
                        id
                    }
                    None => {
                        let id = match self.cpu.smart_tile(dref, layer, so, coord)? {
                            None => 0,
                            Some(t) => {
                                let id = self.id(NodeKey::Smart(key));
                                self.pending_smart.push((id, t));
                                id
                            }
                        };
                        self.smarts.insert(key, id);
                        id
                    }
                };
                v.push(id);
            }
        }
        Ok(v)
    }

    /// Node ids of one page table at `level`.
    fn resolve(&mut self, doc: &Document, t: &TableRef, level: u8) -> EngineResult<Vec<u64>> {
        let id = t.layer.id;
        let mut entry = match self.layers.remove(&id) {
            Some(e) if Arc::ptr_eq(&e.layer, &t.layer) && e.generation == self.generation => e,
            _ => LayerCache {
                layer: t.layer.clone(),
                generation: self.generation,
                seen: self.frame,
                content: Vec::new(),
                mask: Vec::new(),
                smart: HashMap::new(),
            },
        };
        entry.seen = self.frame;
        let out = (|| -> EngineResult<Vec<u64>> {
            let layer = &*t.layer;
            let levels = match t.part {
                Part::Smart => {
                    if let std::collections::hash_map::Entry::Vacant(e) = entry.smart.entry(level) {
                        e.insert(self.smart_level(doc, layer, level)?);
                    }
                    let v = entry.smart[&level].clone();
                    for id in &v {
                        self.touch(*id);
                    }
                    return Ok(v);
                }
                Part::Content => {
                    let r = layer
                        .raster()
                        .ok_or_else(|| EngineError::internal("no raster"))?;
                    self.raster_levels(r, &mut entry.content, level)?;
                    &entry.content
                }
                Part::Mask => {
                    let m = layer
                        .mask
                        .as_ref()
                        .ok_or_else(|| EngineError::internal("no mask"))?;
                    self.raster_levels(&m.raster, &mut entry.mask, level)?;
                    &entry.mask
                }
            };
            for l in &levels[..=level as usize] {
                for id in l {
                    if *id != 0
                        && let Some(n) = self.nodes.get_mut(id)
                    {
                        n.last = self.frame;
                    }
                }
            }
            Ok(levels[level as usize].clone())
        })();
        self.layers.insert(id, entry);
        out
    }

    /// Frees least-recently-used pages of `smart`/main nodes not used by
    /// this frame until `need` pages are available.
    fn evict(&mut self, smart: bool, need: u32) -> u32 {
        let pool = if smart { &self.smart } else { &self.main };
        if pool.available() >= need {
            return 0;
        }
        let mut cands: Vec<(u64, u64)> = self
            .nodes
            .iter()
            .filter(|(_, n)| n.smart() == smart && n.page != NONE && n.last < self.frame)
            .map(|(id, n)| (n.last, *id))
            .collect();
        cands.sort_unstable();
        let mut freed = 0;
        for (_, id) in cands {
            let pool = if smart { &self.smart } else { &self.main };
            if pool.available() >= need {
                break;
            }
            let Some(n) = self.nodes.remove(&id) else {
                continue;
            };
            match n.key {
                NodeKey::L0(addr) => {
                    self.l0.remove(&addr);
                }
                NodeKey::Mip(k) => {
                    self.mips.remove(&k);
                }
                NodeKey::Smart(k) => {
                    self.smarts.remove(&k);
                }
            }
            if smart {
                self.smart.release(n.page);
            } else {
                self.main.release(n.page);
            }
            freed += 1;
        }
        if freed > 0 {
            // Cached layer tables may name evicted nodes.
            self.generation += 1;
        }
        freed
    }

    fn ensure(&mut self, smart: bool, need: u32) -> EngineResult<()> {
        let device = self.device.clone();
        let budget = self.budget;
        // Grow within the budget first (a slab is capped by the binding
        // limit, so a large first frame can take several).
        loop {
            let pool = if smart {
                &mut self.smart
            } else {
                &mut self.main
            };
            let short = need.saturating_sub(pool.available());
            if short == 0 {
                return Ok(());
            }
            if !pool.can_grow() || pool.bytes() + u64::from(short) * pool.page_bytes > budget {
                break;
            }
            pool.grow(&device, short)?;
        }
        // Then reclaim pages the current state does not use (history).
        let freed = self.evict(smart, need);
        self.report.evicted_pages += freed;
        // Then exceed the budget while slabs remain.
        loop {
            let pool = if smart {
                &mut self.smart
            } else {
                &mut self.main
            };
            let short = need.saturating_sub(pool.available());
            if short == 0 {
                return Ok(());
            }
            pool.grow(&device, short)?;
        }
    }

    fn assign(&mut self, id: u64, smart: bool) -> EngineResult<u32> {
        let pool = if smart {
            &mut self.smart
        } else {
            &mut self.main
        };
        let page = pool
            .alloc()
            .ok_or_else(|| EngineError::internal("page pool accounting"))?;
        if let Some(n) = self.nodes.get_mut(&id) {
            n.page = page;
        }
        Ok(page)
    }

    /// The shader address of a node's page (`NONE` when absent).
    fn page(&self, id: u64) -> u32 {
        if id == 0 {
            return NONE;
        }
        match self.nodes.get(&id) {
            Some(n) if n.page != NONE => {
                if n.smart() {
                    self.smart.packed(n.page)
                } else {
                    self.main.packed(n.page)
                }
            }
            _ => NONE,
        }
    }

    fn pool_uniform(&self) -> PoolUniform {
        PoolUniform {
            depth: match self.depth.unwrap_or_default() {
                Depth::U8 => 0,
                Depth::U16 => 1,
                Depth::F32 => 2,
            },
            page_words: (self.main.page_bytes / 4) as u32,
            inv16: 1.0 / 65535.0,
            _p: 0,
        }
    }

    fn slab_entries(&self) -> Vec<wgpu::BindGroupEntry<'_>> {
        (0..SLABS)
            .map(|i| wgpu::BindGroupEntry {
                binding: i as u32,
                resource: self
                    .main
                    .slabs
                    .get(i)
                    .map_or(&self.dummies[i], |s| &s.0)
                    .as_entire_binding(),
            })
            .collect()
    }

    /// Brings the GPU mirror to `doc`'s current state and composites
    /// `level` into its resident buffer: the whole level the first time,
    /// afterwards only the blocks whose inputs changed. Submits and returns
    /// without waiting ([`ResidentRenderer::wait`]).
    pub fn render(&mut self, doc: &Document, level: u8) -> EngineResult<FrameReport> {
        let state = doc.state();
        if level >= MAX_LEVEL {
            return Err(EngineError::invalid(
                "level",
                format!("must be below {MAX_LEVEL}"),
            ));
        }
        if state.canvas.width == 0 || state.canvas.height == 0 {
            return Err(EngineError::invalid("canvas", "empty"));
        }
        if self.depth != Some(state.depth) || self.canvas != state.canvas {
            self.reset(state.depth, state.canvas)?;
        }
        self.frame += 1;
        self.report = FrameReport::default();
        let le = state.canvas.at_level(level);
        let (cols, rows) = le.tile_grid(TILE_SIZE);
        let grid = (cols * rows) as usize;
        let program = Program::compile(&state.root, grid)?;

        // Phase 1: resolve every page table to content-addressed nodes.
        let mut nodes = Vec::with_capacity(program.tables.len() * grid);
        let resolved = (|| -> EngineResult<()> {
            for t in &program.tables {
                nodes.extend(self.resolve(doc, t, level)?);
            }
            Ok(())
        })();
        if let Err(e) = resolved {
            self.drop_pending();
            return Err(e);
        }
        self.layers.retain(|_, e| e.seen == self.frame);

        // Phase 2: pages for new nodes, uploads and mip jobs.
        let encoder = match self.materialize() {
            Ok(e) => e,
            Err(e) => {
                self.drop_pending();
                return Err(e);
            }
        };

        // Damage since this level was last rendered.
        let bytes = program.bytes();
        let level_rect = Rect::of_extent(le);
        let damage = self.damage(doc, level, &bytes, &nodes, grid, cols, le);
        let (bcols, brows) = (le.width.div_ceil(BLOCK), le.height.div_ceil(BLOCK));
        let total = bcols * brows;
        let mut list: Vec<u32> = Vec::new();
        let full = match &damage {
            None => true,
            Some(rects) => {
                let mut mark = vec![false; (bcols * brows) as usize];
                for r in rects {
                    let r = r.intersect(&level_rect);
                    if r.is_empty() {
                        continue;
                    }
                    for by in (r.y0 as u32 / BLOCK)..(r.y1 as u32).div_ceil(BLOCK) {
                        for bx in (r.x0 as u32 / BLOCK)..(r.x1 as u32).div_ceil(BLOCK) {
                            let i = (by * bcols + bx) as usize;
                            if !mark[i] {
                                mark[i] = true;
                                list.push(bx | (by << 16));
                            }
                        }
                    }
                }
                list.len() as u32 == total
            }
        };
        self.report.full = full;
        self.report.damage = match &damage {
            None => vec![level_rect],
            Some(r) => r
                .iter()
                .map(|r| r.intersect(&level_rect))
                .filter(|r| !r.is_empty())
                .collect(),
        };
        let nblocks = if full { total } else { list.len() as u32 };
        self.report.blocks = nblocks;

        let mut encoder = encoder;
        if nblocks > 0 {
            let pages: Vec<u32> = nodes.iter().map(|id| self.page(*id)).collect();
            let steps = bytemuck::cast_slice(&program.steps);
            let (device, queue) = (self.device.clone(), self.queue.clone());
            self.steps.write(&device, &queue, steps);
            self.tables
                .write(&device, &queue, bytemuck::cast_slice(&pages));
            self.aux
                .write(&device, &queue, bytemuck::cast_slice(&program.aux));
            self.blocks
                .write(&device, &queue, bytemuck::cast_slice(&list));
            let out_size = u64::from(le.width) * u64::from(le.height) * 16;
            if out_size > device.limits().max_storage_buffer_binding_size {
                return Err(EngineError::ResourceExhausted {
                    resource: format!("level {level} exceeds the storage binding limit"),
                });
            }
            let st = self.levels.entry(level).or_insert_with(|| LevelState {
                out: device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("resident level"),
                    size: out_size,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                    mapped_at_creation: false,
                }),
                extent: le,
                last: None,
            });
            let out = st.out.clone();
            let frame = FrameUniform {
                lw: le.width,
                lh: le.height,
                cols,
                nsteps: program.steps.len() as u32,
                blocks: if full { 0 } else { nblocks },
                bcols,
                brows,
                clamp: u32::from(!state.depth.is_float()),
                level: u32::from(level),
                _p: [0; 3],
            };
            let init = |label: &str, bytes: &[u8]| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some(label),
                    contents: bytes,
                    usage: wgpu::BufferUsages::UNIFORM,
                })
            };
            let fb = init("resident frame", bytemuck::bytes_of(&frame));
            let pb = init("resident pool", bytemuck::bytes_of(&self.pool_uniform()));
            let mut entries = self.slab_entries();
            let smart_buf = self
                .smart
                .slabs
                .first()
                .map_or(&self.dummies[SLABS], |s| &s.0);
            for (binding, resource) in [
                (8, smart_buf.as_entire_binding()),
                (9, pb.as_entire_binding()),
                (10, fb.as_entire_binding()),
                (11, self.steps.binding()),
                (12, self.tables.binding()),
                (13, self.aux.binding()),
                (14, self.blocks.binding()),
                (15, out.as_entire_binding()),
            ] {
                entries.push(wgpu::BindGroupEntry { binding, resource });
            }
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("resident document"),
                layout: &self.pipes.doc.get_bind_group_layout(0),
                entries: &entries,
            });
            let (gx, gy) = split(nblocks);
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("resident composite"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipes.doc);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(gx, gy, 1);
        }
        self.queue.submit([encoder.finish()]);
        if let Some(st) = self.levels.get_mut(&level) {
            st.last = Some(Rendered {
                key: doc.key(),
                epoch: doc.epoch(),
                rev: state.rev,
                program: bytes,
                nodes,
            });
        }
        let r = &self.report;
        let s = &mut self.stats;
        s.frames += 1;
        match (nblocks, full) {
            (0, _) => s.idle_frames += 1,
            (_, true) => s.full_frames += 1,
            _ => s.partial_frames += 1,
        }
        s.blocks += u64::from(nblocks);
        s.uploaded_pages += u64::from(r.uploaded_pages);
        s.uploaded_bytes += r.uploaded_bytes;
        s.mip_pages += u64::from(r.mip_pages);
        s.smart_pages += u64::from(r.smart_pages);
        s.evicted_pages += u64::from(r.evicted_pages);
        Ok(self.report.clone())
    }

    fn drop_pending(&mut self) {
        for id in self
            .pending_l0
            .drain(..)
            .map(|p| p.0)
            .chain(self.pending_mips.drain(..))
            .chain(self.pending_smart.drain(..).map(|p| p.0))
            .collect::<Vec<_>>()
        {
            if let Some(n) = self.nodes.remove(&id) {
                match n.key {
                    NodeKey::L0(a) => {
                        self.l0.remove(&a);
                    }
                    NodeKey::Mip(k) => {
                        self.mips.remove(&k);
                    }
                    NodeKey::Smart(k) => {
                        self.smarts.remove(&k);
                    }
                }
            }
        }
        self.layers.clear();
    }

    /// Allocates pages for pending nodes, queues their uploads and records
    /// the mip passes (level by level) into a new encoder.
    fn materialize(&mut self) -> EngineResult<wgpu::CommandEncoder> {
        let main_need = (self.pending_l0.len() + self.pending_mips.len()) as u32;
        let smart_need = self.pending_smart.len() as u32;
        self.ensure(false, main_need)?;
        self.ensure(true, smart_need)?;
        let (device, queue) = (self.device.clone(), self.queue.clone());
        // Interleave in parallel, a batch at a time (bounded host memory).
        let pending = std::mem::take(&mut self.pending_l0);
        for batch in pending.chunks(512) {
            let bytes: Vec<EngineResult<Vec<u8>>> = {
                use rayon::prelude::*;
                batch.par_iter().map(|(_, t)| page_bytes_of(t)).collect()
            };
            for ((id, _), bytes) in batch.iter().zip(bytes) {
                let bytes = bytes?;
                let page = self.assign(*id, false)?;
                let (buf, off) = self.main.locate(page);
                queue.write_buffer(buf, off, &bytes);
                self.report.uploaded_pages += 1;
                self.report.uploaded_bytes += bytes.len() as u64;
            }
        }
        for (id, tile) in std::mem::take(&mut self.pending_smart) {
            let page = self.assign(id, true)?;
            let bytes = planar_bytes(&tile)?;
            let (buf, off) = self.smart.locate(page);
            queue.write_buffer(buf, off, &bytes);
            self.report.smart_pages += 1;
        }
        // Mip jobs grouped by level, each group 256-byte aligned.
        let mut by_level: Vec<(u8, Job)> = Vec::new();
        for id in std::mem::take(&mut self.pending_mips) {
            let page = self.assign(id, false)?;
            let Some(NodeKey::Mip(k)) = self.nodes.get(&id).map(|n| n.key) else {
                continue;
            };
            let ce = self.canvas.at_level(k.level - 1);
            let def = f32::from_bits(k.default);
            by_level.push((
                k.level,
                Job {
                    dst: self.main.packed(page),
                    kids: k.kids.map(|c| self.page(c)),
                    cw: ce.width,
                    ch: ce.height,
                    tx: k.tx,
                    ty: k.ty,
                    chans: u32::from(k.chans),
                    def_code: match (def == 1.0, self.depth) {
                        (false, _) => 0,
                        (true, Some(Depth::U16)) => 65535,
                        (true, _) => 255,
                    },
                    def_f: def,
                },
            ));
        }
        self.report.mip_pages = by_level.len() as u32;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("resident frame"),
        });
        if by_level.is_empty() {
            return Ok(encoder);
        }
        by_level.sort_by_key(|(l, _)| *l);
        let align =
            u64::from(device.limits().min_storage_buffer_offset_alignment).max(256) as usize;
        let mut data: Vec<u8> = Vec::new();
        let mut groups: Vec<(u64, u32)> = Vec::new();
        let mut i = 0;
        while i < by_level.len() {
            let l = by_level[i].0;
            let start = data.len();
            let mut n = 0u32;
            while i < by_level.len() && by_level[i].0 == l && n < 65535 {
                data.extend_from_slice(bytemuck::bytes_of(&by_level[i].1));
                i += 1;
                n += 1;
            }
            groups.push((start as u64, n));
            data.resize(data.len().next_multiple_of(align), 0);
        }
        self.jobs.write(&device, &queue, &data);
        let pb = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("resident pool"),
            contents: bytemuck::bytes_of(&self.pool_uniform()),
            usage: wgpu::BufferUsages::UNIFORM,
        });
        let jobs = self.jobs.buffer.as_ref().expect("written");
        let layout = self.pipes.mip.get_bind_group_layout(0);
        for (offset, n) in groups {
            let mut entries = self.slab_entries();
            entries.push(wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: jobs,
                    offset,
                    size: std::num::NonZeroU64::new(
                        u64::from(n) * std::mem::size_of::<Job>() as u64,
                    ),
                }),
            });
            entries.push(wgpu::BindGroupEntry {
                binding: 9,
                resource: pb.as_entire_binding(),
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("resident mip"),
                layout: &layout,
                entries: &entries,
            });
            // Separate passes order the levels.
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("resident mips"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.pipes.mip);
            pass.set_bind_group(0, &group, &[]);
            // 65536 invocations: one per texel (RGBA) or per word (masks).
            pass.dispatch_workgroups(256, n, 1);
        }
        Ok(encoder)
    }

    #[allow(clippy::too_many_arguments)]
    fn damage(
        &self,
        doc: &Document,
        level: u8,
        program: &[u8],
        nodes: &[u64],
        grid: usize,
        cols: u32,
        le: Extent,
    ) -> Option<Vec<Rect>> {
        let last = self.levels.get(&level)?.last.as_ref()?;
        let state = doc.state();
        let logged = (last.key == doc.key() && last.epoch == doc.epoch() && last.rev <= state.rev)
            .then(|| doc.damage_between(last.rev, state.rev, Rect::of_extent(state.canvas)))
            .flatten()
            .map(|d| d.to_level(level));
        if last.program != program || last.nodes.len() != nodes.len() {
            return logged.map(|d| vec![d]);
        }
        let mut changed = vec![false; grid];
        for (i, (a, b)) in nodes.iter().zip(&last.nodes).enumerate() {
            if a != b {
                changed[i % grid] = true;
            }
        }
        let rects = changed
            .iter()
            .enumerate()
            .filter(|(_, c)| **c)
            .map(|(t, _)| {
                let r = Rect::of_tile(TileCoord::new(level, t as u32 % cols, t as u32 / cols), le);
                match logged {
                    Some(d) => r.intersect(&d),
                    None => r,
                }
            })
            .filter(|r| !r.is_empty())
            .collect();
        Some(rects)
    }

    /// Frees a level's resident composite (for example a zoom level no
    /// longer on screen). Pages stay resident.
    pub fn drop_level(&mut self, level: u8) {
        self.levels.remove(&level);
    }

    /// Forgets what every level buffer was rendered from, so the next frame
    /// of each level recomposites it in full (pages stay resident).
    pub fn invalidate(&mut self) {
        for l in self.levels.values_mut() {
            l.last = None;
        }
    }

    /// Blocks until all submitted GPU work has completed.
    pub fn wait(&self) -> EngineResult<()> {
        self.device
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(internal)?;
        Ok(())
    }

    fn level(&self, level: u8) -> EngineResult<&LevelState> {
        self.levels
            .get(&level)
            .filter(|l| l.last.is_some())
            .ok_or_else(|| EngineError::invalid("level", format!("level {level} not rendered")))
    }

    /// Writes `src` (level coordinates) of a rendered level into an RGBA8
    /// storage texture at `dst`: flattened over `background` (opaque), or
    /// premultiplied with alpha when `background` is `None`. Colours are
    /// the document's own encoding. No readback.
    pub fn present(
        &self,
        level: u8,
        target: &wgpu::Texture,
        src: Rect,
        dst: (u32, u32),
        background: Option<[f32; 3]>,
    ) -> EngineResult<()> {
        let st = self.level(level)?;
        let e = st.extent;
        if src.is_empty() || !Rect::of_extent(e).intersect(&src).eq(&src) {
            return Err(EngineError::invalid("src", "outside the level"));
        }
        let (w, h) = (src.width() as u32, src.height() as u32);
        if target.format() != wgpu::TextureFormat::Rgba8Unorm
            || !target
                .usage()
                .contains(wgpu::TextureUsages::STORAGE_BINDING)
        {
            return Err(EngineError::Unsupported {
                what: "presentation target must be an Rgba8Unorm storage texture".into(),
            });
        }
        if dst.0.checked_add(w).is_none_or(|v| v > target.width())
            || dst.1.checked_add(h).is_none_or(|v| v > target.height())
        {
            return Err(EngineError::invalid("dst", "outside the target"));
        }
        let bg = background.unwrap_or([0.0; 3]);
        let u = PresentUniform {
            lw: e.width,
            sx: src.x0 as u32,
            sy: src.y0 as u32,
            dx: dst.0,
            dy: dst.1,
            w,
            h,
            flatten: u32::from(background.is_some()),
            bg: [bg[0], bg[1], bg[2], 1.0],
        };
        let ub = self
            .device
            .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("resident present"),
                contents: bytemuck::bytes_of(&u),
                usage: wgpu::BufferUsages::UNIFORM,
            });
        let view = target.create_view(&Default::default());
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("resident present"),
            layout: &self.pipes.present.get_bind_group_layout(0),
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: st.out.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: ub.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
            ],
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = enc.begin_compute_pass(&Default::default());
            pass.set_pipeline(&self.pipes.present);
            pass.set_bind_group(0, &group, &[]);
            pass.dispatch_workgroups(w.div_ceil(16), h.div_ceil(16), 1);
        }
        self.queue.submit([enc.finish()]);
        Ok(())
    }

    /// [`present`](Self::present) into a retained RGBA8 IOSurface (by id),
    /// imported on this renderer's device.
    pub fn present_iosurface(
        &self,
        level: u8,
        surface: u32,
        src: Rect,
        dst: (u32, u32),
        background: Option<[f32; 3]>,
    ) -> EngineResult<()> {
        let (texture, format) = gpu_core::write_to_iosurface(&self.device, surface)?;
        if format != gpu_core::SurfaceFormat::Rgba8 {
            return Err(EngineError::Unsupported {
                what: "the compositor presents to RGBA8 IOSurfaces only".into(),
            });
        }
        self.present(level, &texture, src, dst, background)
    }

    /// Explicit readback of a rendered level as interleaved f32 RGBA,
    /// premultiplied or straight.
    pub fn read_level(&self, level: u8, premultiplied: bool) -> EngineResult<(Extent, Vec<f32>)> {
        let st = self.level(level)?;
        let bytes = gpu_core::read_buffer(&self.device, &self.queue, &st.out, 0, st.out.size())?;
        let mut v: Vec<f32> = bytemuck::cast_slice(&bytes).to_vec();
        if !premultiplied {
            for p in v.as_chunks_mut::<4>().0 {
                let c = crate::render::pixel::unpremul([p[0], p[1], p[2], p[3]]);
                p[..3].copy_from_slice(&c);
            }
        }
        Ok((st.extent, v))
    }

    /// Explicit readback of a rendered level as premultiplied planar f32
    /// tiles in raster order ([`Tile::premultiplied`] set), the layout of
    /// [`Compositor::render_tile_premultiplied`].
    pub fn read_tiles(&self, level: u8) -> EngineResult<Vec<Tile>> {
        let (e, v) = self.read_level(level, true)?;
        let (cols, rows) = e.tile_grid(TILE_SIZE);
        let mut out = Vec::with_capacity((cols * rows) as usize);
        for ty in 0..rows {
            for tx in 0..cols {
                let coord = TileCoord::new(level, tx, ty);
                let r = Rect::of_tile(coord, e);
                let (w, h) = (r.width() as usize, r.height() as usize);
                let n = w * h;
                let mut s = vec![0.0f32; 4 * n];
                for y in 0..h {
                    for x in 0..w {
                        let o = ((r.y0 as usize + y) * e.width as usize + r.x0 as usize + x) * 4;
                        for c in 0..4 {
                            s[c * n + y * w + x] = v[o + c];
                        }
                    }
                }
                let layout = TileLayout {
                    extent: Extent::new(w as u32, h as u32),
                    halo: 0,
                    channels: 4,
                };
                out.push(Tile::from_samples(coord, layout, s)?.with_premultiplied(true)?);
            }
        }
        Ok(out)
    }
}

/// Splits a 1-D workgroup count into a 2-D grid within per-dimension limits.
fn split(n: u32) -> (u32, u32) {
    let x = n.clamp(1, 32768);
    (x, n.div_ceil(x).max(1))
}
