//! Pixel selections: the pluggable [`SelectionEngine`] that rasterizes
//! marquee and lasso geometry and feathers masks, and the built-in
//! [`RealSelection`] adapter and the lightweight [`BasicSelection`] fallback.
//!
//! Selections are single-channel f32 canvas rasters (the compositor's
//! representation). Combination (replace/add/subtract/intersect), inverse,
//! layer transparency and saved selections are handled by the executor.
use compositor::{Depth, Raster, Rect};
use engine_api::document::CanvasRect;
use engine_api::tile::Extent;
use engine_api::{EngineError, EngineResult};

use super::dense::{self, Dense};

/// Geometry a selection engine rasterizes.
#[derive(Debug, Clone, PartialEq)]
pub enum SelectionGeometry {
    /// Rectangle.
    Rect(CanvasRect),
    /// Ellipse inscribed in the rectangle.
    Ellipse(CanvasRect),
    /// Closed polygon, even-odd fill.
    Polygon(Vec<[f32; 2]>),
}

/// Rasterizes selection geometry. Implementations must be deterministic.
pub trait SelectionEngine: Send + Sync {
    /// Engine name (reported by `describe_document`).
    fn name(&self) -> &str;
    /// A canvas-sized single-channel f32 raster (0 outside, 1 inside,
    /// anti-aliased edges).
    fn rasterize(&self, geometry: &SelectionGeometry, canvas: Extent) -> EngineResult<Raster>;
    /// `mask` blurred by a feather radius in pixels.
    fn feather(&self, mask: &Raster, radius: f32) -> EngineResult<Raster>;
}

/// Production adapter to the selection crate's antialiased geometry and
/// Gaussian feather algorithms.
#[derive(Debug, Clone, Copy, Default)]
pub struct RealSelection;

impl SelectionEngine for RealSelection {
    fn name(&self) -> &str {
        "selection"
    }

    fn rasterize(&self, geometry: &SelectionGeometry, canvas: Extent) -> EngineResult<Raster> {
        let (w, h) = (canvas.width, canvas.height);
        let box4 = |r: &CanvasRect| [r.x0 as f32, r.y0 as f32, r.x1 as f32, r.y1 as f32];
        let mask = match geometry {
            SelectionGeometry::Rect(r) | SelectionGeometry::Ellipse(r)
                if r.x1 <= r.x0 || r.y1 <= r.y0 =>
            {
                return Err(EngineError::invalid(
                    "rect",
                    "must have positive width and height",
                ));
            }
            SelectionGeometry::Rect(r) => selection::marquee::rect(w, h, box4(r), true),
            SelectionGeometry::Ellipse(r) => selection::marquee::ellipse(w, h, box4(r), true),
            SelectionGeometry::Polygon(points) => {
                if points.len() < 3 || points.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(EngineError::invalid(
                        "points",
                        "a polygon needs at least three finite vertices",
                    ));
                }
                selection::marquee::polygon(w, h, points, true)
            }
        };
        mask.to_raster(Depth::F32)
    }

    fn feather(&self, mask: &Raster, radius: f32) -> EngineResult<Raster> {
        if !radius.is_finite() || !(0.0..=4096.0).contains(&radius) {
            return Err(EngineError::invalid(
                "feather",
                "must be finite and in 0..=4096 pixels",
            ));
        }
        if mask.channels() != 1 {
            return Err(EngineError::invalid("mask", "must be single-channel"));
        }
        selection::ops::feather(&selection::Mask::from_raster(mask)?, radius).to_raster(Depth::F32)
    }
}

/// Marquee (rectangle, ellipse) and polygon lasso with 4×4 supersampled
/// edges; feathering is a three-pass box blur approximating a Gaussian of
/// σ = radius / 2.
#[derive(Debug, Clone, Copy, Default)]
pub struct BasicSelection;

const SUB: usize = 4;

fn rect_of(r: &CanvasRect) -> Rect {
    Rect::new(r.x0, r.y0, r.x1, r.y1)
}

impl SelectionEngine for BasicSelection {
    fn name(&self) -> &str {
        "basic-marquee-lasso"
    }

    fn rasterize(&self, geometry: &SelectionGeometry, canvas: Extent) -> EngineResult<Raster> {
        let full = Rect::of_extent(canvas);
        let dense = match geometry {
            SelectionGeometry::Rect(r) => {
                let rect = rect_of(r).intersect(&full);
                Dense {
                    rect,
                    px: vec![
                        [1.0, 0.0, 0.0, 0.0];
                        (rect.width().max(0) * rect.height().max(0)) as usize
                    ],
                }
            }
            SelectionGeometry::Ellipse(r) => {
                let e = rect_of(r);
                let rect = e.intersect(&full);
                let (cx, cy) = ((e.x0 + e.x1) as f64 / 2.0, (e.y0 + e.y1) as f64 / 2.0);
                let (rx, ry) = (e.width() as f64 / 2.0, e.height() as f64 / 2.0);
                let w = rect.width().max(0) as usize;
                let mut px = vec![[0.0; 4]; w * rect.height().max(0) as usize];
                if rx > 0.0 && ry > 0.0 {
                    for y in rect.y0..rect.y1 {
                        for x in rect.x0..rect.x1 {
                            let mut hits = 0;
                            for sy in 0..SUB {
                                for sx in 0..SUB {
                                    let fx = (x as f64 + (sx as f64 + 0.5) / SUB as f64 - cx) / rx;
                                    let fy = (y as f64 + (sy as f64 + 0.5) / SUB as f64 - cy) / ry;
                                    hits += usize::from(fx * fx + fy * fy <= 1.0);
                                }
                            }
                            px[(y - rect.y0) as usize * w + (x - rect.x0) as usize][0] =
                                hits as f32 / (SUB * SUB) as f32;
                        }
                    }
                }
                Dense { rect, px }
            }
            SelectionGeometry::Polygon(points) => polygon(points, full)?,
        };
        dense::mask_raster(canvas, Depth::F32, 0.0, &dense)
    }

    fn feather(&self, mask: &Raster, radius: f32) -> EngineResult<Raster> {
        if radius <= 0.0 {
            return Ok(mask.clone());
        }
        let e = mask.extent();
        let d = dense::read(mask, Rect::of_extent(e))?;
        let (w, h) = (e.width as usize, e.height as usize);
        let mut v: Vec<f32> = d.px.iter().map(|p| p[0]).collect();
        // Three box passes of equal width whose variance sums to σ².
        let sigma = f64::from(radius) / 2.0;
        let half = (((12.0 * sigma * sigma / 3.0 + 1.0).sqrt() - 1.0) / 2.0).round() as usize;
        if half > 0 {
            let mut tmp = vec![0.0f32; v.len()];
            for _ in 0..3 {
                box_pass(&v, &mut tmp, w, h, half, true);
                box_pass(&tmp, &mut v, w, h, half, false);
            }
        }
        let rect = Rect::of_extent(e);
        let px = v.into_iter().map(|a| [a, 0.0, 0.0, 0.0]).collect();
        dense::mask_raster(e, Depth::F32, 0.0, &Dense { rect, px })
    }
}

/// One box-blur pass (clamped edges) along rows or columns.
fn box_pass(src: &[f32], dst: &mut [f32], w: usize, h: usize, r: usize, horizontal: bool) {
    let (n, lines) = if horizontal { (w, h) } else { (h, w) };
    let idx = |line: usize, i: usize| {
        if horizontal {
            line * w + i
        } else {
            i * w + line
        }
    };
    let norm = 1.0 / (2 * r + 1) as f64;
    for line in 0..lines {
        let at = |i: isize| src[idx(line, i.clamp(0, n as isize - 1) as usize)] as f64;
        let mut acc: f64 = (-(r as isize)..=r as isize).map(at).sum();
        for i in 0..n {
            dst[idx(line, i)] = (acc * norm) as f32;
            acc += at(i as isize + r as isize + 1) - at(i as isize - r as isize);
        }
    }
}

/// Even-odd polygon fill with `SUB` sub-scanlines per row and exact
/// horizontal span coverage.
fn polygon(points: &[[f32; 2]], full: Rect) -> EngineResult<Dense> {
    if points.len() < 3 || points.iter().flatten().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid(
            "points",
            "a polygon needs at least three finite vertices",
        ));
    }
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for p in points {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let rect = Rect::new(
        x0.floor() as i64,
        y0.floor() as i64,
        x1.ceil() as i64 + 1,
        y1.ceil() as i64 + 1,
    )
    .intersect(&full);
    let w = rect.width().max(0) as usize;
    let mut px = vec![[0.0; 4]; w * rect.height().max(0) as usize];
    let mut xs = Vec::new();
    for y in rect.y0..rect.y1 {
        for sy in 0..SUB {
            let fy = y as f32 + (sy as f32 + 0.5) / SUB as f32;
            xs.clear();
            for i in 0..points.len() {
                let (a, b) = (points[i], points[(i + 1) % points.len()]);
                if (a[1] <= fy) != (b[1] <= fy) {
                    xs.push(a[0] + (fy - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
                }
            }
            xs.sort_by(f32::total_cmp);
            for span in xs.as_chunks::<2>().0 {
                let (sa, sb) = (span[0].max(rect.x0 as f32), span[1].min(rect.x1 as f32));
                if sb <= sa {
                    continue;
                }
                for x in sa.floor() as i64..sb.ceil() as i64 {
                    let cover = (sb.min(x as f32 + 1.0) - sa.max(x as f32)).max(0.0);
                    px[(y - rect.y0) as usize * w + (x - rect.x0) as usize][0] +=
                        cover / SUB as f32;
                }
            }
        }
    }
    for p in &mut px {
        p[0] = p[0].min(1.0);
    }
    Ok(Dense { rect, px })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_selection_uses_selection_crate_and_validates_inputs() {
        let engine = RealSelection;
        let e = Extent::new(12, 12);
        let mask = engine
            .rasterize(&SelectionGeometry::Ellipse(r(2, 2, 10, 10)), e)
            .unwrap();
        let expected = selection::marquee::ellipse(12, 12, [2.0, 2.0, 10.0, 10.0], true);
        assert_eq!(selection::Mask::from_raster(&mask).unwrap(), expected);
        assert_eq!(
            selection::Mask::from_raster(&engine.feather(&mask, 2.0).unwrap()).unwrap(),
            selection::ops::feather(&expected, 2.0)
        );
        assert!(engine.feather(&mask, f32::NAN).is_err());
        assert!(
            engine
                .rasterize(&SelectionGeometry::Polygon(vec![[0.0, 0.0]; 2]), e)
                .is_err()
        );
    }

    fn r(x0: i64, y0: i64, x1: i64, y1: i64) -> CanvasRect {
        CanvasRect { x0, y0, x1, y1 }
    }

    #[test]
    fn marquee_ellipse_and_lasso_cover_their_shapes() {
        let e = Extent::new(40, 40);
        let s = BasicSelection;
        let rect = s
            .rasterize(&SelectionGeometry::Rect(r(5, 5, 15, 10)), e)
            .unwrap();
        assert_eq!(rect.pixel(5, 5)[0], 1.0);
        assert_eq!(rect.pixel(15, 5)[0], 0.0);
        let ell = s
            .rasterize(&SelectionGeometry::Ellipse(r(0, 0, 40, 40)), e)
            .unwrap();
        assert_eq!(ell.pixel(20, 20)[0], 1.0);
        assert_eq!(ell.pixel(0, 0)[0], 0.0);
        let tri = s
            .rasterize(
                &SelectionGeometry::Polygon(vec![[0.0, 0.0], [40.0, 0.0], [0.0, 40.0]]),
                e,
            )
            .unwrap();
        assert_eq!(tri.pixel(5, 5)[0], 1.0);
        assert_eq!(tri.pixel(35, 35)[0], 0.0);
        let diag = tri.pixel(20, 19)[0];
        assert!(diag > 0.0 && diag < 1.0, "{diag}");
        let soft = s.feather(&rect, 4.0).unwrap();
        let (inside, edge) = (soft.pixel(10, 7)[0], soft.pixel(15, 7)[0]);
        assert!(inside > edge && edge > 0.0 && edge < 1.0);
    }
}
