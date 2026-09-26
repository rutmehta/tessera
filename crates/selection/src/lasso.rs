//! Lasso tools: freehand, polygonal and magnetic (live-wire).
//!
//! The magnetic lasso runs Dijkstra on the lattice of pixel *corners*,
//! where a step edge between two pixel columns has its maximum gradient
//! exactly on the shared corner column, so traced outlines land on the true
//! edge rather than half a pixel to either side.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use crate::marquee::polygon;
use crate::mask::{Image, Mask};

/// Freehand lasso: the pointer path closed back to its start.
pub fn freehand(width: u32, height: u32, path: &[[f32; 2]], antialias: bool) -> Mask {
    polygon(width, height, path, antialias)
}

/// Polygonal lasso: straight segments between clicked vertices.
pub fn polygonal(width: u32, height: u32, vertices: &[[f32; 2]], antialias: bool) -> Mask {
    polygon(width, height, vertices, antialias)
}

/// Live-wire cost field over the `(w + 1) × (h + 1)` corner lattice.
#[derive(Debug, Clone)]
pub struct MagneticLasso {
    w: usize,
    h: usize,
    grad: Vec<f32>,
    cost: Vec<f32>,
}

impl MagneticLasso {
    /// Builds the cost field. `contrast` (0..1) is the gradient below which
    /// an edge is ignored.
    pub fn new(img: &Image, contrast: f32) -> Self {
        let (w, h) = (img.width as usize, img.height as usize);
        let (cw, ch) = (w + 1, h + 1);
        let mut grad = vec![0.0f32; cw * ch];
        for cy in 0..ch as i64 {
            for cx in 0..cw as i64 {
                let p = |x: i64, y: i64| img.get(x, y);
                let (a, b, c, d) = (p(cx - 1, cy - 1), p(cx, cy - 1), p(cx - 1, cy), p(cx, cy));
                let mut g = 0.0f32;
                for k in 0..3 {
                    let gx = 0.5 * ((b[k] + d[k]) - (a[k] + c[k]));
                    let gy = 0.5 * ((c[k] + d[k]) - (a[k] + b[k]));
                    g = g.max(gx.hypot(gy));
                }
                grad[cy as usize * cw + cx as usize] = g;
            }
        }
        let max = grad.iter().fold(0.0f32, |a, b| a.max(*b)).max(1e-6);
        let t = contrast.clamp(0.0, 0.99);
        let cost = grad
            .iter()
            .map(|g| {
                let n = ((g / max - t) / (1.0 - t)).clamp(0.0, 1.0);
                1.0 - n + 0.01
            })
            .collect();
        Self {
            w: cw,
            h: ch,
            grad,
            cost,
        }
    }

    /// The corner of highest gradient within `radius` of `p`.
    pub fn snap(&self, p: [f32; 2], radius: f32) -> (usize, usize) {
        let (px, py) = (p[0].round() as i64, p[1].round() as i64);
        let r = radius.max(0.0).ceil() as i64;
        let mut best = (
            px.clamp(0, self.w as i64 - 1) as usize,
            py.clamp(0, self.h as i64 - 1) as usize,
        );
        let mut bg = self.grad[best.1 * self.w + best.0];
        for y in (py - r).max(0)..=(py + r).min(self.h as i64 - 1) {
            for x in (px - r).max(0)..=(px + r).min(self.w as i64 - 1) {
                let d2 = ((x - px).pow(2) + (y - py).pow(2)) as f32;
                let g = self.grad[y as usize * self.w + x as usize];
                if d2 <= radius * radius && g > bg + 1e-6 {
                    bg = g;
                    best = (x as usize, y as usize);
                }
            }
        }
        best
    }

    /// Minimum-cost 8-connected corner path from `a` to `b`, searched in
    /// their bounding box grown by `margin` corners. Includes both ends.
    pub fn path(&self, a: (usize, usize), b: (usize, usize), margin: usize) -> Vec<[f32; 2]> {
        let x0 = a.0.min(b.0).saturating_sub(margin);
        let y0 = a.1.min(b.1).saturating_sub(margin);
        let x1 = (a.0.max(b.0) + margin).min(self.w - 1);
        let y1 = (a.1.max(b.1) + margin).min(self.h - 1);
        let (bw, bh) = (x1 - x0 + 1, y1 - y0 + 1);
        let idx = |x: usize, y: usize| (y - y0) * bw + (x - x0);
        let mut dist = vec![f32::INFINITY; bw * bh];
        let mut prev = vec![usize::MAX; bw * bh];
        let mut heap = BinaryHeap::new();
        let start = idx(a.0, a.1);
        let goal = idx(b.0, b.1);
        dist[start] = 0.0;
        heap.push(Reverse((0u32, start)));
        const N8: [(i64, i64, f32); 8] = [
            (1, 0, 1.0),
            (-1, 0, 1.0),
            (0, 1, 1.0),
            (0, -1, 1.0),
            (1, 1, std::f32::consts::SQRT_2),
            (1, -1, std::f32::consts::SQRT_2),
            (-1, 1, std::f32::consts::SQRT_2),
            (-1, -1, std::f32::consts::SQRT_2),
        ];
        while let Some(Reverse((dbits, i))) = heap.pop() {
            let d = f32::from_bits(dbits);
            if d > dist[i] {
                continue;
            }
            if i == goal {
                break;
            }
            let (lx, ly) = ((i % bw) as i64, (i / bw) as i64);
            let ci = self.cost[(ly as usize + y0) * self.w + lx as usize + x0];
            for (dx, dy, len) in N8 {
                let (nx, ny) = (lx + dx, ly + dy);
                if nx < 0 || ny < 0 || nx >= bw as i64 || ny >= bh as i64 {
                    continue;
                }
                let j = ny as usize * bw + nx as usize;
                let cj = self.cost[(ny as usize + y0) * self.w + nx as usize + x0];
                let nd = d + 0.5 * (ci + cj) * len;
                if nd < dist[j] {
                    dist[j] = nd;
                    prev[j] = i;
                    heap.push(Reverse((nd.to_bits(), j)));
                }
            }
        }
        let mut out = Vec::new();
        let mut i = goal;
        while i != usize::MAX {
            out.push([(i % bw + x0) as f32, (i / bw + y0) as f32]);
            if i == start {
                break;
            }
            i = prev[i];
        }
        out.reverse();
        out
    }

    /// Closed outline through `anchors` (each snapped to the strongest edge
    /// within `width` pixels), consecutive anchors joined by live-wire.
    pub fn trace(&self, anchors: &[[f32; 2]], width: f32) -> Vec<[f32; 2]> {
        if anchors.is_empty() {
            return Vec::new();
        }
        let snapped: Vec<(usize, usize)> = anchors.iter().map(|a| self.snap(*a, width)).collect();
        let margin = (width.ceil() as usize).max(4) * 2;
        let mut out: Vec<[f32; 2]> = Vec::new();
        for k in 0..snapped.len() {
            let seg = self.path(snapped[k], snapped[(k + 1) % snapped.len()], margin);
            // Drop the duplicated joint.
            let skip = usize::from(!out.is_empty());
            out.extend(seg.into_iter().skip(skip));
        }
        if out.len() > 1 && out.first() == out.last() {
            out.pop();
        }
        out
    }
}

/// Magnetic lasso selection.
pub fn magnetic(
    img: &Image,
    anchors: &[[f32; 2]],
    width: f32,
    contrast: f32,
    antialias: bool,
) -> Mask {
    let lasso = MagneticLasso::new(img, contrast);
    polygon(
        img.width,
        img.height,
        &lasso.trace(anchors, width),
        antialias,
    )
}
