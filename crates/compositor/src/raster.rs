//! Tiled, copy-on-write rasters with per-tile revisions.
//!
//! A [`Raster`] is the level-0 storage of a pixel layer, a mask, a text proxy
//! or a selection. Tiles are engine-api [`Tile`]s (256², planar, no halo) in
//! the document depth; cloning a raster clones only `Arc`s. Every slot
//! carries the revision of the write that produced it. Erasing leaves a
//! tombstone (a slot without a tile) so that footprint revisions never go
//! backwards within a lineage (COMPOSITOR.md §5).

use std::collections::BTreeMap;

use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord, TileFormat, TileLayout};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

use crate::geom::Rect;

/// Bits per channel of a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Depth {
    /// 8-bit unsigned integer, display-referred `[0, 1]`.
    #[default]
    U8,
    /// 16-bit unsigned integer, display-referred `[0, 1]`.
    U16,
    /// 32-bit float, unbounded (scene-referred documents).
    F32,
}

impl Depth {
    /// Tile storage format.
    pub fn format(self) -> TileFormat {
        match self {
            Depth::U8 => TileFormat::U8,
            Depth::U16 => TileFormat::U16,
            Depth::F32 => TileFormat::F32Planar,
        }
    }

    /// True for the float depth (no clamping of blend results).
    pub fn is_float(self) -> bool {
        self == Depth::F32
    }

    /// Bytes per sample.
    pub fn bytes(self) -> usize {
        self.format().bytes_per_sample()
    }
}

/// One tile position of a raster.
#[derive(Clone, Debug)]
pub struct Slot {
    /// Samples, or `None` for a tombstone (reads as the default value).
    pub tile: Option<Tile>,
    /// Revision of the write that produced this slot.
    pub rev: u64,
}

static U8_LUT: std::sync::LazyLock<[f32; 256]> =
    std::sync::LazyLock::new(|| std::array::from_fn(|i| i as f32 / 255.0));

/// Quantizes a normalized value to the depth's code value.
#[inline]
pub(crate) fn quantize_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
pub(crate) fn quantize_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0 + 0.5) as u16
}

/// Converts every sample of `tile` to normalized f32 into `out` (same
/// planar layout, same length as the tile's sample count).
pub(crate) fn load_normalized(tile: &Tile, out: &mut [f32]) -> EngineResult<()> {
    match tile.format() {
        TileFormat::U8 => {
            let lut = &*U8_LUT;
            for (o, s) in out.iter_mut().zip(tile.samples::<u8>()?) {
                *o = lut[*s as usize];
            }
        }
        TileFormat::U16 => {
            for (o, s) in out.iter_mut().zip(tile.samples::<u16>()?) {
                *o = f32::from(*s) * (1.0 / 65535.0);
            }
        }
        TileFormat::F32Planar => out.copy_from_slice(tile.samples::<f32>()?),
        TileFormat::F16Planar => {
            for (o, s) in out.iter_mut().zip(tile.samples::<half::f16>()?) {
                *o = s.to_f32();
            }
        }
    }
    Ok(())
}

/// Like [`load_normalized`] but only for the rows `y0..y1`, columns
/// `x0..x1` of every plane (interior coordinates, halo-free tiles).
pub(crate) fn load_normalized_region(
    tile: &Tile,
    out: &mut [f32],
    (x0, y0, x1, y1): (usize, usize, usize, usize),
) -> EngineResult<()> {
    let l = tile.layout();
    let (stride, plane) = (l.stride(), l.plane_len());
    let lut = &*U8_LUT;
    for c in 0..l.channels as usize {
        for y in y0..y1 {
            let a = c * plane + y * stride;
            let dst = &mut out[a + x0..a + x1];
            match tile.format() {
                TileFormat::U8 => {
                    for (o, s) in dst.iter_mut().zip(&tile.samples::<u8>()?[a + x0..a + x1]) {
                        *o = lut[*s as usize];
                    }
                }
                TileFormat::U16 => {
                    for (o, s) in dst.iter_mut().zip(&tile.samples::<u16>()?[a + x0..a + x1]) {
                        *o = f32::from(*s) * (1.0 / 65535.0);
                    }
                }
                TileFormat::F32Planar => {
                    dst.copy_from_slice(&tile.samples::<f32>()?[a + x0..a + x1])
                }
                TileFormat::F16Planar => {
                    for (o, s) in dst
                        .iter_mut()
                        .zip(&tile.samples::<half::f16>()?[a + x0..a + x1])
                    {
                        *o = s.to_f32();
                    }
                }
            }
        }
    }
    Ok(())
}

/// Builds a tile of `depth` from normalized planar samples.
pub(crate) fn tile_from_normalized(
    coord: TileCoord,
    layout: TileLayout,
    depth: Depth,
    data: &[f32],
) -> EngineResult<Tile> {
    match depth {
        Depth::U8 => Tile::from_samples(
            coord,
            layout,
            data.iter().map(|v| quantize_u8(*v)).collect(),
        ),
        Depth::U16 => Tile::from_samples(
            coord,
            layout,
            data.iter().map(|v| quantize_u16(*v)).collect(),
        ),
        Depth::F32 => Tile::from_samples(coord, layout, data.to_vec()),
    }
}

/// Address of a buffer, for counting unique tile allocations.
pub(crate) fn buffer_addr(tile: &Tile) -> usize {
    match tile.format() {
        TileFormat::U8 => tile.samples::<u8>().map(|s| s.as_ptr() as usize),
        TileFormat::U16 => tile.samples::<u16>().map(|s| s.as_ptr() as usize),
        TileFormat::F32Planar => tile.samples::<f32>().map(|s| s.as_ptr() as usize),
        TileFormat::F16Planar => tile.samples::<half::f16>().map(|s| s.as_ptr() as usize),
    }
    .unwrap_or(0)
}

/// A tiled, copy-on-write, single-level image.
#[derive(Clone, Debug)]
pub struct Raster {
    extent: Extent,
    channels: u8,
    depth: Depth,
    default: f32,
    slots: BTreeMap<(u32, u32), Slot>,
}

impl Raster {
    /// An empty raster: every pixel reads as `default` in every channel.
    pub fn new(extent: Extent, channels: u8, depth: Depth, default: f32) -> Self {
        Self {
            extent,
            channels,
            depth,
            default,
            slots: BTreeMap::new(),
        }
    }

    /// Canvas size.
    pub fn extent(&self) -> Extent {
        self.extent
    }

    /// Channel count (4 = straight RGBA, 1 = mask/selection).
    pub fn channels(&self) -> u8 {
        self.channels
    }

    /// Sample depth.
    pub fn depth(&self) -> Depth {
        self.depth
    }

    /// Value of pixels in absent tiles.
    pub fn default_value(&self) -> f32 {
        self.default
    }

    /// Tile grid `(columns, rows)`.
    pub fn grid(&self) -> (u32, u32) {
        self.extent.tile_grid(TILE_SIZE)
    }

    /// Layout of the level-0 tile at `(tx, ty)`.
    pub fn layout(&self, tx: u32, ty: u32) -> TileLayout {
        let w = self
            .extent
            .width
            .saturating_sub(tx * TILE_SIZE)
            .min(TILE_SIZE);
        let h = self
            .extent
            .height
            .saturating_sub(ty * TILE_SIZE)
            .min(TILE_SIZE);
        TileLayout {
            extent: Extent::new(w, h),
            halo: 0,
            channels: self.channels,
        }
    }

    /// The tile at `(tx, ty)` if one is stored.
    pub fn tile(&self, tx: u32, ty: u32) -> Option<&Tile> {
        self.slots.get(&(ty, tx)).and_then(|s| s.tile.as_ref())
    }

    /// The slot at `(tx, ty)` (tile or tombstone).
    pub fn slot(&self, tx: u32, ty: u32) -> Option<&Slot> {
        self.slots.get(&(ty, tx))
    }

    /// All slots in raster order as `((tx, ty), slot)`.
    pub fn slots(&self) -> impl Iterator<Item = ((u32, u32), &Slot)> {
        self.slots.iter().map(|(&(y, x), s)| ((x, y), s))
    }

    /// Replaces a slot. `tile` must match the raster's layout and depth.
    pub fn set_slot(&mut self, tx: u32, ty: u32, tile: Option<Tile>, rev: u64) -> EngineResult<()> {
        let (cols, rows) = self.grid();
        if tx >= cols || ty >= rows {
            return Err(EngineError::invalid(
                "tile",
                format!("({tx},{ty}) outside raster"),
            ));
        }
        if let Some(t) = &tile
            && (t.layout() != self.layout(tx, ty) || t.format() != self.depth.format())
        {
            return Err(EngineError::invalid(
                "tile",
                format!("({tx},{ty}) layout/format does not match raster"),
            ));
        }
        self.slots.insert((ty, tx), Slot { tile, rev });
        Ok(())
    }

    /// Maximum slot revision over the level-0 tiles covered by the level
    /// `level` tile `(tx, ty)`; 0 when none were ever written.
    pub fn footprint_rev(&self, level: u8, tx: u32, ty: u32) -> u64 {
        let (cols, rows) = self.grid();
        let x0 = tx.checked_shl(u32::from(level)).unwrap_or(u32::MAX);
        let y0 = ty.checked_shl(u32::from(level)).unwrap_or(u32::MAX);
        if x0 >= cols || y0 >= rows {
            return 0;
        }
        let span = 1u32.checked_shl(u32::from(level)).unwrap_or(u32::MAX);
        let x1 = x0.saturating_add(span).min(cols);
        let y1 = y0.saturating_add(span).min(rows);
        if self.slots.len() as u64 <= u64::from(y1 - y0) {
            return self
                .slots
                .iter()
                .filter(|((y, x), _)| *y >= y0 && *y < y1 && *x >= x0 && *x < x1)
                .map(|(_, s)| s.rev)
                .max()
                .unwrap_or(0);
        }
        let mut best = 0;
        for y in y0..y1 {
            for (_, s) in self.slots.range((y, x0)..(y, x1)) {
                best = best.max(s.rev);
            }
        }
        best
    }

    /// True if any stored (non-tombstone) tile lies in the footprint of the
    /// level `level` tile `(tx, ty)`.
    pub fn footprint_has_tile(&self, level: u8, tx: u32, ty: u32) -> bool {
        let (cols, rows) = self.grid();
        let x0 = tx.checked_shl(u32::from(level)).unwrap_or(u32::MAX);
        let y0 = ty.checked_shl(u32::from(level)).unwrap_or(u32::MAX);
        if x0 >= cols || y0 >= rows {
            return false;
        }
        let span = 1u32.checked_shl(u32::from(level)).unwrap_or(u32::MAX);
        let x1 = x0.saturating_add(span).min(cols);
        let y1 = y0.saturating_add(span).min(rows);
        (y0..y1).any(|y| {
            self.slots
                .range((y, x0)..(y, x1))
                .any(|(_, s)| s.tile.is_some())
        })
    }

    /// Maximum revision over all slots.
    pub fn max_rev(&self) -> u64 {
        self.slots.values().map(|s| s.rev).max().unwrap_or(0)
    }

    /// Normalized samples of the level-0 tile `(tx, ty)` (planar, straight).
    pub fn read_tile(&self, tx: u32, ty: u32, out: &mut Vec<f32>) -> EngineResult<()> {
        let layout = self.layout(tx, ty);
        out.clear();
        out.resize(layout.len(), self.default);
        if let Some(t) = self.tile(tx, ty) {
            load_normalized(t, out)?;
        }
        Ok(())
    }

    /// Normalized value of one pixel (channels beyond the raster's read 0).
    pub fn pixel(&self, x: u32, y: u32) -> [f32; 4] {
        let mut px = [0.0; 4];
        if x >= self.extent.width || y >= self.extent.height {
            return px;
        }
        let (tx, ty) = (x / TILE_SIZE, y / TILE_SIZE);
        let layout = self.layout(tx, ty);
        for (c, p) in px.iter_mut().enumerate().take(self.channels as usize) {
            *p = match self.tile(tx, ty) {
                None => self.default,
                Some(t) => {
                    let i = layout
                        .index(c as u8, (x % TILE_SIZE) as i32, (y % TILE_SIZE) as i32)
                        .unwrap_or(0);
                    match t.format() {
                        TileFormat::U8 => {
                            f32::from(t.samples::<u8>().map(|s| s[i]).unwrap_or(0)) / 255.0
                        }
                        TileFormat::U16 => {
                            f32::from(t.samples::<u16>().map(|s| s[i]).unwrap_or(0)) / 65535.0
                        }
                        TileFormat::F32Planar => t.samples::<f32>().map(|s| s[i]).unwrap_or(0.0),
                        TileFormat::F16Planar => t
                            .samples::<half::f16>()
                            .map(|s| s[i].to_f32())
                            .unwrap_or(0.0),
                    }
                }
            };
        }
        px
    }

    /// Computes new tiles for every tile touched by `rect` without
    /// modifying `self`: `f(x, y, pixel)` edits normalized channels in
    /// place. Returns `(tx, ty, tile)` deltas, suitable for a paint op.
    pub fn render_region(
        &self,
        rect: Rect,
        mut f: impl FnMut(u32, u32, &mut [f32; 4]),
    ) -> EngineResult<Vec<(u32, u32, Tile)>> {
        let r = rect.intersect(&Rect::of_extent(self.extent));
        let mut out = Vec::new();
        if r.is_empty() {
            return Ok(out);
        }
        let ts = i64::from(TILE_SIZE);
        let mut buf = Vec::new();
        for ty in (r.y0 / ts) as u32..=((r.y1 - 1) / ts) as u32 {
            for tx in (r.x0 / ts) as u32..=((r.x1 - 1) / ts) as u32 {
                let layout = self.layout(tx, ty);
                self.read_tile(tx, ty, &mut buf)?;
                let plane = layout.plane_len();
                let (ox, oy) = (i64::from(tx) * ts, i64::from(ty) * ts);
                let tr = r.intersect(&Rect::new(
                    ox,
                    oy,
                    ox + i64::from(layout.extent.width),
                    oy + i64::from(layout.extent.height),
                ));
                for y in tr.y0..tr.y1 {
                    for x in tr.x0..tr.x1 {
                        let i = ((y - oy) as usize) * layout.stride() + (x - ox) as usize;
                        let mut px = [0.0f32; 4];
                        for c in 0..self.channels as usize {
                            px[c] = buf[c * plane + i];
                        }
                        f(x as u32, y as u32, &mut px);
                        for c in 0..self.channels as usize {
                            buf[c * plane + i] = px[c];
                        }
                    }
                }
                let tile =
                    tile_from_normalized(TileCoord::new(0, tx, ty), layout, self.depth, &buf)?;
                out.push((tx, ty, tile));
            }
        }
        Ok(out)
    }

    /// Applies `f` over `rect` in place with revision `rev` (convenience for
    /// building documents; edits of a live document go through ops).
    pub fn edit_region(
        &mut self,
        rect: Rect,
        rev: u64,
        f: impl FnMut(u32, u32, &mut [f32; 4]),
    ) -> EngineResult<()> {
        for (tx, ty, t) in self.render_region(rect, f)? {
            self.set_slot(tx, ty, Some(t), rev)?;
        }
        Ok(())
    }

    /// Union of stored tile rectangles (tombstones excluded), or `None`.
    pub fn bounds(&self) -> Option<Rect> {
        let mut r = Rect::default();
        for ((tx, ty), s) in self.slots() {
            if s.tile.is_some() {
                let l = self.layout(tx, ty);
                let (x0, y0) = (i64::from(tx * TILE_SIZE), i64::from(ty * TILE_SIZE));
                r = r.union(&Rect::new(
                    x0,
                    y0,
                    x0 + i64::from(l.extent.width),
                    y0 + i64::from(l.extent.height),
                ));
            }
        }
        (!r.is_empty()).then_some(r)
    }

    /// Stored tiles (tombstones excluded).
    pub fn tile_count(&self) -> usize {
        self.slots.values().filter(|s| s.tile.is_some()).count()
    }

    /// True if every tile of `self` shares its buffer with the same slot of
    /// `other` (used by COW tests).
    pub fn shares_all_tiles_with(&self, other: &Raster) -> bool {
        self.slots.len() == other.slots.len()
            && self
                .slots
                .iter()
                .all(|(k, s)| match (other.slots.get(k), &s.tile) {
                    (Some(o), Some(t)) => {
                        o.tile.as_ref().is_some_and(|ot| ot.shares_buffer_with(t))
                    }
                    (Some(o), None) => o.tile.is_none(),
                    _ => false,
                })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_and_read_back() {
        let mut r = Raster::new(Extent::new(300, 260), 4, Depth::U8, 0.0);
        r.edit_region(Rect::new(250, 240, 270, 250), 7, |_, _, p| {
            *p = [1.0, 0.5, 0.25, 1.0]
        })
        .unwrap();
        assert_eq!(r.tile_count(), 2);
        let p = r.pixel(260, 245);
        assert_eq!(p[0], 1.0);
        assert!((p[1] - 128.0 / 255.0).abs() < 1e-6);
        assert_eq!(r.pixel(10, 10), [0.0; 4]);
        assert_eq!(r.footprint_rev(0, 1, 0), 7);
        assert_eq!(r.footprint_rev(1, 0, 0), 7);
        assert_eq!(r.footprint_rev(0, 1, 1), 0);
        assert_eq!(r.bounds(), Some(Rect::new(0, 0, 300, 256)));
    }
}
