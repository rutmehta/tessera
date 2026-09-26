//! Gaussian-mask Laplacian pyramid blending, with nearest-covered extension.
use std::collections::VecDeque;
pub(crate) fn extend(p: &mut [[f32; 3]], mask: &[bool], w: usize, h: usize) {
    let mut seen = mask.to_vec();
    let mut q = VecDeque::new();
    for (i, v) in mask.iter().enumerate() {
        if *v {
            q.push_back(i);
        }
    }
    while let Some(i) = q.pop_front() {
        let x = i % w;
        let y = i / w;
        for (nx, ny) in [
            (x.saturating_sub(1), y),
            ((x + 1).min(w - 1), y),
            (x, y.saturating_sub(1)),
            (x, (y + 1).min(h - 1)),
        ] {
            let j = ny * w + nx;
            if !seen[j] {
                p[j] = p[i];
                seen[j] = true;
                q.push_back(j);
            }
        }
    }
}
pub(crate) fn down<const N: usize>(p: &[[f32; N]], w: usize, h: usize) -> Vec<[f32; N]> {
    let nw = w.div_ceil(2);
    let nh = h.div_ceil(2);
    let kernel = [1., 4., 6., 4., 1.];
    let mut out = vec![[0.; N]; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            for (dy, ky) in kernel.iter().enumerate() {
                for (dx, kx) in kernel.iter().enumerate() {
                    let xx = (2 * x + dx).saturating_sub(2).min(w - 1);
                    let yy = (2 * y + dy).saturating_sub(2).min(h - 1);
                    for c in 0..N {
                        out[y * nw + x][c] += p[yy * w + xx][c] * kx * ky / 256.;
                    }
                }
            }
        }
    }
    out
}
pub(crate) fn up(p: &[[f32; 3]], w: usize, h: usize, nw: usize, nh: usize) -> Vec<[f32; 3]> {
    let mut out = vec![[0.; 3]; nw * nh];
    for y in 0..nh {
        for x in 0..nw {
            let xx = x / 2;
            let yy = y / 2;
            let fx = (x % 2) as f32 * 0.5;
            let fy = (y % 2) as f32 * 0.5;
            for c in 0..3 {
                out[y * nw + x][c] = (p[yy * w + xx][c] * (1. - fx)
                    + p[yy * w + (xx + 1).min(w - 1)][c] * fx)
                    * (1. - fy)
                    + (p[(yy + 1).min(h - 1) * w + xx][c] * (1. - fx)
                        + p[(yy + 1).min(h - 1) * w + (xx + 1).min(w - 1)][c] * fx)
                        * fy;
            }
        }
    }
    out
}
pub(crate) struct Blender {
    dims: Vec<(usize, usize)>,
    sum: Vec<Vec<[f32; 3]>>,
    weight: Vec<Vec<[f32; 1]>>,
}
impl Blender {
    pub(crate) fn new(w: usize, h: usize, levels: usize) -> Self {
        let mut dims = vec![(w, h)];
        while dims.len() < levels {
            let (w, h) = *dims.last().unwrap();
            if w == 1 && h == 1 {
                break;
            }
            dims.push((w.div_ceil(2), h.div_ceil(2)));
        }
        Self {
            sum: dims.iter().map(|(w, h)| vec![[0.; 3]; w * h]).collect(),
            weight: dims.iter().map(|(w, h)| vec![[0.]; w * h]).collect(),
            dims,
        }
    }
    pub(crate) fn add(&mut self, pixels: Vec<[f32; 3]>, weights: Vec<[f32; 1]>) {
        let mask: Vec<_> = weights.iter().map(|v| v[0] > 0.).collect();
        self.add_covered(pixels, weights, &mask);
    }
    /// Ownership is not source support: retain real overlap samples for the
    /// Laplacian stencils on both sides of the chosen seam.
    pub(crate) fn add_covered(
        &mut self,
        mut pixels: Vec<[f32; 3]>,
        weights: Vec<[f32; 1]>,
        mask: &[bool],
    ) {
        let (w, h) = self.dims[0];
        extend(&mut pixels, mask, w, h);
        let mut gp = vec![pixels];
        let mut gm = vec![weights];
        for l in 1..self.dims.len() {
            let (w, h) = self.dims[l - 1];
            gp.push(down(&gp[l - 1], w, h));
            gm.push(down(&gm[l - 1], w, h));
        }
        for l in 0..self.dims.len() {
            let expanded = if l + 1 < self.dims.len() {
                let (w, h) = self.dims[l + 1];
                let (nw, nh) = self.dims[l];
                Some(up(&gp[l + 1], w, h, nw, nh))
            } else {
                None
            };
            for i in 0..gp[l].len() {
                let wt = gm[l][i][0];
                self.weight[l][i][0] += wt;
                for c in 0..3 {
                    let lap = gp[l][i][c] - expanded.as_ref().map_or(0., |p| p[i][c]);
                    self.sum[l][i][c] += wt * lap;
                }
            }
        }
    }
    pub(crate) fn finish(mut self) -> Vec<[f32; 3]> {
        for l in 0..self.dims.len() {
            for i in 0..self.sum[l].len() {
                let w = self.weight[l][i][0];
                if w > 1e-12 {
                    for c in 0..3 {
                        self.sum[l][i][c] /= w;
                    }
                }
            }
        }
        let mut out = self.sum.pop().unwrap();
        for l in (0..self.dims.len() - 1).rev() {
            let (w, h) = self.dims[l + 1];
            let (nw, nh) = self.dims[l];
            let expanded = up(&out, w, h, nw, nh);
            out = self.sum.pop().unwrap();
            for (p, e) in out.iter_mut().zip(expanded) {
                for c in 0..3 {
                    p[c] += e[c];
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn multiband_preserves_constants_and_softens_a_seam() {
        let (w, h) = (97, 33);
        let n = w * h;
        let mut single = Blender::new(w, h, 6);
        single.add(vec![[0.4, 0.7, 1.2]; n], vec![[1.]; n]);
        for p in single.finish() {
            for (a, b) in p.into_iter().zip([0.4, 0.7, 1.2]) {
                assert!((a - b).abs() < 1e-5);
            }
        }
        let run = |levels| {
            let mut blend = Blender::new(w, h, levels);
            blend.add(
                vec![[0.2; 3]; n],
                (0..n).map(|i| [if i % w < 49 { 1. } else { 0. }]).collect(),
            );
            blend.add(
                vec![[0.8; 3]; n],
                (0..n)
                    .map(|i| [if i % w >= 49 { 1. } else { 0. }])
                    .collect(),
            );
            blend.finish()
        };
        let hard = run(1);
        let soft = run(5);
        let seam = 16 * w + 49;
        assert!(soft[seam][0] - soft[seam - 1][0] < (hard[seam][0] - hard[seam - 1][0]) * 0.25);
        assert!(soft.iter().all(|p| p[0] >= 0.19 && p[0] <= 0.81));
    }
}
