//! Brush strokes: the pluggable [`BrushEngine`] that turns stroke geometry
//! into coverage, and the built-in [`RoundBrush`] (a round dab with
//! hardness falloff) used until the brush crate is linked.
//!
//! Engines only produce coverage. The executor composites it (paint or
//! erase, pixels or mask), limits it by the selection and records the
//! result as one tile-delta op, so every engine gets history, locks and
//! selection handling for free.
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

/// Rasterizes stroke geometry into coverage. Implementations must be
/// deterministic: the same points and parameters give the same coverage
/// (recorded Actions replay to identical pixels).
pub trait BrushEngine: Send + Sync {
    /// Engine name (reported by `describe_document`).
    fn name(&self) -> &str;
    /// Coverage of one stroke on a canvas of `canvas` pixels. `points` is
    /// non-empty and `brush` has been validated.
    fn rasterize(
        &self,
        points: &[StrokePoint],
        brush: &BrushParams,
        canvas: Extent,
    ) -> EngineResult<StrokeCoverage>;
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
