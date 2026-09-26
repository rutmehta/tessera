//! Marching-ants outlines: iso-contours of a selection as polylines.
//!
//! Marching squares over pixel centres at level 0.5, with the mask padded by
//! a ring of zeros so every contour is closed. Saddles are resolved by the
//! cell average. Points are in canvas pixel coordinates.

use std::collections::HashMap;

use crate::mask::Mask;

/// A contour crossing: edge id and position.
type Crossing = (u64, [f32; 2]);

/// One outline.
#[derive(Debug, Clone, PartialEq)]
pub struct Polyline {
    /// Vertices; a closed polyline does not repeat its first vertex.
    pub points: Vec<[f32; 2]>,
    /// Always true for selection outlines.
    pub closed: bool,
}

impl Polyline {
    /// Signed shoelace area (closed polylines).
    pub fn area(&self) -> f32 {
        let n = self.points.len();
        let mut a = 0.0f64;
        for i in 0..n {
            let (p, q) = (self.points[i], self.points[(i + 1) % n]);
            a += f64::from(p[0]) * f64::from(q[1]) - f64::from(q[0]) * f64::from(p[1]);
        }
        (a * 0.5) as f32
    }
}

/// Traces the `level` iso-contours of `m`, simplified with Douglas–Peucker
/// at `tolerance` pixels (0 keeps every vertex).
pub fn trace(m: &Mask, level: f32, tolerance: f32) -> Vec<Polyline> {
    let (w, h) = (m.width() as i64, m.height() as i64);
    let gw = w + 2;
    let g = |gx: i64, gy: i64| m.get(gx - 1, gy - 1);
    // Edge ids: horizontal (gx,gy)-(gx+1,gy) → 2k, vertical (gx,gy)-(gx,gy+1) → 2k+1.
    let hid = |gx: i64, gy: i64| ((gy * gw + gx) * 2) as u64;
    let vid = |gx: i64, gy: i64| ((gy * gw + gx) * 2 + 1) as u64;
    let mut points: HashMap<u64, [f32; 2]> = HashMap::new();
    let mut adj: HashMap<u64, Vec<u64>> = HashMap::new();
    let interp = |a: f32, b: f32| {
        let d = b - a;
        if d.abs() < 1e-12 {
            0.5
        } else {
            ((level - a) / d).clamp(0.0, 1.0)
        }
    };
    for cy in 0..h + 1 {
        for cx in 0..w + 1 {
            let (tl, tr, br, bl) = (g(cx, cy), g(cx + 1, cy), g(cx + 1, cy + 1), g(cx, cy + 1));
            let case = (usize::from(tl >= level) << 3)
                | (usize::from(tr >= level) << 2)
                | (usize::from(br >= level) << 1)
                | usize::from(bl >= level);
            if case == 0 || case == 15 {
                continue;
            }
            // Pixel-centre coordinates of lattice node (gx, gy): (gx − 0.5, gy − 0.5).
            let (x, y) = (cx as f32 - 0.5, cy as f32 - 0.5);
            let top = (hid(cx, cy), [x + interp(tl, tr), y]);
            let bottom = (hid(cx, cy + 1), [x + interp(bl, br), y + 1.0]);
            let left = (vid(cx, cy), [x, y + interp(tl, bl)]);
            let right = (vid(cx + 1, cy), [x + 1.0, y + interp(tr, br)]);
            let centre = (tl + tr + br + bl) * 0.25 >= level;
            let segs: &[(Crossing, Crossing)] = &match case {
                1 | 14 => vec![(left, bottom)],
                2 | 13 => vec![(bottom, right)],
                3 | 12 => vec![(left, right)],
                4 | 11 => vec![(top, right)],
                6 | 9 => vec![(top, bottom)],
                7 | 8 => vec![(left, top)],
                5 => {
                    if centre {
                        vec![(left, top), (bottom, right)]
                    } else {
                        vec![(left, bottom), (top, right)]
                    }
                }
                10 => {
                    if centre {
                        vec![(left, bottom), (top, right)]
                    } else {
                        vec![(left, top), (bottom, right)]
                    }
                }
                _ => vec![],
            };
            for &((ia, pa), (ib, pb)) in segs {
                points.insert(ia, pa);
                points.insert(ib, pb);
                adj.entry(ia).or_default().push(ib);
                adj.entry(ib).or_default().push(ia);
            }
        }
    }
    let mut keys: Vec<u64> = adj.keys().copied().collect();
    keys.sort_unstable();
    let mut visited: HashMap<u64, bool> = HashMap::new();
    let mut out = Vec::new();
    for start in keys {
        if visited.contains_key(&start) {
            continue;
        }
        let mut loop_pts = Vec::new();
        let (mut prev, mut cur) = (u64::MAX, start);
        loop {
            visited.insert(cur, true);
            loop_pts.push(points[&cur]);
            let next = adj[&cur]
                .iter()
                .copied()
                .find(|n| *n != prev && !visited.contains_key(n));
            match next {
                Some(n) => {
                    prev = cur;
                    cur = n;
                }
                None => break,
            }
        }
        if loop_pts.len() >= 3 {
            orient(&mut loop_pts, m, level);
            let pts = if tolerance > 0.0 {
                simplify_closed(&loop_pts, tolerance)
            } else {
                loop_pts
            };
            out.push(Polyline {
                points: pts,
                closed: true,
            });
        }
    }
    out
}

/// Bilinear value of the pixel-centre lattice at canvas point `(x, y)`.
fn sample(m: &Mask, x: f32, y: f32) -> f32 {
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0, y0) = (fx.floor(), fy.floor());
    let (ax, ay) = (fx - x0, fy - y0);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let a = m.get(x0, y0) + (m.get(x0 + 1, y0) - m.get(x0, y0)) * ax;
    let b = m.get(x0, y0 + 1) + (m.get(x0 + 1, y0 + 1) - m.get(x0, y0 + 1)) * ax;
    a + (b - a) * ay
}

/// Orients a loop so the selection is on the left of the direction of
/// travel (y down): outlines then have negative and holes positive
/// shoelace area.
fn orient(pts: &mut [[f32; 2]], m: &Mask, level: f32) {
    let n = pts.len();
    let i = (0..n)
        .max_by(|&i, &j| {
            let l = |k: usize| {
                let (p, q) = (pts[k], pts[(k + 1) % n]);
                (q[0] - p[0]).hypot(q[1] - p[1])
            };
            l(i).total_cmp(&l(j))
        })
        .unwrap_or(0);
    let (p, q) = (pts[i], pts[(i + 1) % n]);
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let len = dx.hypot(dy).max(1e-6);
    // Left normal in y-down coordinates.
    let (nx, ny) = (dy / len, -dx / len);
    let (mx, my) = ((p[0] + q[0]) * 0.5, (p[1] + q[1]) * 0.5);
    let left = sample(m, mx + 0.25 * nx, my + 0.25 * ny);
    let right = sample(m, mx - 0.25 * nx, my - 0.25 * ny);
    if left < level && right >= level || (left < right && (left >= level) == (right >= level)) {
        pts.reverse();
    }
}

fn dp(pts: &[[f32; 2]], tol: f32, keep: &mut [bool], a: usize, b: usize) {
    if b <= a + 1 {
        return;
    }
    let (p, q) = (pts[a], pts[b]);
    let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
    let len = dx.hypot(dy).max(1e-12);
    let (mut best, mut idx) = (0.0f32, a);
    for (i, r) in pts.iter().enumerate().take(b).skip(a + 1) {
        let d = ((r[0] - p[0]) * dy - (r[1] - p[1]) * dx).abs() / len;
        if d > best {
            best = d;
            idx = i;
        }
    }
    if best > tol {
        keep[idx] = true;
        dp(pts, tol, keep, a, idx);
        dp(pts, tol, keep, idx, b);
    }
}

/// Douglas–Peucker for a closed ring (split at the farthest vertex).
fn simplify_closed(pts: &[[f32; 2]], tol: f32) -> Vec<[f32; 2]> {
    let n = pts.len();
    let far = (1..n)
        .max_by(|&i, &j| {
            let d = |k: usize| (pts[k][0] - pts[0][0]).hypot(pts[k][1] - pts[0][1]);
            d(i).total_cmp(&d(j))
        })
        .unwrap_or(0);
    let mut ring = pts.to_vec();
    ring.push(pts[0]);
    let mut keep = vec![false; n + 1];
    keep[0] = true;
    keep[far] = true;
    keep[n] = true;
    dp(&ring, tol, &mut keep, 0, far);
    dp(&ring, tol, &mut keep, far, n);
    (0..n).filter(|&i| keep[i]).map(|i| ring[i]).collect()
}
