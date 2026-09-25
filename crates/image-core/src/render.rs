//! The tiled, memoizing, progressive renderer.
//!
//! # Frames
//!
//! Stages up to and including `WhiteBalance` run on level-0 tiles of the
//! full sensor plane ([`crate::graph::Frame::Sensor`]); CFA phase is indexed
//! there and neighbourhood operators gather their halo across tile seams
//! with the same edge rule as `pipeline_cpu::Image::tile`. The
//! `WhiteBalance` output is then cropped to the active area
//! (`RawMetadata::default_crop`) and box-averaged in linear light into the
//! output pyramid ([`crate::graph::Frame::Output`]): level `L` pixel
//! `(x, y)` is the mean of the crop's `2^L`-square block at `(x·2^L,
//! y·2^L)`, partial edge blocks averaging only real samples. The CFA itself
//! is never decimated. Tone and Output run on output-pyramid tiles.
//!
//! # Memoization
//!
//! Keys are [`engine_api::stage::MemoKey`]s with the chained parameter hash
//! of the stage (seeded by the process version), so a change invalidates
//! exactly the stages at and after the earliest dirty stage. Two outputs are
//! stored, as `F16Planar`, in the shared byte-budgeted LRU
//! [`TileCache`]: Demosaic (sensor frame, level 0) and the resampled
//! WhiteBalance buffer (output frame, per level). A tone-only change
//! therefore runs Tone and Output only; a white-balance change reruns the two
//! colour matrices from the cached demosaic. Everything in flight is `F32`.
//!
//! Within one request, tiles shared by several consumers (linearized CFA
//! tiles read by neighbouring demosaic halos, sensor tiles straddling two
//! output tiles) are kept at `F32` in a reference-counted request scratch,
//! so a request never mixes a freshly computed tile with its own f16-rounded
//! copy. Consequently, on a cold cache the output is bit-identical to
//! `pipeline_cpu::render_scaled(settings, source, 2^L)` whenever Tone is the
//! identity (default tone settings) and at level 0 for any settings. At
//! levels above 0 with non-neutral tone, Tone runs on the downsampled buffer
//! (the interactive preview path) rather than before downsampling as the
//! reference does. Renders served from the f16 cache differ from a cold
//! render by f16 rounding (relative 2^-11 in scene-linear values).
//!
//! # M2 controls
//!
//! Changed detail, extended tone, colour, geometry or effects settings use a
//! whole active-area WB image assembled through the same memoized M1 path.
//! Detail gathers its halo from the immutable preceding image; extended tone
//! uses an image-level barrier for global dehaze statistics. Geometry follows
//! colour, and effects use post-crop coordinates. All operators dispatch through
//! `StageOp`, including the overridable image-level barriers.
//!
//! At levels above zero, M2 operates on requested-level WB, not full-resolution
//! RGB. Like preview tone, this is intentionally an interactive approximation:
//! neighbourhood radii/grain are preview pixels and crop dimensions are rounded
//! from the preview extent. Level zero is the full-resolution reference path.
//! M2 intermediates are request-local, never inserted into the upstream cache;
//! changing any M2 control therefore reuses WB without stale developed pixels.
//!
//! # Parallelism and cancellation
//!
//! Output tiles are processed in chunks bounded by the number of new sensor
//! tiles they need; each step inside a chunk runs across
//! [`RendererConfig::threads`] scoped threads on CPU. Backends advertising
//! batching receive contiguous operator chains and own their submission size.
//! Halo gathering, memo storage and resampling end a chain. The cancellation token is
//! polled before every tile of every step, and before each delivered tile.

#[path = "resident_render.rs"]
mod resident_render;

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use engine_api::color::{ColorMatrix3, WorkingSpace};
use engine_api::jobs::{CancellationToken, Job, JobContext, Priority};
use engine_api::recipe::settings::{DemosaicMethod, HighlightReconstruction};
use engine_api::recipe::{DevelopSettings, ProcessVersion};
use engine_api::stage::{ParamHash, StageId};
use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord};
use engine_api::{EngineError, EngineResult};
use pipeline_cpu::DemosaicAlgorithm;
use raw_decode::CfaLayout;

use crate::cache::{TileCache, to_f16, to_f32};
use crate::graph::PipelineGraph;
use crate::ops::{CpuStageOp, Op, StageOp};
use crate::resample::{Crop, gather, gather_sources, resample, resample_sources};
use crate::source::RawImage;

/// Upper bound on sensor tiles one chunk newly materialises (~0.8 MB each
/// at F32 RGB), which bounds request scratch memory.
const CHUNK_SENSOR_TILES: usize = 64;

/// Highest supported output level (`2^12` = 4096× reduction).
pub const MAX_LEVEL: u8 = 12;

/// A rectangle in pixels of one output-pyramid level.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelRect {
    /// Left edge.
    pub x: u32,
    /// Top edge.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl PixelRect {
    /// Creates a rectangle.
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// The whole of an extent.
    pub const fn full(extent: Extent) -> Self {
        Self::new(0, 0, extent.width, extent.height)
    }

    /// The smallest rectangle at `level` covering this level-0 rectangle.
    pub fn at_level(self, level: u8) -> Self {
        let s = 1u64 << level;
        let x0 = u64::from(self.x) / s;
        let y0 = u64::from(self.y) / s;
        let x1 = (u64::from(self.x) + u64::from(self.width)).div_ceil(s);
        let y1 = (u64::from(self.y) + u64::from(self.height)).div_ceil(s);
        Self::new(x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
    }
}

/// What the renderer delivers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderOutput {
    /// Display-encoded 8-bit sRGB tiles (`U8`, three planes): the Output stage.
    #[default]
    Display,
    /// Scene-linear Rec.2020 `F32` tiles after Tone, before Output.
    SceneLinear,
}

/// The on-screen region for [`Renderer::render_progressive`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    /// Visible area in level-0 active-area pixels.
    pub rect: PixelRect,
    /// Finest level to refine to (0 = 1:1).
    pub finest_level: u8,
    /// Level of the first, coarse pass.
    pub coarsest_level: u8,
}

impl Viewport {
    /// Level 3 → 2 → 1 → 0 over `rect` (level-0 pixels).
    pub const fn new(rect: PixelRect) -> Self {
        Self {
            rect,
            finest_level: 0,
            coarsest_level: 3,
        }
    }
}

/// Renderer configuration.
#[derive(Debug, Clone)]
pub struct RendererConfig {
    /// Payload budget of the memo cache created by [`Renderer::new`].
    pub cache_budget_bytes: usize,
    /// Worker threads per request (1 renders on the calling thread).
    pub threads: usize,
    /// Seeds every memo key; recipes of another process never share tiles.
    pub process_version: ProcessVersion,
    /// Stage graph and memoization flags.
    pub graph: PipelineGraph,
}

impl Default for RendererConfig {
    fn default() -> Self {
        Self {
            cache_budget_bytes: 512 << 20,
            threads: std::thread::available_parallelism().map_or(1, |n| n.get()),
            process_version: ProcessVersion::NATIVE_CURRENT,
            graph: PipelineGraph::m2(),
        }
    }
}

/// Pulls output tiles through the stage graph with memoized upstream tiles.
/// `Send + Sync`: share one renderer (and its cache) between jobs.
pub struct Renderer {
    ops: Arc<dyn StageOp>,
    cache: Arc<TileCache>,
    config: RendererConfig,
}

/// Per-request parameters resolved once from the settings and metadata.
struct Resolved<'a> {
    image: &'a RawImage,
    settings: &'a DevelopSettings,
    chain: [(StageId, ParamHash); StageId::COUNT],
    sensor: Extent,
    crop: Crop,
    cfa: CfaLayout,
    period: u32,
    lin_halo: u16,
    dem_halo: u16,
    highlights: HighlightReconstruction,
    algorithm: DemosaicAlgorithm,
    profile: ColorMatrix3,
    wb: ColorMatrix3,
}

impl Renderer {
    /// A renderer on the CPU reference operators with its own cache.
    pub fn new(config: RendererConfig) -> Self {
        let cache = Arc::new(TileCache::new(config.cache_budget_bytes));
        Self::with_ops(Arc::new(CpuStageOp), cache, config)
    }

    /// A renderer on any backend and (possibly shared) cache. The cache's own
    /// budget applies; `config.cache_budget_bytes` is ignored.
    pub fn with_ops(ops: Arc<dyn StageOp>, cache: Arc<TileCache>, config: RendererConfig) -> Self {
        Self { ops, cache, config }
    }

    /// The memo cache.
    pub fn cache(&self) -> &Arc<TileCache> {
        &self.cache
    }

    /// Configuration.
    pub fn config(&self) -> &RendererConfig {
        &self.config
    }

    /// Output tiles at `level` intersecting `rect` (pixels of that level),
    /// in raster order.
    pub fn tiles_for(image: &RawImage, level: u8, rect: PixelRect) -> Vec<TileCoord> {
        if level > MAX_LEVEL {
            return Vec::new();
        }
        Self::tiles_in_extent(image.level_extent(level), level, rect)
    }

    fn tiles_in_extent(e: Extent, level: u8, rect: PixelRect) -> Vec<TileCoord> {
        let x1 = (u64::from(rect.x) + u64::from(rect.width)).min(u64::from(e.width)) as u32;
        let y1 = (u64::from(rect.y) + u64::from(rect.height)).min(u64::from(e.height)) as u32;
        if rect.x >= x1 || rect.y >= y1 {
            return Vec::new();
        }
        let (c0, c1) = (rect.x / TILE_SIZE, (x1 - 1) / TILE_SIZE);
        let (r0, r1) = (rect.y / TILE_SIZE, (y1 - 1) / TILE_SIZE);
        (r0..=r1)
            .flat_map(|y| (c0..=c1).map(move |x| TileCoord::new(level, x, y)))
            .collect()
    }

    /// Renders the display tiles of `level` intersecting `rect` (pixels of
    /// that level), in raster order.
    pub fn render_region(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: PixelRect,
    ) -> EngineResult<Vec<Tile>> {
        self.render_region_as(image, settings, level, rect, RenderOutput::Display)
    }

    /// [`Renderer::render_region`] with a choice of output.
    pub fn render_region_as(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
        rect: PixelRect,
        output: RenderOutput,
    ) -> EngineResult<Vec<Tile>> {
        pipeline_cpu::validate_settings(settings)?;
        let extent = Self::output_extent(image, settings, level)?;
        let coords = Self::tiles_in_extent(extent, level, rect);
        let mut out = Vec::with_capacity(coords.len());
        self.render_tiles(
            image,
            settings,
            &coords,
            output,
            &CancellationToken::new(),
            &mut |t| out.push(t),
        )?;
        Ok(out)
    }

    /// Renders specific output tiles (all of one level), delivering each to
    /// `sink` in the given order. Duplicates are rendered once.
    pub fn render_tiles(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        sink: &mut dyn FnMut(Tile),
    ) -> EngineResult<()> {
        cancel.check()?;
        let r = self.resolve(image, settings)?;
        if has_m2_settings(settings) {
            self.run_m2(image, settings, coords, output, cancel, sink)
        } else {
            self.run(&r, coords, output, cancel, sink)
        }
    }

    /// Renders the viewport coarse to fine (by default level 3, 2, 1, 0),
    /// delivering every tile of a level before any tile of the next finer
    /// one. Returns `Err(Cancelled)` as soon as `cancel` fires, between tiles.
    pub fn render_progressive(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        viewport: &Viewport,
        output: RenderOutput,
        cancel: &CancellationToken,
        sink: &mut dyn FnMut(Tile),
    ) -> EngineResult<()> {
        if viewport.finest_level > viewport.coarsest_level || viewport.coarsest_level > MAX_LEVEL {
            return Err(EngineError::invalid(
                "viewport",
                format!("need finest <= coarsest <= {MAX_LEVEL}"),
            ));
        }
        let r = self.resolve(image, settings)?;
        for level in (viewport.finest_level..=viewport.coarsest_level).rev() {
            let extent = Self::output_extent(image, settings, level)?;
            let coords = Self::tiles_in_extent(extent, level, viewport.rect.at_level(level));
            if has_m2_settings(settings) {
                self.run_m2(image, settings, &coords, output, cancel, sink)?;
            } else {
                self.run(&r, &coords, output, cancel, sink)?;
            }
        }
        Ok(())
    }

    /// Cropped output extent at a preview level. M2 previews apply geometry
    /// to the requested-level WB buffer, just as preview tone runs after resize.
    pub fn output_extent(
        image: &RawImage,
        settings: &DevelopSettings,
        level: u8,
    ) -> EngineResult<Extent> {
        if level > MAX_LEVEL {
            return Err(EngineError::invalid("level", "level exceeds MAX_LEVEL"));
        }
        let e = image.level_extent(level);
        let r = settings.geometry.crop.rect;
        if !r.is_valid() {
            return Err(EngineError::invalid("crop", "invalid rectangle"));
        }
        Ok(Extent::new(
            ((r.right - r.left) * e.width as f32).round().max(1.) as u32,
            ((r.bottom - r.top) * e.height as f32).round().max(1.) as u32,
        ))
    }

    fn run_m2(
        &self,
        image: &RawImage,
        settings: &DevelopSettings,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        sink: &mut dyn FnMut(Tile),
    ) -> EngineResult<()> {
        cancel.check()?;
        let Some(first) = coords.first() else {
            return Ok(());
        };
        let level = first.level;
        let extent = Self::output_extent(image, settings, level)?;
        let grid = extent.tile_grid(TILE_SIZE);
        if coords
            .iter()
            .any(|c| c.level != level || c.x >= grid.0 || c.y >= grid.1)
        {
            return Err(EngineError::invalid(
                "tiles",
                "coordinates must share one level and lie inside cropped output",
            ));
        }
        // Reuse the existing memoized sensor/WB path. At L0 this is the full
        // active area; previews do expensive M2 work only at preview resolution.
        let mut base = settings.clone();
        base.detail = Default::default();
        base.tone = Default::default();
        base.color = Default::default();
        base.effects = Default::default();
        base.geometry = Default::default();
        let r = self.resolve(image, &base)?;
        let e = image.level_extent(level);
        let all = Self::tiles_for(image, level, PixelRect::full(e));
        let mut wb =
            pipeline_cpu::Image::new(e.width, e.height, vec![vec![0.; e.area() as usize]; 3])?;
        let mut error = None;
        self.run(&r, &all, RenderOutput::SceneLinear, cancel, &mut |t| {
            let result = Tile::from_samples(
                TileCoord::new(0, t.coord().x, t.coord().y),
                t.layout(),
                t.samples::<f32>().expect("scene-linear tile").to_vec(),
            )
            .and_then(|t| wb.put(&t));
            if let Err(e) = result {
                error = Some(e);
            }
        })?;
        if let Some(e) = error {
            return Err(e);
        }
        let mut developed = wb;
        for (stage, op) in [
            (StageId::Detail, Op::Detail(&settings.detail)),
            (StageId::Tone, Op::Tone(&settings.tone)),
            (StageId::Tone, Op::ToneExtra(&settings.tone)),
            (StageId::Color, Op::Color(&settings.color)),
            (
                StageId::Effects,
                Op::EffectsInCrop(&settings.effects, e, &settings.geometry.crop),
            ),
            (StageId::Geometry, Op::Geometry(&settings.geometry)),
        ] {
            developed = self.ops.run_image(stage, &op, developed, cancel)?;
        }
        let mut seen = HashSet::new();
        for &coord in coords {
            cancel.check()?;
            if !seen.insert(coord) {
                continue;
            }
            let mut t = developed.tile(TileCoord::new(0, coord.x, coord.y), 0, 1)?;
            cancel.check()?;
            if output == RenderOutput::Display {
                t = self.ops.run(
                    StageId::Output,
                    &Op::Display {
                        gamut: settings.output.gamut_mapping,
                    },
                    t,
                )?;
                t = Tile::from_samples(coord, t.layout(), t.samples::<u8>()?.to_vec())?;
            } else {
                t = Tile::from_samples(coord, t.layout(), t.samples::<f32>()?.to_vec())?;
            }
            cancel.check()?;
            sink(t);
        }
        cancel.check()
    }

    fn resolve<'a>(
        &self,
        image: &'a RawImage,
        settings: &'a DevelopSettings,
    ) -> EngineResult<Resolved<'a>> {
        pipeline_cpu::validate_settings(settings)?;
        let m = image.metadata();
        let (period, dem_halo) = match m.cfa_layout {
            CfaLayout::Bayer(_) => (2, 2),
            CfaLayout::XTrans(_) => (6, 3),
            CfaLayout::Unsupported => {
                return Err(EngineError::invalid("CFA", "unsupported pattern"));
            }
        };
        if m.width < period || m.height < period {
            return Err(EngineError::invalid(
                "CFA",
                "image must contain a complete CFA period",
            ));
        }
        let camera_xyz = pipeline_cpu::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
            m.cam_xyz[r].map(f64::from)
        })))?;
        let profile = WorkingSpace::LinearRec2020.to_xyz().inverse()? * camera_xyz;
        let wb =
            pipeline_cpu::white_balance_matrix(&settings.white_balance, camera_xyz, m.as_shot_wb)?;
        let algorithm = match settings.demosaic.method {
            DemosaicMethod::Auto => DemosaicAlgorithm::MalvarHeCutler,
            DemosaicMethod::Bilinear => DemosaicAlgorithm::Bilinear,
            _ => {
                return Err(EngineError::invalid(
                    "demosaic",
                    "only Auto (MHC) and Bilinear implemented",
                ));
            }
        };
        let highlights = settings.linearize.highlight_reconstruction;
        Ok(Resolved {
            image,
            settings,
            chain: settings.stage_chain(self.config.process_version.chain_seed()),
            sensor: image.sensor_extent(),
            crop: m.default_crop,
            cfa: m.cfa_layout,
            period,
            lin_halo: if highlights == HighlightReconstruction::Clip {
                0
            } else {
                4
            },
            dem_halo,
            highlights,
            algorithm,
            profile,
            wb,
        })
    }

    fn run(
        &self,
        r: &Resolved<'_>,
        coords: &[TileCoord],
        output: RenderOutput,
        cancel: &CancellationToken,
        sink: &mut dyn FnMut(Tile),
    ) -> EngineResult<()> {
        let mut seen = HashSet::new();
        let coords: Vec<TileCoord> = coords.iter().copied().filter(|c| seen.insert(*c)).collect();
        let Some(first) = coords.first() else {
            return Ok(());
        };
        let level = first.level;
        let grid = r.image.level_extent(level).tile_grid(TILE_SIZE);
        if level > MAX_LEVEL
            || coords
                .iter()
                .any(|c| c.level != level || c.x >= grid.0 || c.y >= grid.1)
        {
            return Err(EngineError::invalid(
                "tiles",
                "coordinates must share one level and lie inside the output pyramid",
            ));
        }

        if matches!(r.cfa, CfaLayout::Bayer(_))
            && let Some(batch) = self.ops.begin_resident()
        {
            for tile in self
                .run_resident(r, &coords, output, cancel, batch, None)?
                .tiles
            {
                cancel.check()?;
                sink(tile);
            }
            return Ok(());
        }

        let graph = &self.config.graph;
        let cache_dem = graph.node(StageId::Demosaic).cacheable;
        let cache_wb = graph.node(StageId::WhiteBalance).cacheable;
        let id = r.image.id();
        let key = |stage, c| PipelineGraph::memo_key(id, &r.chain, stage, c);
        let threads = self.config.threads.max(1);

        // Plan reference counts so request-shared F32 tiles live exactly as
        // long as a planned consumer still needs them.
        let sources: Vec<Vec<TileCoord>> = coords
            .iter()
            .map(|&c| resample_sources(r.crop, c))
            .collect();
        let planned_miss: Vec<bool> = coords
            .iter()
            .map(|&c| !(cache_wb && self.cache.contains(&key(StageId::WhiteBalance, c))))
            .collect();
        let mut wb_uses: HashMap<TileCoord, usize> = HashMap::new();
        for (s, _) in sources.iter().zip(&planned_miss).filter(|(_, m)| **m) {
            for t in s {
                *wb_uses.entry(*t).or_default() += 1;
            }
        }
        let mut planned_dem: HashSet<TileCoord> = wb_uses
            .keys()
            .copied()
            .filter(|&d| !(cache_dem && self.cache.contains(&key(StageId::Demosaic, d))))
            .collect();
        let mut lin_uses: HashMap<TileCoord, usize> = HashMap::new();
        for &d in &planned_dem {
            for l in gather_sources(r.sensor, d, r.dem_halo, r.period) {
                *lin_uses.entry(l).or_default() += 1;
            }
        }
        let mut wb_scratch: HashMap<TileCoord, Tile> = HashMap::new();
        let mut lin_scratch: HashMap<TileCoord, Tile> = HashMap::new();

        let mut start = 0;
        while start < coords.len() {
            cancel.check()?;
            let mut end = start;
            let mut pending: HashSet<TileCoord> = HashSet::new();
            while end < coords.len() {
                let fresh: Vec<TileCoord> = if planned_miss[end] {
                    sources[end]
                        .iter()
                        .filter(|s| !wb_scratch.contains_key(s) && !pending.contains(s))
                        .copied()
                        .collect()
                } else {
                    Vec::new()
                };
                if end > start && pending.len() + fresh.len() > CHUNK_SENSOR_TILES {
                    break;
                }
                pending.extend(fresh);
                end += 1;
            }
            let chunk = &coords[start..end];

            // A. Resampled WhiteBalance buffers from the memo cache (the
            // f16 → f32 widening is the bulk of a tone-only edit's input cost,
            // so it runs across the worker threads).
            let hits: Vec<Option<Tile>> = chunk
                .iter()
                .map(|&c| {
                    cache_wb
                        .then(|| self.cache.get(&key(StageId::WhiteBalance, c)))
                        .flatten()
                })
                .collect();
            let mut pre_tone: Vec<Option<Tile>> = par_map(threads, cancel, &hits, |hit| {
                hit.as_ref().map(to_f32).transpose()
            })?;
            let missing: Vec<usize> = (0..chunk.len())
                .filter(|&i| pre_tone[i].is_none())
                .collect();

            // B. Sensor tiles those buffers need, and which must be demosaiced.
            let need_s: Vec<TileCoord> = missing
                .iter()
                .flat_map(|&i| sources[start + i].iter().copied())
                .filter(|s| !wb_scratch.contains_key(s))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            let dem_cached: Vec<Mutex<Option<Tile>>> = need_s
                .iter()
                .map(|&s| {
                    let hit = cache_dem
                        .then(|| self.cache.get(&key(StageId::Demosaic, s)))
                        .flatten();
                    hit.map(|t| to_f32(&t)).transpose().map(Mutex::new)
                })
                .collect::<EngineResult<_>>()?;
            let to_dem: Vec<TileCoord> = need_s
                .iter()
                .zip(&dem_cached)
                .filter(|(_, t)| t.lock().unwrap().is_none())
                .map(|(s, _)| *s)
                .collect();

            // C. Decode → Linearize for every CFA tile the demosaic halos read.
            let need_l: Vec<TileCoord> = to_dem
                .iter()
                .flat_map(|&d| gather_sources(r.sensor, d, r.dem_halo, r.period))
                .filter(|l| !lin_scratch.contains_key(l))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect();
            if !need_l.is_empty() {
                let need_dec: Vec<TileCoord> = need_l
                    .iter()
                    .flat_map(|&l| gather_sources(r.sensor, l, r.lin_halo, r.period))
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();
                let pyramid = r.image.cfa().pyramid();
                let decoded: HashMap<TileCoord, Tile> = need_dec
                    .iter()
                    .copied()
                    .zip(par_map(threads, cancel, &need_dec, |&c| {
                        engine_api::tile::Pyramid::tile(pyramid, c)
                    })?)
                    .collect();
                let op = Op::Highlights {
                    cfa: r.cfa,
                    mode: r.highlights,
                };
                let inputs = par_map(threads, cancel, &need_l, |&l| {
                    gather(r.sensor, l, r.lin_halo, r.period, &decoded)
                })?;
                let lin = self.run_ops(&[(StageId::Linearize, op)], inputs, cancel)?;
                lin_scratch.extend(need_l.iter().copied().zip(lin));
            }

            // D. Demosaic (memoized) → CameraProfile → WhiteBalance.
            let dem_op = Op::Demosaic {
                cfa: r.cfa,
                algorithm: r.algorithm,
            };
            let dem_inputs = par_map(threads, cancel, &to_dem, |&s| {
                gather(r.sensor, s, r.dem_halo, r.period, &lin_scratch)
            })?;
            let matrices = [
                (StageId::CameraProfile, Op::Matrix(r.profile)),
                (StageId::WhiteBalance, Op::Matrix(r.wb)),
            ];
            let balanced = if cache_dem {
                // A host memoization boundary ends the first GPU chain.
                let dem = self.run_ops(&[(StageId::Demosaic, dem_op)], dem_inputs, cancel)?;
                let mut fresh: HashMap<_, _> = to_dem.iter().copied().zip(dem).collect();
                for (&s, t) in &fresh {
                    self.cache.insert(key(StageId::Demosaic, s), to_f16(t)?);
                }
                let rgb = need_s
                    .iter()
                    .enumerate()
                    .map(|(j, s)| {
                        dem_cached[j]
                            .lock()
                            .unwrap()
                            .take()
                            .unwrap_or_else(|| fresh.remove(s).expect("demosaic resolved"))
                    })
                    .collect();
                self.run_ops(&matrices, rgb, cancel)?
            } else {
                // Without a memo boundary all three passes stay resident.
                self.run_ops(
                    &[(StageId::Demosaic, dem_op), matrices[0], matrices[1]],
                    dem_inputs,
                    cancel,
                )?
            };
            wb_scratch.extend(need_s.iter().copied().zip(balanced));
            for d in &to_dem {
                if planned_dem.remove(d) {
                    for l in gather_sources(r.sensor, *d, r.dem_halo, r.period) {
                        release(&mut lin_uses, l);
                    }
                }
            }
            lin_scratch.retain(|c, _| lin_uses.contains_key(c));

            // E. Crop + linear-light downsample into the output pyramid.
            let resampled = par_map(threads, cancel, &missing, |&i| {
                resample(r.crop, chunk[i], &wb_scratch)
            })?;
            for (&i, t) in missing.iter().zip(resampled) {
                if cache_wb {
                    self.cache
                        .insert(key(StageId::WhiteBalance, chunk[i]), to_f16(&t)?);
                }
                pre_tone[i] = Some(t);
            }
            for i in start..end {
                if planned_miss[i] {
                    for s in &sources[i] {
                        release(&mut wb_uses, *s);
                    }
                }
            }
            wb_scratch.retain(|c, _| wb_uses.contains_key(c));

            // F. Tone → Output.
            let tone = Op::Tone(&r.settings.tone);
            let display = Op::Display {
                gamut: r.settings.output.gamut_mapping,
            };
            let mut chain = vec![(StageId::Tone, tone)];
            if output == RenderOutput::Display {
                chain.push((StageId::Output, display));
            }
            let inputs = pre_tone
                .into_iter()
                .map(|t| t.expect("every chunk tile resolved"))
                .collect();
            let finished = self.run_ops(&chain, inputs, cancel)?;
            for t in finished {
                cancel.check()?;
                sink(t);
            }
            start = end;
        }
        Ok(())
    }

    fn run_ops(
        &self,
        chain: &[(StageId, Op<'_>)],
        inputs: Vec<Tile>,
        cancel: &CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        if self.ops.batch_size() > 1 {
            self.ops.run_chain_batch(chain, inputs, cancel)
        } else {
            let inputs: Vec<_> = inputs.into_iter().map(|t| Mutex::new(Some(t))).collect();
            let groups = par_map(self.config.threads, cancel, &inputs, |t| {
                let t = t.lock().unwrap().take().expect("tile processed once");
                self.ops.run_chain_batch(chain, vec![t], cancel)
            })?;
            Ok(groups.into_iter().flatten().collect())
        }
    }
}

// Kept private so integration does not depend on the reference renderer helper.
fn has_m2_settings(s: &DevelopSettings) -> bool {
    s.detail != Default::default()
        || s.color != Default::default()
        || s.effects != Default::default()
        || s.geometry != Default::default()
        || s.tone.texture != 0.
        || s.tone.clarity != 0.
        || s.tone.dehaze != 0.
        || s.tone.curves != Default::default()
}

fn release(uses: &mut HashMap<TileCoord, usize>, c: TileCoord) {
    if let Some(n) = uses.get_mut(&c) {
        *n -= 1;
        if *n == 0 {
            uses.remove(&c);
        }
    }
}

/// Maps `f` over `items` on up to `threads` scoped threads, preserving
/// order. Polls `cancel` before every item; the first error (in item order)
/// is returned and stops further items from starting.
fn par_map<T: Sync, R: Send>(
    threads: usize,
    cancel: &CancellationToken,
    items: &[T],
    f: impl Fn(&T) -> EngineResult<R> + Sync,
) -> EngineResult<Vec<R>> {
    let workers = threads.min(items.len());
    if workers <= 1 {
        return items
            .iter()
            .map(|t| {
                cancel.check()?;
                f(t)
            })
            .collect();
    }
    let next = AtomicUsize::new(0);
    let failed = AtomicBool::new(false);
    let slots: Vec<Mutex<Option<EngineResult<R>>>> =
        items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    if i >= items.len() || failed.load(Ordering::Relaxed) {
                        break;
                    }
                    let result = cancel.check().and_then(|()| f(&items[i]));
                    if result.is_err() {
                        failed.store(true, Ordering::Relaxed);
                    }
                    *slots[i].lock().unwrap() = Some(result);
                }
            });
        }
    });
    let mut out = Vec::with_capacity(items.len());
    for slot in slots {
        match slot.into_inner().unwrap() {
            Some(Ok(v)) => out.push(v),
            Some(Err(e)) => return Err(e),
            // Skipped after another item failed; that error comes later.
            None => {}
        }
    }
    if out.len() == items.len() {
        Ok(out)
    } else {
        Err(cancel
            .check()
            .err()
            .unwrap_or_else(|| EngineError::internal("parallel map lost results")))
    }
}

/// A [`Job`] that renders a viewport progressively and hands each tile to a
/// sink (for example a channel to the UI compositor). Cancelling the job's
/// token stops it between tiles.
pub struct ProgressiveRenderJob {
    /// Shared renderer (and cache).
    pub renderer: Arc<Renderer>,
    /// Source image.
    pub image: RawImage,
    /// Settings to render.
    pub settings: DevelopSettings,
    /// Region and levels.
    pub viewport: Viewport,
    /// Display or scene-linear tiles.
    pub output: RenderOutput,
    /// Scheduler class, normally [`Priority::Viewport`].
    pub priority: Priority,
    /// Receives each finished tile.
    pub sink: Box<dyn FnMut(Tile) + Send>,
}

impl Job for ProgressiveRenderJob {
    fn label(&self) -> &str {
        "render viewport"
    }

    fn priority(&self) -> Priority {
        self.priority
    }

    fn run(mut self: Box<Self>, ctx: &JobContext) -> EngineResult<()> {
        let coarsest = self.viewport.coarsest_level;
        let levels = f32::from(coarsest.saturating_sub(self.viewport.finest_level)) + 1.0;
        let sink = &mut self.sink;
        let mut last_level = None;
        let mut deliver = |t: Tile| {
            let level = t.coord().level;
            if last_level != Some(level) {
                last_level = Some(level);
                ctx.report_progress(f32::from(coarsest - level) / levels, None);
            }
            sink(t);
        };
        self.renderer.render_progressive(
            &self.image,
            &self.settings,
            &self.viewport,
            self.output,
            &ctx.cancellation,
            &mut deliver,
        )?;
        ctx.report_progress(1.0, None);
        Ok(())
    }
}
