//! The tiled compositor: scene-graph traversal per tile at any pyramid
//! level, per-layer tile caches, dirty-rect recompositing.

mod cache;
pub(crate) mod exec;
pub(crate) mod pixel;

use std::collections::HashMap;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use engine_api::jobs::CancellationToken;
use engine_api::tile::{Extent, Pyramid, TILE_SIZE, Tile, TileCoord, TileFormat, TileLayout};
use engine_api::{EngineError, EngineResult};
use rayon::prelude::*;

use crate::document::{DocState, Layer, SmartObject};
use crate::edit::Document;
use crate::geom::Rect;
use crate::raster::{Raster, load_normalized, tile_from_normalized};
use cache::RenderCache;
pub(crate) use cache::{NodeKey, Part};
use exec::{Region, TileJob};

/// Levels beyond the one-tile level are allowed (thumbnails, smart-object
/// minification); every level is at least 1×1.
pub const MAX_LEVEL: u8 = 24;

/// A document state plus its cache namespace.
#[derive(Clone, Copy)]
pub(crate) struct DocRef<'a> {
    pub state: &'a DocState,
    pub key: u64,
}

#[derive(Default)]
pub(crate) struct Counters {
    hits: AtomicU64,
    misses: AtomicU64,
    mips: AtomicU64,
    groups: AtomicU64,
    root_full: AtomicU64,
    root_partial: AtomicU64,
    root_reused: AtomicU64,
    smart: AtomicU64,
    blends: AtomicU64,
}

impl Counters {
    pub(crate) fn bump_blend(&self) {
        self.blends.fetch_add(1, Ordering::Relaxed);
    }
    pub(crate) fn bump_group(&self) {
        self.groups.fetch_add(1, Ordering::Relaxed);
    }
}

/// Counters since creation or [`Compositor::reset_stats`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CompositorStats {
    /// Cache lookups that hit.
    pub cache_hits: u64,
    /// Cache lookups that missed.
    pub cache_misses: u64,
    /// Layer/mask mip tiles computed.
    pub mip_tiles: u64,
    /// Isolated-group composites computed (and cached).
    pub group_tiles: u64,
    /// Document tiles composited in full.
    pub root_full: u64,
    /// Document tiles recomposited over a dirty sub-rectangle only.
    pub root_partial: u64,
    /// Document tiles whose damage missed them (previous tile re-keyed).
    pub root_reused: u64,
    /// Smart-object tiles resampled.
    pub smart_tiles: u64,
    /// Layer blend operations executed (one per layer per tile/region).
    pub blends: u64,
    /// Resident cache bytes.
    pub cache_bytes: usize,
    /// Resident cache entries.
    pub cache_entries: usize,
    /// Entries evicted to stay within budget.
    pub evictions: u64,
}

/// Renders documents tile by tile with caching. Thread-safe; share one per
/// process (or per window).
pub struct Compositor {
    cache: RenderCache,
    pub(crate) stats: Counters,
    latest: Mutex<HashMap<(u64, TileCoord), (u64, u64)>>,
    /// Whether dirty-rect (sub-tile) recompositing is enabled.
    pub partial_updates: bool,
}

impl Compositor {
    /// A compositor with a cache budget in bytes.
    pub fn new(cache_budget: usize) -> Self {
        Self {
            cache: RenderCache::new(cache_budget),
            stats: Counters::default(),
            latest: Mutex::new(HashMap::new()),
            partial_updates: true,
        }
    }

    /// Current counters.
    pub fn stats(&self) -> CompositorStats {
        let s = &self.stats;
        let l = |a: &AtomicU64| a.load(Ordering::Relaxed);
        CompositorStats {
            cache_hits: l(&s.hits),
            cache_misses: l(&s.misses),
            mip_tiles: l(&s.mips),
            group_tiles: l(&s.groups),
            root_full: l(&s.root_full),
            root_partial: l(&s.root_partial),
            root_reused: l(&s.root_reused),
            smart_tiles: l(&s.smart),
            blends: l(&s.blends),
            cache_bytes: self.cache.bytes(),
            cache_entries: self.cache.len(),
            evictions: self.cache.evictions(),
        }
    }

    /// Zeroes the counters (cache contents are kept).
    pub fn reset_stats(&self) {
        let s = &self.stats;
        for a in [
            &s.hits,
            &s.misses,
            &s.mips,
            &s.groups,
            &s.root_full,
            &s.root_partial,
            &s.root_reused,
            &s.smart,
            &s.blends,
        ] {
            a.store(0, Ordering::Relaxed);
        }
    }

    /// Drops composites (root, groups, smart objects) but keeps layer mips.
    pub fn clear_composites(&self) {
        self.cache
            .retain(|k| matches!(k.part, Part::Content | Part::Mask));
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// Drops everything.
    pub fn clear(&self) {
        self.cache.retain(|_| false);
        self.latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    pub(crate) fn cache_get(&self, key: &NodeKey) -> Option<Tile> {
        let t = self.cache.get(key);
        let c = if t.is_some() {
            &self.stats.hits
        } else {
            &self.stats.misses
        };
        c.fetch_add(1, Ordering::Relaxed);
        t
    }

    pub(crate) fn cache_put(&self, key: NodeKey, tile: Tile) {
        self.cache.insert(key, tile);
    }

    /// A raster's tile at `coord.level` (mip), in the raster's depth,
    /// straight; `None` when the footprint holds no tiles (all default).
    pub(crate) fn raster_level(
        &self,
        doc: u64,
        node: u64,
        part: Part,
        raster: &Raster,
        coord: TileCoord,
    ) -> EngineResult<Option<Tile>> {
        let level = coord.level;
        if level == 0 {
            return Ok(raster.tile(coord.x, coord.y).cloned());
        }
        if !raster.footprint_has_tile(level, coord.x, coord.y) {
            return Ok(None);
        }
        let key = NodeKey {
            doc,
            node,
            part,
            stamp: raster.footprint_rev(level, coord.x, coord.y),
            coord,
        };
        if let Some(t) = self.cache_get(&key) {
            return Ok(Some(t));
        }
        let le = raster.extent().at_level(level);
        let ce = raster.extent().at_level(level - 1);
        let out = Rect::of_tile(coord, le);
        let (w, h) = (out.width() as usize, out.height() as usize);
        let ch = raster.channels() as usize;
        let def = raster.default_value();
        let (ccols, crows) = ce.tile_grid(TILE_SIZE);
        let kids = coord
            .children()
            .ok_or_else(|| EngineError::internal("level 0 children"))?;
        let depth = raster.depth();
        let exact = depth != crate::raster::Depth::F32 && (def == 0.0 || def == 1.0);
        let tile = if exact {
            let mut data: [Option<Tile>; 4] = Default::default();
            let mut layouts = [(0usize, 0usize); 4];
            for (k, c) in kids.iter().enumerate() {
                if c.x < ccols && c.y < crows {
                    data[k] = self.raster_level(doc, node, part, raster, *c)?;
                    if let Some(t) = &data[k] {
                        layouts[k] = (t.layout().stride(), t.layout().plane_len());
                    }
                }
            }
            let layout = TileLayout {
                extent: Extent::new(w as u32, h as u32),
                halo: 0,
                channels: ch as u8,
            };
            let dims = (ce.width as usize, ce.height as usize);
            let at = (coord.x as usize, coord.y as usize);
            if depth == crate::raster::Depth::U8 {
                let mut kids8: [Option<&[u8]>; 4] = [None; 4];
                for (k, t) in data.iter().enumerate() {
                    kids8[k] = t.as_ref().map(|t| t.samples::<u8>()).transpose()?;
                }
                let def = if def == 1.0 { u8::MAX } else { 0 };
                let res = mip_exact(&kids8, &layouts, (w, h), dims, at, ch, def);
                Tile::from_samples(coord, layout, res)?
            } else {
                let mut kids16: [Option<&[u16]>; 4] = [None; 4];
                for (k, t) in data.iter().enumerate() {
                    kids16[k] = t.as_ref().map(|t| t.samples::<u16>()).transpose()?;
                }
                let def = if def == 1.0 { u16::MAX } else { 0 };
                let res = mip_exact(&kids16, &layouts, (w, h), dims, at, ch, def);
                Tile::from_samples(coord, layout, res)?
            }
        } else {
            self.mip_float(doc, node, part, raster, coord, (w, h), ce, &kids)?
        };
        self.stats.mips.fetch_add(1, Ordering::Relaxed);
        self.cache_put(key, tile.clone());
        Ok(Some(tile))
    }

    /// The f32 mip of one tile (float rasters, and 8/16-bit rasters with a
    /// default other than 0 or 1), quantized back to the raster depth.
    #[allow(clippy::too_many_arguments)]
    fn mip_float(
        &self,
        doc: u64,
        node: u64,
        part: Part,
        raster: &Raster,
        coord: TileCoord,
        (w, h): (usize, usize),
        ce: Extent,
        kids: &[TileCoord; 4],
    ) -> EngineResult<Tile> {
        let ch = raster.channels() as usize;
        let def = raster.default_value();
        let (ccols, crows) = ce.tile_grid(TILE_SIZE);
        let mut data: [Option<(Vec<f32>, usize, usize)>; 4] = Default::default();
        for (k, c) in kids.iter().enumerate() {
            if c.x < ccols
                && c.y < crows
                && let Some(t) = self.raster_level(doc, node, part, raster, *c)?
            {
                let l = t.layout();
                let mut v = vec![0.0; l.len()];
                load_normalized(&t, &mut v)?;
                data[k] = Some((v, l.stride(), l.plane_len()));
            }
        }
        let n = w * h;
        let mut res = vec![0.0f32; ch * n];
        let ts = TILE_SIZE as usize;
        let (cw, chh) = (ce.width as usize, ce.height as usize);
        let (bx, by) = (2 * coord.x as usize * ts, 2 * coord.y as usize * ts);
        for y in 0..h {
            for x in 0..w {
                let mut acc = [0.0f32; 4];
                let mut cnt = 0.0f32;
                for dy in 0..2 {
                    let cy = by + 2 * y + dy;
                    if cy >= chh {
                        continue;
                    }
                    for dx in 0..2 {
                        let cx = bx + 2 * x + dx;
                        if cx >= cw {
                            continue;
                        }
                        let (lx, ly) = (cx - bx, cy - by);
                        let k = (ly / ts) * 2 + lx / ts;
                        let (px, py) = (lx % ts, ly % ts);
                        cnt += 1.0;
                        let get = |c: usize| match &data[k] {
                            Some((v, stride, plane)) => v[c * plane + py * stride + px],
                            None => def,
                        };
                        if ch == 4 {
                            let a = get(3);
                            acc[0] += a * get(0);
                            acc[1] += a * get(1);
                            acc[2] += a * get(2);
                            acc[3] += a;
                        } else {
                            acc[0] += get(0);
                        }
                    }
                }
                let i = y * w + x;
                if ch == 4 {
                    let inv = if acc[3] > 0.0 { 1.0 / acc[3] } else { 0.0 };
                    res[i] = acc[0] * inv;
                    res[n + i] = acc[1] * inv;
                    res[2 * n + i] = acc[2] * inv;
                    res[3 * n + i] = acc[3] / cnt.max(1.0);
                } else {
                    res[i] = acc[0] / cnt.max(1.0);
                }
            }
        }
        let layout = TileLayout {
            extent: Extent::new(w as u32, h as u32),
            halo: 0,
            channels: ch as u8,
        };
        tile_from_normalized(coord, layout, raster.depth(), &res)
    }

    /// A smart object resampled into the parent at `coord` (straight f32
    /// RGBA), or `None` when it does not reach this tile.
    pub(crate) fn smart_tile(
        &self,
        doc: DocRef<'_>,
        layer: &Layer,
        so: &SmartObject,
        coord: TileCoord,
    ) -> EngineResult<Option<Tile>> {
        let le = doc.state.canvas.at_level(coord.level);
        let tile_rect = Rect::of_tile(coord, le);
        let parent_rect = tile_rect.to_level0(coord.level);
        if !so.bounds().intersects(&parent_rect) {
            return Ok(None);
        }
        let key = NodeKey {
            doc: doc.key,
            node: layer.id.0,
            part: Part::Smart,
            stamp: so.state.rev.max(layer.content_rev),
            coord,
        };
        if let Some(t) = self.cache_get(&key) {
            return Ok(Some(t));
        }
        let inv = so
            .transform
            .inverse()
            .ok_or_else(|| EngineError::invalid("transform", "singular"))?;
        let child = DocRef {
            state: &so.state,
            key: so.key,
        };
        let ce0 = so.state.canvas;
        let scale = f64::from(1u32 << coord.level);
        let jac = scale * inv.det().abs().sqrt();
        let lc = if jac > 1.0 {
            (jac.log2().floor().min(f64::from(MAX_LEVEL - 1))) as u8
        } else {
            0
        };
        let ce = ce0.at_level(lc);
        let cscale = f64::from(1u32 << lc);
        // Child tiles needed at level lc.
        let need = inv.map_rect(&parent_rect).inflate(2);
        let need_l = Rect::new(
            (need.x0 as f64 / cscale).floor() as i64 - 1,
            (need.y0 as f64 / cscale).floor() as i64 - 1,
            (need.x1 as f64 / cscale).ceil() as i64 + 1,
            (need.y1 as f64 / cscale).ceil() as i64 + 1,
        )
        .intersect(&Rect::of_extent(ce));
        let mut tiles: HashMap<(u32, u32), Tile> = HashMap::new();
        if !need_l.is_empty() {
            let ts = i64::from(TILE_SIZE);
            for ty in need_l.y0 / ts..=(need_l.y1 - 1) / ts {
                for tx in need_l.x0 / ts..=(need_l.x1 - 1) / ts {
                    let c = TileCoord::new(lc, tx as u32, ty as u32);
                    tiles.insert((tx as u32, ty as u32), self.composite_premult(child, c)?);
                }
            }
        }
        let (w, h) = (tile_rect.width() as usize, tile_rect.height() as usize);
        let n = w * h;
        let mut out = vec![0.0f32; 4 * n];
        let fetch = |x: i64, y: i64| -> [f32; 4] {
            if x < 0 || y < 0 || x >= i64::from(ce.width) || y >= i64::from(ce.height) {
                return [0.0; 4];
            }
            let (tx, ty) = ((x as u32) / TILE_SIZE, (y as u32) / TILE_SIZE);
            let Some(t) = tiles.get(&(tx, ty)) else {
                return [0.0; 4];
            };
            let l = t.layout();
            let s = t.samples::<f32>().unwrap_or(&[]);
            let i =
                (y as usize % TILE_SIZE as usize) * l.stride() + (x as usize % TILE_SIZE as usize);
            let p = l.plane_len();
            if s.len() < 4 * p {
                return [0.0; 4];
            }
            [s[i], s[p + i], s[2 * p + i], s[3 * p + i]]
        };
        let mut any = false;
        for y in 0..h {
            for x in 0..w {
                let px = (tile_rect.x0 as f64 + x as f64 + 0.5) * scale;
                let py = (tile_rect.y0 as f64 + y as f64 + 0.5) * scale;
                let (qx, qy) = inv.apply(px, py);
                let (fx, fy) = (qx / cscale - 0.5, qy / cscale - 0.5);
                let (x0, y0) = (fx.floor(), fy.floor());
                let (ax, ay) = ((fx - x0) as f32, (fy - y0) as f32);
                let (ix, iy) = (x0 as i64, y0 as i64);
                let p00 = fetch(ix, iy);
                let p10 = fetch(ix + 1, iy);
                let p01 = fetch(ix, iy + 1);
                let p11 = fetch(ix + 1, iy + 1);
                let mut v = [0.0f32; 4];
                for c in 0..4 {
                    let top = p00[c] + (p10[c] - p00[c]) * ax;
                    let bot = p01[c] + (p11[c] - p01[c]) * ax;
                    v[c] = top + (bot - top) * ay;
                }
                let i = y * w + x;
                let rgb = pixel::unpremul(v);
                out[i] = rgb[0];
                out[n + i] = rgb[1];
                out[2 * n + i] = rgb[2];
                out[3 * n + i] = v[3];
                any |= v[3] > 0.0;
            }
        }
        self.stats.smart.fetch_add(1, Ordering::Relaxed);
        if !any {
            return Ok(None);
        }
        let layout = TileLayout {
            extent: Extent::new(w as u32, h as u32),
            halo: 0,
            channels: 4,
        };
        let tile = Tile::from_samples(coord, layout, out)?;
        self.cache_put(key, tile.clone());
        Ok(Some(tile))
    }

    pub(crate) fn job<'a>(
        &'a self,
        doc: DocRef<'a>,
        coord: TileCoord,
    ) -> EngineResult<TileJob<'a>> {
        let le = doc.state.canvas.at_level(coord.level);
        let (cols, rows) = le.tile_grid(TILE_SIZE);
        if coord.level >= MAX_LEVEL || coord.x >= cols || coord.y >= rows {
            return Err(EngineError::invalid(
                "coord",
                format!("{coord} outside document"),
            ));
        }
        let r = Rect::of_tile(coord, le);
        let (w, h) = (r.width() as usize, r.height() as usize);
        Ok(TileJob {
            comp: self,
            doc,
            coord,
            w,
            n: w * h,
            origin: (r.x0 as u32, r.y0 as u32),
            region: Region {
                x0: 0,
                y0: 0,
                x1: w,
                y1: h,
            },
            full: true,
        })
    }

    /// The premultiplied composite of a document state at `coord`, cached.
    pub(crate) fn composite_premult(
        &self,
        doc: DocRef<'_>,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        let key = NodeKey {
            doc: doc.key,
            node: 0,
            part: Part::Root,
            stamp: doc.state.root_stamp(coord.level, coord.x, coord.y),
            coord,
        };
        if let Some(t) = self.cache_get(&key) {
            return Ok(t);
        }
        let job = self.job(doc, coord)?;
        let ops = job.compile()?;
        let acc = job.run(&ops)?;
        self.stats.root_full.fetch_add(1, Ordering::Relaxed);
        let t = Tile::from_samples(coord, job.layout(), acc)?.with_premultiplied(true)?;
        self.cache_put(key, t.clone());
        Ok(t)
    }

    /// The composite of `doc` at `coord` as premultiplied f32 RGBA,
    /// recompositing only the damaged sub-rectangle when the previous
    /// version of the tile is cached (dirty-rect compositing).
    pub fn render_tile_premultiplied(
        &self,
        doc: &Document,
        coord: TileCoord,
    ) -> EngineResult<Tile> {
        let state = doc.state();
        let dref = DocRef {
            state,
            key: doc.key(),
        };
        let stamp = state.root_stamp(coord.level, coord.x, coord.y);
        let key = NodeKey {
            doc: doc.key(),
            node: 0,
            part: Part::Root,
            stamp,
            coord,
        };
        let remember = |s: u64| {
            self.latest
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .insert((doc.key(), coord), (doc.epoch(), s));
        };
        if let Some(t) = self.cache_get(&key) {
            remember(stamp);
            return Ok(t);
        }
        let prev = self
            .latest
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&(doc.key(), coord))
            .copied();
        if let Some((epoch, old)) = prev.filter(|_| self.partial_updates)
            && epoch == doc.epoch()
            && old < stamp
        {
            let le = state.canvas.at_level(coord.level);
            let tr = Rect::of_tile(coord, le);
            if let (Some(old_tile), Some(damage)) = (
                self.cache.get(&NodeKey { stamp: old, ..key }),
                doc.damage_between(old, stamp, tr.to_level0(coord.level)),
            ) {
                let d = damage.to_level(coord.level).intersect(&tr);
                if d.is_empty() {
                    self.stats.root_reused.fetch_add(1, Ordering::Relaxed);
                    self.cache_put(key, old_tile.clone());
                    remember(stamp);
                    return Ok(old_tile);
                }
                if d.area() * 2 <= tr.area() {
                    let mut job = self.job(dref, coord)?;
                    job.full = false;
                    job.region = Region {
                        x0: (d.x0 - tr.x0) as usize,
                        y0: (d.y0 - tr.y0) as usize,
                        x1: (d.x1 - tr.x0) as usize,
                        y1: (d.y1 - tr.y0) as usize,
                    };
                    let ops = job.compile()?;
                    let acc = job.run(&ops)?;
                    let mut t = old_tile;
                    let dst = t.samples_mut::<f32>()?;
                    let (n, w, r) = (job.n, job.w, job.region);
                    for c in 0..4 {
                        for y in r.y0..r.y1 {
                            let a = c * n + y * w;
                            dst[a + r.x0..a + r.x1].copy_from_slice(&acc[a + r.x0..a + r.x1]);
                        }
                    }
                    self.stats.root_partial.fetch_add(1, Ordering::Relaxed);
                    self.cache_put(key, t.clone());
                    remember(stamp);
                    return Ok(t);
                }
            }
        }
        let t = self.composite_premult(dref, coord)?;
        remember(stamp);
        Ok(t)
    }

    /// The composite at `coord` as straight-alpha f32 RGBA (planar).
    pub fn render_tile(&self, doc: &Document, coord: TileCoord) -> EngineResult<Tile> {
        unpremultiply(&self.render_tile_premultiplied(doc, coord)?)
    }

    /// Every tile of `level`, rendered in parallel, in raster order.
    pub fn render_level(
        &self,
        doc: &Document,
        level: u8,
        cancel: &CancellationToken,
    ) -> EngineResult<Vec<Tile>> {
        let (cols, rows) = doc.state().canvas.at_level(level).tile_grid(TILE_SIZE);
        let coords: Vec<TileCoord> = (0..rows)
            .flat_map(|y| (0..cols).map(move |x| TileCoord::new(level, x, y)))
            .collect();
        coords
            .par_iter()
            .map(|c| {
                cancel.check()?;
                self.render_tile_premultiplied(doc, *c)
            })
            .collect::<EngineResult<Vec<_>>>()?
            .iter()
            .map(unpremultiply)
            .collect()
    }

    /// A whole level as interleaved straight RGBA (`width·height·4`).
    pub fn render_level_rgba(&self, doc: &Document, level: u8) -> EngineResult<(Extent, Vec<f32>)> {
        let e = doc.state().canvas.at_level(level);
        let tiles = self.render_level(doc, level, &CancellationToken::new())?;
        Ok((e, interleave(e, &tiles)?))
    }

    /// A read-only [`Pyramid`] view of the composite (straight f32 RGBA).
    pub fn pyramid<'a>(&'a self, doc: &'a Document) -> CompositePyramid<'a> {
        CompositePyramid { comp: self, doc }
    }
}

/// The 2×2 mip of 8/16-bit code values in exact integer arithmetic:
/// `C = round(ΣAᵢCᵢ / ΣAᵢ)`, `A = round(ΣAᵢ / n)` for RGBA and
/// `v = round(Σvᵢ / n)` for one channel, rounding halves up. This is the
/// COMPOSITOR.md §5 definition evaluated exactly, so the GPU mip shader
/// reproduces it bit for bit. `default` is the code of absent children.
pub(crate) fn mip_exact<T: Copy + Into<u64> + TryFrom<u64>>(
    kids: &[Option<&[T]>; 4],
    kid_layouts: &[(usize, usize); 4],
    (w, h): (usize, usize),
    (cw, chh): (usize, usize),
    (tx, ty): (usize, usize),
    ch: usize,
    default: T,
) -> Vec<T> {
    let ts = TILE_SIZE as usize;
    let n = w * h;
    let mut res = vec![default; ch * n];
    let (bx, by) = (2 * tx * ts, 2 * ty * ts);
    let q = |v: u64| T::try_from(v).unwrap_or(default);
    for y in 0..h {
        for x in 0..w {
            let (mut num, mut den, mut sum, mut cnt) = ([0u64; 3], 0u64, 0u64, 0u64);
            for dy in 0..2 {
                let cy = by + 2 * y + dy;
                if cy >= chh {
                    continue;
                }
                for dx in 0..2 {
                    let cx = bx + 2 * x + dx;
                    if cx >= cw {
                        continue;
                    }
                    let (lx, ly) = (cx - bx, cy - by);
                    let k = (ly / ts) * 2 + lx / ts;
                    let (px, py) = (lx % ts, ly % ts);
                    let (stride, plane) = kid_layouts[k];
                    let get = |c: usize| -> u64 {
                        match kids[k] {
                            Some(v) => v[c * plane + py * stride + px].into(),
                            None => default.into(),
                        }
                    };
                    cnt += 1;
                    if ch == 4 {
                        let a = get(3);
                        for (c, s) in num.iter_mut().enumerate() {
                            *s += a * get(c);
                        }
                        den += a;
                    } else {
                        sum += get(0);
                    }
                }
            }
            let i = y * w + x;
            let cnt = cnt.max(1);
            if ch == 4 {
                for c in 0..3 {
                    res[c * n + i] = q(if den > 0 {
                        (2 * num[c] + den) / (2 * den)
                    } else {
                        0
                    });
                }
                res[3 * n + i] = q((2 * den + cnt) / (2 * cnt));
            } else {
                res[i] = q((2 * sum + cnt) / (2 * cnt));
            }
        }
    }
    res
}

/// Planar tiles of a level → interleaved RGBA.
pub fn interleave(e: Extent, tiles: &[Tile]) -> EngineResult<Vec<f32>> {
    let mut out = vec![0.0f32; e.width as usize * e.height as usize * 4];
    for t in tiles {
        let (ox, oy) = t.coord().pixel_origin(TILE_SIZE);
        let l = t.layout();
        let s = t.samples::<f32>()?;
        let p = l.plane_len();
        for y in 0..l.extent.height as usize {
            for x in 0..l.extent.width as usize {
                let o = ((oy as usize + y) * e.width as usize + ox as usize + x) * 4;
                for c in 0..4 {
                    out[o + c] = s[c * p + y * l.stride() + x];
                }
            }
        }
    }
    Ok(out)
}

/// Premultiplied f32 RGBA tile → straight (the result's
/// [`Tile::premultiplied`] flag is false).
pub fn unpremultiply(t: &Tile) -> EngineResult<Tile> {
    let s = t.samples::<f32>()?;
    let n = t.layout().plane_len();
    let mut o = s.to_vec();
    for i in 0..n {
        let c = pixel::unpremul([s[i], s[n + i], s[2 * n + i], s[3 * n + i]]);
        o[i] = c[0];
        o[n + i] = c[1];
        o[2 * n + i] = c[2];
    }
    Tile::from_samples(t.coord(), t.layout(), o)
}

/// [`Pyramid`] over a document composite.
pub struct CompositePyramid<'a> {
    comp: &'a Compositor,
    doc: &'a Document,
}

impl Pyramid for CompositePyramid<'_> {
    fn extent(&self) -> Extent {
        self.doc.state().canvas
    }
    fn format(&self) -> TileFormat {
        TileFormat::F32Planar
    }
    fn channels(&self) -> u8 {
        4
    }
    fn halo(&self) -> u16 {
        0
    }
    /// Every level down to 1×1 (thumbnails and far zoom-outs), capped at
    /// [`MAX_LEVEL`].
    fn level_count(&self) -> u8 {
        self.extent().full_level_count().min(MAX_LEVEL)
    }
    fn tile(&self, coord: TileCoord) -> EngineResult<Tile> {
        self.comp.render_tile(self.doc, coord)
    }
}
