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
