//! Dense (row-major) views of tiled compositor rasters, used where a tool
//! needs random access to a region: stroke compositing, selections,
//! resampling and merges.
use compositor::{Depth, Raster, Rect, TileDelta};
use engine_api::tile::{Extent, TILE_SIZE};
use engine_api::{EngineError, EngineResult};

/// A rectangle of straight RGBA (or single-channel, in `[0]`) pixels.
pub(crate) struct Dense {
    pub rect: Rect,
    pub px: Vec<[f32; 4]>,
}

impl Dense {
    pub fn width(&self) -> usize {
        self.rect.width().max(0) as usize
    }

    /// Pixel at canvas `(x, y)`, or `outside` when outside the rectangle.
    #[inline]
    pub fn get(&self, x: i64, y: i64, outside: [f32; 4]) -> [f32; 4] {
        let r = &self.rect;
        if x < r.x0 || y < r.y0 || x >= r.x1 || y >= r.y1 {
            return outside;
        }
        self.px[(y - r.y0) as usize * self.width() + (x - r.x0) as usize]
    }
}

/// Reads `rect` (clipped to the raster) as dense normalized pixels.
pub(crate) fn read(raster: &Raster, rect: Rect) -> EngineResult<Dense> {
    let rect = rect.intersect(&Rect::of_extent(raster.extent()));
    let (w, h) = (rect.width().max(0) as usize, rect.height().max(0) as usize);
    let def = raster.default_value();
    let mut px = vec![[def; 4]; w * h];
    let channels = raster.channels() as usize;
    for p in &mut px {
        for v in p.iter_mut().skip(channels) {
            *v = 0.0;
        }
    }
    if rect.is_empty() {
        return Ok(Dense { rect, px });
    }
    let ts = i64::from(TILE_SIZE);
    let mut buf = Vec::new();
    for ty in (rect.y0 / ts) as u32..=((rect.y1 - 1) / ts) as u32 {
        for tx in (rect.x0 / ts) as u32..=((rect.x1 - 1) / ts) as u32 {
            if raster.tile(tx, ty).is_none() {
                continue;
            }
            raster.read_tile(tx, ty, &mut buf)?;
            let layout = raster.layout(tx, ty);
            let plane = layout.plane_len();
            let stride = layout.stride();
            let (ox, oy) = (i64::from(tx) * ts, i64::from(ty) * ts);
            let tr = rect.intersect(&Rect::new(
                ox,
                oy,
                ox + i64::from(layout.extent.width),
                oy + i64::from(layout.extent.height),
            ));
            for y in tr.y0..tr.y1 {
                for x in tr.x0..tr.x1 {
                    let i = (y - oy) as usize * stride + (x - ox) as usize;
                    let o = &mut px[(y - rect.y0) as usize * w + (x - rect.x0) as usize];
                    for (c, v) in o.iter_mut().enumerate().take(channels) {
                        *v = buf[c * plane + i];
                    }
                }
            }
        }
    }
    Ok(Dense { rect, px })
}

/// Tile deltas that make `rect` of `raster` equal `f(x, y, current)`;
/// pixels outside `rect` are unchanged.
pub(crate) fn deltas(
    raster: &Raster,
    rect: Rect,
    f: impl FnMut(u32, u32, &mut [f32; 4]),
) -> EngineResult<Vec<TileDelta>> {
    Ok(raster
        .render_region(rect, f)?
        .into_iter()
        .map(|(tx, ty, tile)| TileDelta {
            tx,
            ty,
            tile: Some(tile),
        })
        .collect())
}

/// A single-channel float raster holding `dense` (everything else reads
/// `default`).
pub(crate) fn mask_raster(
    extent: Extent,
    depth: Depth,
    default: f32,
    dense: &Dense,
) -> EngineResult<Raster> {
    let mut r = Raster::new(extent, 1, depth, default);
    let w = dense.width();
    let rect = dense.rect;
    r.edit_region(rect, 0, |x, y, p| {
        p[0] =
            dense.px[(i64::from(y) - rect.y0) as usize * w + (i64::from(x) - rect.x0) as usize][0];
    })?;
    Ok(r)
}

/// Copies a single-channel raster into a new one of another depth (for
/// example an f32 selection into a document-depth layer mask).
pub(crate) fn convert_mask(src: &Raster, depth: Depth) -> EngineResult<Raster> {
    if src.channels() != 1 {
        return Err(EngineError::internal("mask conversion needs one channel"));
    }
    let mut out = Raster::new(src.extent(), 1, depth, src.default_value());
    let tiles: Vec<(u32, u32)> = src
        .slots()
        .filter(|(_, s)| s.tile.is_some())
        .map(|(k, _)| k)
        .collect();
    let mut buf = Vec::new();
    for (tx, ty) in tiles {
        src.read_tile(tx, ty, &mut buf)?;
        let layout = src.layout(tx, ty);
        let stride = layout.stride();
        let (ox, oy) = (i64::from(tx * TILE_SIZE), i64::from(ty * TILE_SIZE));
        let rect = Rect::new(
            ox,
            oy,
            ox + i64::from(layout.extent.width),
            oy + i64::from(layout.extent.height),
        );
        out.edit_region(rect, 0, |x, y, p| {
            p[0] = buf[(i64::from(y) - oy) as usize * stride + (i64::from(x) - ox) as usize];
        })?;
    }
    Ok(out)
}

/// Tight bounds of pixels whose channel `channel` exceeds zero (alpha
/// bounds of a layer), scanning stored tiles only.
pub(crate) fn content_bounds(raster: &Raster, channel: usize) -> EngineResult<Option<Rect>> {
    if raster.default_value() > 0.0 {
        return Ok(Some(Rect::of_extent(raster.extent())));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (i64::MAX, i64::MAX, i64::MIN, i64::MIN);
    let mut buf = Vec::new();
    for ((tx, ty), slot) in raster.slots() {
        if slot.tile.is_none() {
            continue;
        }
        raster.read_tile(tx, ty, &mut buf)?;
        let layout = raster.layout(tx, ty);
        let plane = layout.plane_len();
        let stride = layout.stride();
        let c = channel.min(raster.channels() as usize - 1);
        let (ox, oy) = (i64::from(tx * TILE_SIZE), i64::from(ty * TILE_SIZE));
        for y in 0..layout.extent.height as usize {
            let row =
                &buf[c * plane + y * stride..c * plane + y * stride + layout.extent.width as usize];
            let (Some(first), Some(last)) = (
                row.iter().position(|v| *v > 0.0),
                row.iter().rposition(|v| *v > 0.0),
            ) else {
                continue;
            };
            x0 = x0.min(ox + first as i64);
            x1 = x1.max(ox + last as i64 + 1);
            y0 = y0.min(oy + y as i64);
            y1 = y1.max(oy + y as i64 + 1);
        }
    }
    Ok((x1 > x0).then(|| Rect::new(x0, y0, x1, y1)))
}
