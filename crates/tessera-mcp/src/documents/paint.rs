//! Pluggable brush strokes: [`RealBrush`] and [`PresetBrush`] return exact
//! replacement tiles from the brush crate; [`RoundBrush`] retains the legacy
//! scalar-coverage fallback. The executor enforces locks/targets, supplies the
//! stroke-start raster and selection, and commits one tile-delta history op.
//! Direct tiles must not be composited or selection-masked a second time.
use compositor::Rect;
use engine_api::EngineResult;
use engine_api::document::{BrushParams, StrokePoint};
use engine_api::tile::Extent;

/// Per-pixel stroke alpha in `[0, 1]` over `rect` (row-major), before the
/// selection is applied. Pixels outside `rect` are untouched.
#[derive(Debug, Clone, Default)]
pub struct StrokeCoverage {
    /// Level-0 canvas rectangle (within the canvas).
    pub rect: Rect,
    /// `rect.width() * rect.height()` alphas.
    pub alpha: Vec<f32>,
}

/// Paints directly into replacement tiles, or rasterizes legacy coverage.
/// Implementations must be deterministic: the same base, selection, points
/// and settings produce identical pixels (including the captured preset seed).
pub trait BrushEngine: Send + Sync {
    /// Engine name (reported by `describe_document`).
    fn name(&self) -> &str;
    /// Direct replacement tiles, already composited over `base` and limited by
    /// `selection`. `None` requests the legacy coverage path; a handled no-op
    /// returns `Some((Rect::default(), vec![]))`, never `None`.
    fn paint_tiles(
        &self,
        _base: &compositor::Raster,
        _selection: Option<&compositor::Raster>,
        _points: &[StrokePoint],
        _brush: &BrushParams,
    ) -> EngineResult<Option<brush::api::StrokeTiles>> {
        Ok(None)
    }

    /// Coverage of one stroke on a canvas of `canvas` pixels. `points` is
    /// non-empty and `brush` has been validated.
    fn rasterize(
        &self,
        points: &[StrokePoint],
        brush: &BrushParams,
        canvas: Extent,
    ) -> EngineResult<StrokeCoverage>;
}

/// The real brush crate adapter. Direct painting preserves the brush engine's
/// per-dab transfer, mask luminance, selection and quantization semantics.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealBrush;

impl BrushEngine for RealBrush {
    fn name(&self) -> &str {
        "brush"
    }

    fn paint_tiles(
        &self,
        base: &compositor::Raster,
        selection: Option<&compositor::Raster>,
        points: &[StrokePoint],
        params: &BrushParams,
    ) -> EngineResult<Option<brush::api::StrokeTiles>> {
        Ok(Some(
            brush::api::paint_stroke(base, selection, params, points, 0)?.unwrap_or_default(),
        ))
    }

    fn rasterize(
        &self,
        points: &[StrokePoint],
        params: &BrushParams,
        canvas: Extent,
    ) -> EngineResult<StrokeCoverage> {
        let mut base = compositor::Raster::new(canvas, 4, compositor::Depth::F32, 0.0);
        let mut params = *params;
        params.mode = engine_api::document::BrushMode::Paint;
        let Some((rect, tiles)) = brush::api::paint_stroke(&base, None, &params, points, 0)? else {
            return Ok(StrokeCoverage::default());
        };
        for (tx, ty, tile) in tiles {
            base.set_slot(tx, ty, Some(tile), 1)?;
        }
        let mut alpha = Vec::with_capacity((rect.width() * rect.height()) as usize);
        for y in rect.y0..rect.y1 {
            for x in rect.x0..rect.x1 {
                alpha.push(base.pixel(x as u32, y as u32)[3]);
            }
        }
        Ok(StrokeCoverage { rect, alpha })
    }
}

/// A full preset snapshot for one request. The MCP layer resolves a local
/// preset ID before constructing this engine; neither IDs nor mutable store
/// lookups participate in painting. `BrushParams` are deliberately ignored:
/// callers wanting overrides must apply them to the snapshot first.
#[derive(Debug, Clone)]
pub struct PresetBrush {
    brush: brush::Brush,
    seed: u64,
}

impl PresetBrush {
    /// Validates and captures all settings with a deterministic stroke seed.
    pub fn new(brush: brush::Brush, seed: u64) -> EngineResult<Self> {
        brush.validate()?;
        Ok(Self { brush, seed })
    }
}

impl BrushEngine for PresetBrush {
    fn name(&self) -> &str {
        "brush-preset"
    }

    fn paint_tiles(
        &self,
        base: &compositor::Raster,
        selection: Option<&compositor::Raster>,
        points: &[StrokePoint],
        _params: &BrushParams,
    ) -> EngineResult<Option<brush::api::StrokeTiles>> {
        if points.is_empty() {
            return Err(engine_api::EngineError::invalid(
                "points",
                "at least one point",
            ));
        }
        let mut stroke = brush::Stroke::new(self.brush.clone(), base, self.seed)?;
        if let Some(selection) = selection {
            stroke = stroke.with_selection(selection)?;
        }
        for point in points {
            stroke.add_point((*point).into())?;
        }
        stroke.finish()?;
        let Some(dirty) = stroke.dirty() else {
            return Ok(Some(Default::default()));
        };
        Ok(Some((dirty, stroke.render_tiles(dirty)?)))
    }

    fn rasterize(
        &self,
        _points: &[StrokePoint],
        _params: &BrushParams,
        _canvas: Extent,
    ) -> EngineResult<StrokeCoverage> {
        // Blend/clone/heal presets cannot be represented by scalar coverage.
        Err(engine_api::EngineError::Unsupported {
            what: "full brush presets require direct tile painting".into(),
        })
    }
}

/// A round tip stamped every `spacing · size` pixels along the path. Dab
/// alpha is `flow · falloff(distance)` (hardness sets where the smoothstep
/// falloff starts; hard tips get a one-pixel anti-aliased edge); dabs
/// accumulate as `a + d·(1 − a)`, and the stroke alpha is that times
/// `opacity`. Pressure optionally scales size and flow, interpolated
/// linearly between points.
#[derive(Debug, Clone, Copy, Default)]
pub struct RoundBrush;

struct Dab {
    x: f32,
    y: f32,
    radius: f32,
    flow: f32,
}

fn dabs(points: &[StrokePoint], b: &BrushParams) -> Vec<Dab> {
    let dab = |x: f32, y: f32, p: f32| Dab {
        x,
        y,
        radius: 0.5 * b.size * if b.pressure_size { p } else { 1.0 },
        flow: b.flow * if b.pressure_flow { p } else { 1.0 },
    };
    let p0 = points[0];
    let mut out = vec![dab(p0.x, p0.y, p0.pressure.clamp(0.0, 1.0))];
    // Distance travelled since the last dab.
    let mut carry = 0.0f32;
    for w in points.windows(2) {
        let (a, b2) = (w[0], w[1]);
        let (dx, dy) = (b2.x - a.x, b2.y - a.y);
        let len = (dx * dx + dy * dy).sqrt();
        if len <= 0.0 {
            continue;
        }
        let mut t = 0.0f32;
        loop {
            let pressure = (a.pressure + (b2.pressure - a.pressure) * (t / len)).clamp(0.0, 1.0);
            let size = b.size * if b.pressure_size { pressure } else { 1.0 };
            let step = (b.spacing * size).max(0.5);
            let next = t + step - carry;
            if next > len {
                carry += len - t;
                break;
            }
            t = next;
            carry = 0.0;
            let f = t / len;
            let p = (a.pressure + (b2.pressure - a.pressure) * f).clamp(0.0, 1.0);
            out.push(dab(a.x + dx * f, a.y + dy * f, p));
        }
    }
    out
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl BrushEngine for RoundBrush {
    fn name(&self) -> &str {
        "round-dab"
    }

    fn rasterize(
        &self,
        points: &[StrokePoint],
        brush: &BrushParams,
        canvas: Extent,
    ) -> EngineResult<StrokeCoverage> {
        let dabs = dabs(points, brush);
        let mut rect = Rect::default();
        for d in &dabs {
            let r = d.radius + 1.0;
            rect = rect.union(&Rect::new(
                (d.x - r).floor() as i64,
                (d.y - r).floor() as i64,
                (d.x + r).ceil() as i64 + 1,
                (d.y + r).ceil() as i64 + 1,
            ));
        }
        let rect = rect.intersect(&Rect::of_extent(canvas));
        if rect.is_empty() {
            return Ok(StrokeCoverage::default());
        }
        let w = rect.width() as usize;
        let mut alpha = vec![0.0f32; w * rect.height() as usize];
        for d in &dabs {
            if d.radius <= 0.0 || d.flow <= 0.0 {
                continue;
            }
            let inner = brush.hardness.clamp(0.0, 1.0) * d.radius;
            let soft = d.radius - inner;
            let bx = Rect::new(
                (d.x - d.radius - 1.0).floor() as i64,
                (d.y - d.radius - 1.0).floor() as i64,
                (d.x + d.radius + 1.0).ceil() as i64 + 1,
                (d.y + d.radius + 1.0).ceil() as i64 + 1,
            )
            .intersect(&rect);
            for y in bx.y0..bx.y1 {
                for x in bx.x0..bx.x1 {
                    let (px, py) = (x as f32 + 0.5 - d.x, y as f32 + 0.5 - d.y);
                    let dist = (px * px + py * py).sqrt();
                    let edge = (d.radius - dist + 0.5).clamp(0.0, 1.0);
                    if edge <= 0.0 {
                        continue;
                    }
                    let falloff = if soft < 1.0 || dist <= inner {
                        1.0
                    } else {
                        smoothstep((d.radius - dist) / soft)
                    };
                    let v = d.flow * falloff * edge;
                    let a = &mut alpha[(y - rect.y0) as usize * w + (x - rect.x0) as usize];
                    *a += v * (1.0 - *a);
                }
            }
        }
        let opacity = brush.opacity.clamp(0.0, 1.0);
        for a in &mut alpha {
            *a = (*a * opacity).clamp(0.0, 1.0);
        }
        Ok(StrokeCoverage { rect, alpha })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(x: f32, y: f32) -> StrokePoint {
        StrokePoint {
            x,
            y,
            pressure: 1.0,
        }
    }

    #[test]
    fn full_preset_engine_matches_seeded_stroke_without_losing_advanced_settings() {
        use brush::{Brush, SampledTip, Stroke, Tip};
        use compositor::{BlendMode, Depth, Raster};
        let base = Raster::new(Extent::new(40, 30), 4, Depth::U8, 0.4);
        let preset = Brush {
            tip: Tip::sampled(
                SampledTip::new("asymmetric", 2, 2, vec![0.0, 1.0, 0.5, 0.0]).unwrap(),
            ),
            size: 9.0,
            wet_edges: true,
            blend: BlendMode::Multiply,
            ..Brush::default()
        };
        let engine = PresetBrush::new(preset.clone(), 123).unwrap();
        let points = [pt(7.0, 9.0), pt(22.0, 16.0)];
        let actual = engine
            .paint_tiles(&base, None, &points, &BrushParams::default())
            .unwrap()
            .unwrap();
        let mut expected = Stroke::new(preset, &base, 123).unwrap();
        for p in points {
            expected.add_point(p.into()).unwrap();
        }
        expected.finish().unwrap();
        let rect = expected.dirty().unwrap();
        assert_eq!(actual.0, rect);
        let tiles = expected.render_tiles(rect).unwrap();
        assert_eq!(actual.1.len(), tiles.len());
        for ((_, _, a), (_, _, e)) in actual.1.iter().zip(&tiles) {
            assert_eq!(a.samples::<u8>().unwrap(), e.samples::<u8>().unwrap());
        }
        assert!(
            engine
                .paint_tiles(&base, None, &[], &BrushParams::default())
                .is_err()
        );
        let no_op = engine
            .paint_tiles(
                &base,
                None,
                &[pt(-1000.0, -1000.0)],
                &BrushParams::default(),
            )
            .unwrap()
            .unwrap();
        assert!(no_op.0.is_empty());
        assert!(no_op.1.is_empty());
    }

    #[test]
    fn direct_tiles_match_brush_api_exactly_and_legacy_declines() {
        use compositor::{Depth, Raster};
        use engine_api::document::BrushMode;
        let points = [
            pt(3.5, 8.0),
            StrokePoint {
                x: 22.0,
                y: 13.0,
                pressure: 0.4,
            },
        ];
        for channels in [1, 4] {
            for mode in [BrushMode::Paint, BrushMode::Erase] {
                let base = Raster::new(Extent::new(32, 24), channels, Depth::F32, 0.35);
                let selection = Raster::new(base.extent(), 1, Depth::F32, 0.6);
                let params = BrushParams {
                    size: 11.0,
                    hardness: 0.3,
                    flow: 0.4,
                    opacity: 0.7,
                    color: [0.8, 0.2, 0.1],
                    pressure_size: true,
                    pressure_flow: true,
                    mode,
                    ..Default::default()
                };
                assert!(
                    RoundBrush
                        .paint_tiles(&base, Some(&selection), &points, &params)
                        .unwrap()
                        .is_none()
                );
                let actual = RealBrush
                    .paint_tiles(&base, Some(&selection), &points, &params)
                    .unwrap()
                    .unwrap();
                let expected =
                    brush::api::paint_stroke(&base, Some(&selection), &params, &points, 0)
                        .unwrap()
                        .unwrap();
                assert_eq!(actual.0, expected.0);
                assert_eq!(actual.1.len(), expected.1.len());
                for ((ax, ay, a), (ex, ey, e)) in actual.1.iter().zip(&expected.1) {
                    assert_eq!((ax, ay), (ex, ey));
                    assert_eq!(a.samples::<f32>().unwrap(), e.samples::<f32>().unwrap());
                }
            }
        }
    }

    #[test]
    fn hard_dab_covers_its_disc_and_spacing_fills_a_line() {
        let brush = BrushParams {
            size: 10.0,
            ..Default::default()
        };
        let c = RoundBrush
            .rasterize(
                &[pt(20.0, 20.0), pt(60.0, 20.0)],
                &brush,
                Extent::new(100, 50),
            )
            .unwrap();
        let at = |x: i64, y: i64| {
            c.alpha[(y - c.rect.y0) as usize * c.rect.width() as usize + (x - c.rect.x0) as usize]
        };
        for x in 20..60 {
            assert!(at(x, 20) > 0.99, "gap at {x}");
        }
        assert_eq!(at(40, 26), 0.0);
        assert!(c.rect.x0 >= 13 && c.rect.x1 <= 68);
    }

    #[test]
    fn opacity_caps_and_soft_edges_fall_off() {
        let brush = BrushParams {
            size: 20.0,
            hardness: 0.0,
            opacity: 0.5,
            ..Default::default()
        };
        let c = RoundBrush
            .rasterize(&[pt(10.0, 10.0)], &brush, Extent::new(20, 20))
            .unwrap();
        let w = c.rect.width() as usize;
        let centre = c.alpha[(10 - c.rect.y0) as usize * w + (10 - c.rect.x0) as usize];
        let edge = c.alpha[(10 - c.rect.y0) as usize * w + (18 - c.rect.x0) as usize];
        assert!(centre <= 0.5 && centre > 0.45);
        assert!(edge < centre * 0.2);
    }
}
