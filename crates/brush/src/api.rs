//! Adapters from the engine-api 1.2 `paint_stroke` tool.

use compositor::{Raster, Rect};
use engine_api::document::{BrushMode, BrushParams, StrokePoint};
use engine_api::tile::Tile;
use engine_api::{EngineError, EngineResult};

use crate::dynamics::Control;
use crate::engine::{Brush, PaintMode, Stroke};
use crate::stroke::InputPoint;
use crate::tip::Tip;

/// Dirty rect and replaced tiles `(tx, ty, tile)` of a stroke.
pub type StrokeTiles = (Rect, Vec<(u32, u32, Tile)>);

/// Start a clone/heal stroke whose source offset is measured in the unfolded
/// Vanishing Point atlas. Input points and brush footprints remain in canvas
/// pixels. The caller can use `PreparedVanishingPoint::plane_stroke` for atlas
/// dab centers. This adapter snapshots a plane-mapped source before painting,
/// so repeated shading cannot feed painted pixels back into the clone source.
///
/// The ordinary CloneSource offset must be zero (avoids ambiguous double offsets).
/// Source samples are bilinear in premultiplied RGBA, then converted to the
/// brush engine's straight RGBA. The valid plane coverage intersects selection.
/// Preparation is O(canvas pixels), currently limited to 16 MP. Clone/heal
/// rasterization, dynamics, history tiles and Poisson blending stay in Stroke.
pub fn vanishing_point_stroke(
    base: &Raster,
    selection: Option<&Raster>,
    mut brush: Brush,
    document: &transform::vanishing::VanishingPoint,
    offset: transform::Point,
    seed: u64,
) -> EngineResult<Stroke> {
    use crate::pixels::Pixels;
    use compositor::Depth;
    let invalid = |s: &str| EngineError::invalid("vanishing_point", s);
    brush.validate()?;
    if offset.iter().any(|v| !v.is_finite()) {
        return Err(invalid("non-finite atlas offset"));
    }
    let clone = match &mut brush.mode {
        PaintMode::Clone(c) | PaintMode::Heal(c) => c,
        _ => return Err(invalid("clone or heal brush required")),
    };
    if clone.offset != [0.; 2] {
        return Err(invalid(
            "canvas clone offset must be zero; use atlas offset",
        ));
    }
    let source = clone.source.as_ref().unwrap_or(base);
    if !matches!(source.channels(), 1 | 3 | 4) || !matches!(base.channels(), 1 | 3 | 4) {
        return Err(invalid("source and target require 1, 3 or 4 channels"));
    }
    let extent = base.extent();
    let n = u64::from(extent.width) * u64::from(extent.height);
    if n == 0 || n > 16_000_000 {
        return Err(invalid("nonempty canvas of at most 16 MP required"));
    }
    if selection.is_some_and(|s| s.channels() != 1 || s.extent() != extent) {
        return Err(invalid("selection must be single-channel and canvas-sized"));
    }
    let ready = document.prepare().map_err(|e| invalid(&e.to_string()))?;
    let mut pixels = Pixels::new(source);
    let mut selected = selection.map(Pixels::new);
    let mut mapped = Raster::new(extent, 4, Depth::F32, 0.);
    let mut mask = Raster::new(extent, 1, Depth::F32, 0.);
    let bounds = Rect::of_extent(extent);
    mapped.edit_region(bounds, 0, |x, y, p| {
        *p = ready
            .clone_source([f64::from(x) + 0.5, f64::from(y) + 0.5], offset)
            .map(|q| plane_sample(&mut pixels, q))
            .unwrap_or([0.; 4]);
    })?;
    mask.edit_region(bounds, 0, |x, y, p| {
        let valid = ready
            .clone_source([f64::from(x) + 0.5, f64::from(y) + 0.5], offset)
            .is_some();
        p[0] = if valid {
            selected
                .as_mut()
                .map(|s| s.get(i64::from(x), i64::from(y))[0].clamp(0., 1.))
                .unwrap_or(1.)
        } else {
            0.
        };
    })?;
    clone.source = Some(mapped);
    Stroke::new(brush, base, seed)?.with_selection(&mask)
}

fn plane_sample(pixels: &mut crate::pixels::Pixels, q: transform::Point) -> [f32; 4] {
    let extent = pixels.raster().extent();
    if q.iter().any(|v| !v.is_finite())
        || q[0] < -1.
        || q[1] < -1.
        || q[0] > f64::from(extent.width) + 1.
        || q[1] > f64::from(extent.height) + 1.
    {
        return [0.; 4];
    }
    let ch = pixels.raster().channels();
    let x = (q[0] - 0.5).floor() as i64;
    let y = (q[1] - 0.5).floor() as i64;
    let u = (q[0] - 0.5 - x as f64) as f32;
    let v = (q[1] - 0.5 - y as f64) as f32;
    let mut rgba = [0.; 4];
    for (dx, dy, w) in [
        (0, 0, (1. - u) * (1. - v)),
        (1, 0, u * (1. - v)),
        (0, 1, (1. - u) * v),
        (1, 1, u * v),
    ] {
        let (sx, sy) = (x + dx, y + dy);
        if sx < 0 || sy < 0 || sx >= i64::from(extent.width) || sy >= i64::from(extent.height) {
            continue;
        }
        let p = pixels.get(sx, sy);
        let a = if ch == 4 { p[3] } else { 1. };
        for c in 0..3 {
            rgba[c] += w * a * p[if ch == 1 { 0 } else { c }];
        }
        rgba[3] += w * a;
    }
    if rgba[3] > 0. {
        for c in 0..3 {
            rgba[c] /= rgba[3];
        }
    } else {
        rgba = [0.; 4];
    }
    rgba
}

impl From<StrokePoint> for InputPoint {
    fn from(p: StrokePoint) -> Self {
        InputPoint::at(p.x, p.y).pressure(p.pressure)
    }
}

/// The engine brush for tool parameters.
pub fn brush_from_params(p: &BrushParams) -> Brush {
    let mut b = Brush {
        tip: Tip::round(p.hardness),
        size: p.size,
        spacing: p.spacing,
        flow: p.flow,
        opacity: p.opacity,
        color: p.color,
        mode: match p.mode {
            BrushMode::Paint => PaintMode::Paint,
            BrushMode::Erase => PaintMode::Erase,
        },
        ..Brush::default()
    };
    if p.pressure_size {
        b.dynamics.size.control = Control::Pressure;
    }
    if p.pressure_flow {
        b.dynamics.flow.control = Control::Pressure;
    }
    b
}

/// Rasterizes a whole `paint_stroke` call over `base` (a layer's pixels or
/// mask), limited by `selection`. Returns the dirty rect and the replaced
/// tiles, ready for `DocOp::PaintTiles`.
pub fn paint_stroke(
    base: &Raster,
    selection: Option<&Raster>,
    params: &BrushParams,
    points: &[StrokePoint],
    seed: u64,
) -> EngineResult<Option<StrokeTiles>> {
    if points.is_empty() {
        return Err(EngineError::invalid("points", "at least one point"));
    }
    let mut s = Stroke::new(brush_from_params(params), base, seed)?;
    if let Some(sel) = selection {
        s = s.with_selection(sel)?;
    }
    for p in points {
        s.add_point((*p).into())?;
    }
    s.finish()?;
    let Some(d) = s.dirty() else {
        return Ok(None);
    };
    let tiles = s.render_tiles(d)?;
    Ok(Some((d, tiles)))
}
