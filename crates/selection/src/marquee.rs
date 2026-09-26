//! Geometric selections: rectangle, ellipse, single row/column, polygon.

use crate::mask::Mask;

/// Sub-scanlines per pixel for anti-aliased polygon fill.
const SUB: usize = 16;

fn overlap(a0: f32, a1: f32, b0: f32, b1: f32) -> f32 {
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

/// Rectangle `[x0, x1) × [y0, y1)` in continuous pixel coordinates. With
/// anti-aliasing each pixel gets its exact area coverage; without, pixels
/// whose centre is inside are selected.
pub fn rect(width: u32, height: u32, r: [f32; 4], antialias: bool) -> Mask {
    let [x0, y0, x1, y1] = r;
    let (x0, x1) = (x0.min(x1), x0.max(x1));
    let (y0, y1) = (y0.min(y1), y0.max(y1));
    Mask::from_fn(width, height, |x, y| {
        let (fx, fy) = (x as f32, y as f32);
        if antialias {
            overlap(fx, fx + 1.0, x0, x1) * overlap(fy, fy + 1.0, y0, y1)
        } else {
            let (cx, cy) = (fx + 0.5, fy + 0.5);
            f32::from(u8::from(cx >= x0 && cx < x1 && cy >= y0 && cy < y1))
        }
    })
}

/// Ellipse inscribed in `r = [x0, y0, x1, y1]`; anti-aliased by the
/// first-order signed distance to the boundary.
pub fn ellipse(width: u32, height: u32, r: [f32; 4], antialias: bool) -> Mask {
    let [x0, y0, x1, y1] = r;
    let (cx, cy) = ((x0 + x1) * 0.5, (y0 + y1) * 0.5);
    let (rx, ry) = (
        ((x1 - x0) * 0.5).abs().max(1e-3),
        ((y1 - y0) * 0.5).abs().max(1e-3),
    );
    Mask::from_fn(width, height, |x, y| {
        let (u, v) = ((x as f32 + 0.5 - cx) / rx, (y as f32 + 0.5 - cy) / ry);
        let f = u.hypot(v);
        if !antialias {
            return f32::from(u8::from(f <= 1.0));
        }
        if f < 1e-6 {
            return 1.0;
        }
        let g = (u / rx).hypot(v / ry) / f;
        let d = (f - 1.0) / g.max(1e-6);
        (0.5 - d).clamp(0.0, 1.0)
    })
}

/// A single selected row.
pub fn single_row(width: u32, height: u32, y: u32) -> Mask {
    Mask::from_fn(width, height, |_, yy| f32::from(u8::from(yy == y)))
}

/// A single selected column.
pub fn single_column(width: u32, height: u32, x: u32) -> Mask {
    Mask::from_fn(width, height, |xx, _| f32::from(u8::from(xx == x)))
}

/// Closed polygon, even-odd rule. Anti-aliasing uses 16 sub-scanlines per
/// pixel with exact horizontal coverage.
pub fn polygon(width: u32, height: u32, points: &[[f32; 2]], antialias: bool) -> Mask {
    let mut m = Mask::new(width, height);
    if points.len() < 3 || width == 0 || height == 0 {
        return m;
    }
    let (w, h) = (width as usize, height as usize);
    let edges: Vec<([f32; 2], [f32; 2])> = (0..points.len())
        .map(|i| (points[i], points[(i + 1) % points.len()]))
        .filter(|(a, b)| a[1] != b[1])
        .collect();
    let (ymin, ymax) = points
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), p| {
            (a.min(p[1]), b.max(p[1]))
        });
    let row0 = ymin.floor().max(0.0) as usize;
    let row1 = (ymax.ceil().max(0.0) as usize).min(h);
    let subs = if antialias { SUB } else { 1 };
    let mut xs: Vec<f32> = Vec::new();
    let mut row = vec![0.0f32; w];
    for y in row0..row1 {
        row.iter_mut().for_each(|v| *v = 0.0);
        for s in 0..subs {
            let sy = y as f32 + (s as f32 + 0.5) / subs as f32;
            xs.clear();
            for (a, b) in &edges {
                let (lo, hi) = if a[1] < b[1] { (a, b) } else { (b, a) };
                if sy >= lo[1] && sy < hi[1] {
                    xs.push(lo[0] + (sy - lo[1]) / (hi[1] - lo[1]) * (hi[0] - lo[0]));
                }
            }
            xs.sort_by(|a, b| a.total_cmp(b));
            for pair in xs.chunks(2) {
                let [xa, xb] = pair else { continue };
                let (xa, xb) = (xa.clamp(0.0, w as f32), xb.clamp(0.0, w as f32));
                if xb <= xa {
                    continue;
                }
                if antialias {
                    let (p0, p1) = (xa.floor() as usize, (xb.ceil() as usize).min(w));
                    for px in p0..p1 {
                        row[px] += overlap(px as f32, px as f32 + 1.0, xa, xb) / subs as f32;
                    }
                } else {
                    let p0 = (xa - 0.5).ceil().max(0.0) as usize;
                    let p1 = ((xb - 0.5).ceil().max(0.0) as usize).min(w);
                    for v in &mut row[p0.min(p1)..p1] {
                        *v = 1.0;
                    }
                }
            }
        }
        for (x, v) in row.iter().enumerate() {
            m.set(x as i64, y as i64, v.clamp(0.0, 1.0));
        }
    }
    m
}
