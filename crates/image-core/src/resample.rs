//! Halo gathering across tile seams and the linear-light crop/downsample.
//!
//! Both reproduce `pipeline_cpu::Image` sample for sample so tiled renders
//! are bit-identical to the reference renderer on a cold cache.

use std::collections::{BTreeSet, HashMap};

use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord, TileLayout};
use engine_api::{EngineError, EngineResult};

/// Maps a possibly out-of-range coordinate to the nearest in-range one with
/// the same CFA phase (period 2 Bayer, 6 X-Trans, 1 RGB). Identical to the
/// rule in `pipeline_cpu::Image::tile`.
fn clamp_phase(v: i64, n: u32, period: u32) -> u32 {
    if v >= 0 && v < i64::from(n) {
        return v as u32;
    }
    let phase = v.rem_euclid(i64::from(period)) as u32;
    if phase >= n {
        return v.clamp(0, i64::from(n) - 1) as u32;
    }
    if v < 0 {
        phase
    } else {
        phase + (n - 1 - phase) / period * period
    }
}

/// Source positions, per sample along one axis of a haloed tile.
fn axis(origin: u32, len: u32, halo: u16, n: u32, period: u32) -> Vec<u32> {
    let start = i64::from(origin) - i64::from(halo);
    (0..len as i64 + 2 * i64::from(halo))
        .map(|i| clamp_phase(start + i, n, period))
        .collect()
}

/// Interior extent of a level-0 tile in a frame of size `frame`.
pub(crate) fn interior(frame: Extent, coord: TileCoord) -> Extent {
    let (ox, oy) = coord.pixel_origin(TILE_SIZE);
    Extent::new(
        (frame.width - ox).min(TILE_SIZE),
        (frame.height - oy).min(TILE_SIZE),
    )
}

/// Level-0 tiles of `frame` that [`gather`] reads for `coord` with `halo`.
pub(crate) fn gather_sources(
    frame: Extent,
    coord: TileCoord,
    halo: u16,
    period: u32,
) -> Vec<TileCoord> {
    let e = interior(frame, coord);
    let (ox, oy) = coord.pixel_origin(TILE_SIZE);
    let cols: BTreeSet<u32> = axis(ox, e.width, halo, frame.width, period)
        .into_iter()
        .map(|v| v / TILE_SIZE)
        .collect();
    let rows: BTreeSet<u32> = axis(oy, e.height, halo, frame.height, period)
        .into_iter()
        .map(|v| v / TILE_SIZE)
        .collect();
    rows.iter()
        .flat_map(|&y| cols.iter().map(move |&x| TileCoord::new(0, x, y)))
        .collect()
}

/// Assembles the level-0 tile `coord` with `halo` from halo-free `F32`
/// neighbour tiles, clamping to the same CFA phase at the frame edges.
pub(crate) fn gather(
    frame: Extent,
    coord: TileCoord,
    halo: u16,
    period: u32,
    tiles: &HashMap<TileCoord, Tile>,
) -> EngineResult<Tile> {
    let e = interior(frame, coord);
    let (ox, oy) = coord.pixel_origin(TILE_SIZE);
    let xs = axis(ox, e.width, halo, frame.width, period);
    let ys = axis(oy, e.height, halo, frame.height, period);
    let lookup = |x: u32, y: u32| -> EngineResult<&Tile> {
        tiles
            .get(&TileCoord::new(0, x / TILE_SIZE, y / TILE_SIZE))
            .ok_or_else(|| EngineError::internal(format!("gather for {coord}: source missing")))
    };
    let channels = lookup(xs[0], ys[0])?.layout().channels;
    let layout = TileLayout {
        extent: e,
        halo,
        channels,
    };
    let mut data = vec![0.0f32; layout.len()];
    let plane = layout.plane_len();
    // Split each row into runs that come from one source tile.
    let mut runs: Vec<(u32, usize, usize)> = Vec::new(); // (tile col, first x index, count)
    for (i, &x) in xs.iter().enumerate() {
        match runs.last_mut() {
            Some((col, first, count))
                if *col == x / TILE_SIZE && xs[*first + *count - 1] + 1 == x =>
            {
                *count += 1;
            }
            _ => runs.push((x / TILE_SIZE, i, 1)),
        }
    }
    for (row, &sy) in ys.iter().enumerate() {
        for &(col, first, count) in &runs {
            let src = lookup(col * TILE_SIZE, sy)?;
            let sl = src.layout();
            if sl.channels != channels || sl.halo != 0 {
                return Err(EngineError::internal("gather: inconsistent source tiles"));
            }
            let samples = src.samples::<f32>()?;
            let sx0 = (xs[first] % TILE_SIZE) as usize;
            let row_off = (sy % TILE_SIZE) as usize * sl.stride();
            for c in 0..channels as usize {
                let from = c * sl.plane_len() + row_off + sx0;
                let to = c * plane + row * layout.stride() + first;
                data[to..to + count].copy_from_slice(&samples[from..from + count]);
            }
        }
    }
    Tile::from_samples(coord, layout, data)
}

/// Active-area crop `[left, top, width, height]` in sensor pixels.
pub(crate) type Crop = [u32; 4];

/// Sensor-frame, level-0 tiles covering the source block of output tile
/// `coord` (level `L` of the active-area pyramid).
pub(crate) fn resample_sources(crop: Crop, coord: TileCoord) -> Vec<TileCoord> {
    let (x0, x1, y0, y1) = source_rect(crop, coord);
    let (c0, c1) = (x0 / TILE_SIZE, (x1 - 1) / TILE_SIZE);
    let (r0, r1) = (y0 / TILE_SIZE, (y1 - 1) / TILE_SIZE);
    (r0..=r1)
        .flat_map(|y| (c0..=c1).map(move |x| TileCoord::new(0, x, y)))
        .collect()
}

/// Output-tile extent at its level.
pub(crate) fn output_extent(crop: Crop, coord: TileCoord) -> Extent {
    interior(Extent::new(crop[2], crop[3]).at_level(coord.level), coord)
}

/// Half-open sensor rectangle `(x0, x1, y0, y1)` read by an output tile.
fn source_rect(crop: Crop, coord: TileCoord) -> (u32, u32, u32, u32) {
    let [left, top, w, h] = crop;
    let s = 1u32 << coord.level;
    let e = output_extent(crop, coord);
    let (ox, oy) = coord.pixel_origin(TILE_SIZE);
    let x0 = ox * s;
    let y0 = oy * s;
    let x1 = ((ox + e.width) * s).min(w);
    let y1 = ((oy + e.height) * s).min(h);
    (left + x0, left + x1, top + y0, top + y1)
}

/// Output tile `coord` as the box average of `2^level` blocks of the crop,
/// in the summation order of `pipeline_cpu::Image::downsample_crop` (rows
/// outer, columns inner, one f32 accumulator per sample, partial edge blocks
/// divided by their own sample count).
pub(crate) fn resample(
    crop: Crop,
    coord: TileCoord,
    tiles: &HashMap<TileCoord, Tile>,
) -> EngineResult<Tile> {
    let [left, top, w, h] = crop;
    let s = 1u32 << coord.level;
    let e = output_extent(crop, coord);
    let (ox, oy) = coord.pixel_origin(TILE_SIZE);
    let (sx0, sx1, _, _) = source_rect(crop, coord);
    let n = e.area() as usize;
    let mut acc = vec![0.0f32; 3 * n];
    // Per source column: (tile column, offset within tile).
    let cols: Vec<(u32, usize)> = (sx0..sx1)
        .map(|x| (x / TILE_SIZE, (x % TILE_SIZE) as usize))
        .collect();
    for y in 0..e.height {
        let by0 = (oy + y) * s;
        let by1 = (by0 + s).min(h);
        for sy in by0..by1 {
            let row = top + sy;
            let (ty, off_y) = (row / TILE_SIZE, (row % TILE_SIZE) as usize);
            // Tiles of this sensor row, fetched lazily per tile column.
            let mut current: Option<(u32, &Tile, &[f32])> = None;
            for x in 0..e.width {
                let bx0 = (ox + x) * s;
                let bx1 = (bx0 + s).min(w);
                for sx in bx0..bx1 {
                    let (tx, off_x) = cols[(left + sx - sx0) as usize];
                    let (tile, samples) = match current {
                        Some((cx, t, d)) if cx == tx => (t, d),
                        _ => {
                            let t = tiles.get(&TileCoord::new(0, tx, ty)).ok_or_else(|| {
                                EngineError::internal(format!("resample {coord}: source missing"))
                            })?;
                            let d = t.samples::<f32>()?;
                            current = Some((tx, t, d));
                            (t, d)
                        }
                    };
                    let l = tile.layout();
                    let i = off_y * l.stride() + off_x;
                    let o = (y * e.width + x) as usize;
                    for c in 0..3 {
                        acc[c * n + o] += samples[c * l.plane_len() + i];
                    }
                }
            }
        }
    }
    for y in 0..e.height {
        let by0 = (oy + y) * s;
        let rows = (by0 + s).min(h) - by0;
        for x in 0..e.width {
            let bx0 = (ox + x) * s;
            let count = ((bx0 + s).min(w) - bx0) * rows;
            let o = (y * e.width + x) as usize;
            for c in 0..3 {
                acc[c * n + o] /= count as f32;
            }
        }
    }
    Tile::from_samples(
        coord,
        TileLayout {
            extent: e,
            halo: 0,
            channels: 3,
        },
        acc,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase_clamp_matches_reference() {
        // Bayer: -1 -> 1, -2 -> 0, n -> n-2 (same parity), n+1 -> n-1.
        assert_eq!(clamp_phase(-1, 10, 2), 1);
        assert_eq!(clamp_phase(-2, 10, 2), 0);
        assert_eq!(clamp_phase(10, 10, 2), 8);
        assert_eq!(clamp_phase(11, 10, 2), 9);
        assert_eq!(clamp_phase(-1, 10, 1), 0);
        assert_eq!(clamp_phase(12, 10, 1), 9);
        for v in -7..20 {
            let c = clamp_phase(v, 13, 6);
            assert!(c < 13);
            if !(0..13).contains(&v) {
                assert_eq!((i64::from(c) - v).rem_euclid(6), 0);
            }
        }
    }

    #[test]
    fn sources_cover_halo_and_edges() {
        let frame = Extent::new(600, 300);
        let s = gather_sources(frame, TileCoord::new(0, 1, 0), 2, 2);
        assert_eq!(
            s,
            [0, 1, 2]
                .map(|x| TileCoord::new(0, x, 0))
                .to_vec()
                .into_iter()
                .chain([0, 1, 2].map(|x| TileCoord::new(0, x, 1)))
                .collect::<Vec<_>>()
        );
        // A 3-pixel-wide last column with X-Trans period reaches back a tile.
        let narrow = Extent::new(515, 10);
        let s = gather_sources(narrow, TileCoord::new(0, 2, 0), 3, 6);
        assert!(s.contains(&TileCoord::new(0, 1, 0)));
        let crop = [3, 5, 590, 290];
        assert_eq!(resample_sources(crop, TileCoord::new(1, 0, 0)).len(), 6);
        assert_eq!(
            output_extent(crop, TileCoord::new(1, 1, 0)),
            Extent::new(39, 145)
        );
    }
}
