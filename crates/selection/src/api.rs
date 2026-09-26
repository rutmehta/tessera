//! Adapter for the engine-api 1.2 `set_pixel_selection` tool.
//!
//! `DocState::selection == None` means "everything is editable"; so do
//! `SelectionShape::All` and `None` here (the document keeps no mask).

use compositor::{Depth, Raster};
use engine_api::document::{SelectionMode, SelectionShape};
use engine_api::tile::Extent;
use engine_api::{EngineError, EngineResult};

use crate::marquee::{ellipse, polygon, rect};
use crate::mask::Mask;
use crate::ops::{Combine, combine, feather, invert};

/// The document selection as a mask (`None` = all selected).
pub fn current_mask(selection: Option<&Raster>, extent: Extent) -> EngineResult<Mask> {
    match selection {
        Some(r) => Mask::from_raster(r),
        None => Ok(Mask::filled(extent.width, extent.height, 1.0)),
    }
}

/// Applies a selection change. `resolve` supplies masks for
/// `LayerAlpha`/`Saved` shapes, which need the document. Returns the new
/// document selection (`None` = no selection, everything editable).
pub fn set_pixel_selection(
    current: Option<&Raster>,
    extent: Extent,
    shape: &SelectionShape,
    mode: SelectionMode,
    feather_radius: f32,
    resolve: &mut dyn FnMut(&SelectionShape) -> EngineResult<Mask>,
) -> EngineResult<Option<Raster>> {
    if !feather_radius.is_finite() || feather_radius < 0.0 {
        return Err(EngineError::invalid("feather", "must be ≥ 0"));
    }
    let (w, h) = (extent.width, extent.height);
    let box4 =
        |r: &engine_api::document::CanvasRect| [r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32];
    let new = match shape {
        SelectionShape::All | SelectionShape::None => return Ok(None),
        SelectionShape::Inverse => {
            let cur = current_mask(current, extent)?;
            return Ok(Some(invert(&cur).to_raster(Depth::F32)?));
        }
        SelectionShape::Rect { rect: r } => rect(w, h, box4(r), true),
        SelectionShape::Ellipse { rect: r } => ellipse(w, h, box4(r), true),
        SelectionShape::Polygon { points } => {
            if points.len() < 3 {
                return Err(EngineError::invalid("points", "polygon needs 3 vertices"));
            }
            polygon(w, h, points, true)
        }
        s @ (SelectionShape::LayerAlpha { .. } | SelectionShape::Saved { .. }) => {
            let m = resolve(s)?;
            if m.extent() != extent {
                return Err(EngineError::invalid("selection", "extent mismatch"));
            }
            m
        }
    };
    let new = feather(&new, feather_radius);
    let out = match (mode, current) {
        (SelectionMode::Replace, _) | (SelectionMode::Add, None) => {
            if mode == SelectionMode::Add && current.is_none() {
                // Adding to "everything" stays everything.
                return Ok(None);
            }
            new
        }
        (m, _) => {
            let cur = current_mask(current, extent)?;
            let op = match m {
                SelectionMode::Add => Combine::Add,
                SelectionMode::Subtract => Combine::Subtract,
                _ => Combine::Intersect,
            };
            combine(&cur, &new, op)?
        }
    };
    Ok(Some(out.to_raster(Depth::F32)?))
}
