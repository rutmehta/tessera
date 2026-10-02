//! Develop recipe adapter over the existing clone/heal stroke kernels.
use crate::{Brush, CloneSource, InputPoint, PaintMode, Stroke, Tip};
use compositor::{Depth, Raster, Rect};
use engine_api::{
    EngineError, EngineResult,
    recipe::{
        MaskKind,
        mask::{MaskCombine, RetouchKind, RetouchOperation, RetouchTarget},
    },
    tile::{Extent, TILE_SIZE},
};

/// Apply explicit-source brush spots in recipe order to scene-linear planar RGB.
/// Unsupported targets/kinds fail before publishing pixels to the caller.
pub fn render_retouch(
    width: u32,
    height: u32,
    planes: &mut [Vec<f32>],
    spots: &[RetouchOperation],
) -> EngineResult<()> {
    let invalid = || {
        EngineError::invalid(
            "retouch",
            "only additive explicit-source clone/heal brush spots with zero operation feather are supported",
        )
    };
    let n = width as usize * height as usize;
    if width == 0 || height == 0 || planes.len() != 3 || planes.iter().any(|p| p.len() != n) {
        return Err(EngineError::invalid(
            "retouch",
            "expected nonempty planar RGB",
        ));
    }
    let extent = Extent::new(width, height);
    let rect = Rect::of_extent(extent);
    let mut raster = Raster::new(extent, 3, Depth::F32, 0.0);
    raster.edit_region(rect, 1, |x, y, pixel| {
        let i = y as usize * width as usize + x as usize;
        *pixel = [planes[0][i], planes[1][i], planes[2][i], 1.0];
    })?;
    for op in spots.iter().filter(|op| op.enabled) {
        if !op.opacity.is_finite() || !(0.0..=100.0).contains(&op.opacity) || op.feather != 0.0 {
            return Err(invalid());
        }
        let (offset, heal) = match op.kind {
            RetouchKind::Clone { source_offset } => (source_offset, false),
            RetouchKind::Heal { source_offset } => (source_offset, true),
            _ => return Err(invalid()),
        };
        let RetouchTarget::Area { components } = &op.target else {
            return Err(invalid());
        };
        if components.is_empty() {
            return Err(invalid());
        }
        let mut union: Option<Stroke> = None;
        for component in components {
            if component.invert || component.combine != MaskCombine::Add {
                return Err(invalid());
            }
            let MaskKind::Brush { strokes } = &component.kind else {
                return Err(invalid());
            };
            if strokes.is_empty() {
                return Err(invalid());
            }
            for stroke in strokes {
                if stroke.erase
                    || stroke.points.is_empty()
                    || !stroke.radius.is_finite()
                    || stroke.radius <= 0.0
                    || !stroke.feather.is_finite()
                    || !(0.0..=100.0).contains(&stroke.feather)
                    || !stroke.flow.is_finite()
                    || !(0.0..=100.0).contains(&stroke.flow)
                    || stroke
                        .points
                        .iter()
                        .any(|p| p.iter().any(|v| !v.is_finite()) || !(0.0..=1.0).contains(&p[2]))
                {
                    return Err(invalid());
                }
                let source = CloneSource {
                    offset: [offset[0] * width as f32, offset[1] * height as f32],
                    source: None,
                };
                let brush = Brush {
                    size: stroke.radius * width as f32 * 2.0,
                    opacity: op.opacity / 100.0,
                    flow: stroke.flow / 100.0,
                    tip: Tip::round(1.0 - stroke.feather / 100.0),
                    // Rasterize all paths before solving/compositing the spot.
                    mode: PaintMode::Clone(source),
                    ..Brush::default()
                };
                let mut render = Stroke::new(brush, &raster, 1)?;
                for p in &stroke.points {
                    render.add_point(
                        InputPoint::at(p[0] * width as f32, p[1] * height as f32).pressure(p[2]),
                    )?;
                }
                render.finish()?;
                if let Some(union) = &mut union {
                    union.union_retouch_mask(&render);
                } else {
                    union = Some(render);
                }
            }
        }
        if let Some(mut union) = union
            && let Some(dirty) = union.finish_retouch(heal)
        {
            union.apply(&mut raster, dirty, 2)?;
        }
    }
    // Reuse one tile-sized buffer; do not decode the same tile per pixel or
    // allocate a second full-frame planar result.
    let mut buffer = Vec::new();
    for ty in 0..height.div_ceil(TILE_SIZE) {
        for tx in 0..width.div_ceil(TILE_SIZE) {
            raster.read_tile(tx, ty, &mut buffer)?;
            let layout = raster.layout(tx, ty);
            for (c, plane) in planes.iter_mut().enumerate() {
                for y in 0..layout.extent.height {
                    let dest = ((ty * TILE_SIZE + y) * width + tx * TILE_SIZE) as usize;
                    let source = c * layout.plane_len() + y as usize * layout.stride();
                    let count = layout.extent.width as usize;
                    plane[dest..dest + count].copy_from_slice(&buffer[source..source + count]);
                }
            }
        }
    }
    Ok(())
}
