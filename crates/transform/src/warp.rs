use crate::{Error, Point, Result};
use serde::{Deserialize, Serialize};

/// Piecewise bicubic Bézier surface. Points are destination pixels, row-major.
/// Adjacent patches share one row/column; split parameters are normalized UV.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WarpMesh {
    pub width: f64,
    pub height: f64,
    pub control_points: Vec<Vec<Point>>,
    pub u_splits: Vec<f64>,
    pub v_splits: Vec<f64>,
}

/// Signed bend presets; bend is in [-1, 1]. These are editable cubic
/// approximations, not a claim of pixel-identical proprietary preset geometry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum WarpPreset {
    Arc,
    ArcLower,
    ArcUpper,
    Arch,
    Bulge,
    Shell,
    Flag,
    Wave,
    Fish,
    Rise,
    Fisheye,
    Inflate,
    Squeeze,
    Twist,
}
impl Default for WarpMesh {
    fn default() -> Self {
        Self::identity(1., 1.)
    }
}
impl WarpMesh {
    pub fn preset(width: f64, height: f64, preset: WarpPreset, bend: f64) -> Result<Self> {
        let mut mesh = Self::identity(width, height);
        mesh.validate()?;
        if !bend.is_finite() || !(-1. ..=1.).contains(&bend) {
            return Err(Error::Invalid("bend must be finite and in [-1, 1]".into()));
        }
        if bend == 0. {
            return Ok(mesh);
        }
        for row in &mut mesh.control_points {
            for p in row {
                let x = 2. * p[0] / width - 1.;
                let y = 2. * p[1] / height - 1.;
                let cx = 1. - x * x;
                let cy = 1. - y * y;
                use WarpPreset::*;
                let (dx, dy) = match preset {
                    Arc => (-0.15 * x * y, -0.5 * cx),
                    ArcLower => (0., 0.5 * cx * (y + 1.) / 2.),
                    ArcUpper => (0., -0.5 * cx * (1. - y) / 2.),
                    Arch => (0., -0.5 * cx),
                    Bulge => (0.3 * x * cy, 0.3 * y * cx),
                    Shell => (0.2 * x * (y + 1.), 0.4 * cx * (y + 1.)),
                    Flag => (0., 0.35 * (std::f64::consts::PI * x).sin() * (1. - 0.3 * y)),
                    Wave => (
                        0.1 * (std::f64::consts::PI * y).sin(),
                        0.35 * (std::f64::consts::PI * x).sin(),
                    ),
                    Fish => (0.2 * cx, 0.35 * y * (1. - x)),
                    Rise => (0., -0.4 * x),
                    Fisheye => (0.4 * x * (cx + cy), 0.4 * y * (cx + cy)),
                    Inflate => (0.25 * x * cy, 0.4 * y * cx),
                    Squeeze => (-0.4 * x * cy, 0.2 * y * cx),
                    Twist => (-0.35 * y * (cx + cy), 0.35 * x * (cx + cy)),
                };
                p[0] += width * bend * dx;
                p[1] += height * bend * dy;
            }
        }
        mesh.validate()?;
        Ok(mesh)
    }
    pub fn identity(width: f64, height: f64) -> Self {
        Self {
            width,
            height,
            control_points: (0..4)
                .map(|y| {
                    (0..4)
                        .map(|x| [width * x as f64 / 3., height * y as f64 / 3.])
                        .collect()
                })
                .collect(),
            u_splits: vec![0., 1.],
            v_splits: vec![0., 1.],
        }
    }
    /// Add a regular UV grid (1..=128 segments per axis), retaining existing
    /// splits and preserving geometry. A failed request leaves the mesh intact.
    pub fn subdivide(&mut self, columns: usize, rows: usize) -> Result<()> {
        self.validate()?;
        if !(1..=128).contains(&columns) || !(1..=128).contains(&rows) {
            return Err(Error::Invalid("subdivision counts must be 1..=128".into()));
        }
        let mut next = self.clone();
        for x in 1..columns {
            let t = x as f64 / columns as f64;
            if next.u_splits.iter().all(|v| (*v - t).abs() > 1e-10) {
                next.split_u(t)?;
            }
        }
        for y in 1..rows {
            let t = y as f64 / rows as f64;
            if next.v_splits.iter().all(|v| (*v - t).abs() > 1e-10) {
                next.split_v(t)?;
            }
        }
        *self = next;
        Ok(())
    }
    /// Insert an entire vertical split using exact de Casteljau subdivision.
    pub fn split_u(&mut self, u: f64) -> Result<()> {
        self.split(u, false)
    }
    /// Insert an entire horizontal split without changing the surface.
    pub fn split_v(&mut self, v: f64) -> Result<()> {
        self.split(v, true)
    }
    fn split(&mut self, t: f64, vertical: bool) -> Result<()> {
        self.validate()?;
        let knots = if vertical {
            &self.v_splits
        } else {
            &self.u_splits
        };
        if !t.is_finite() || t <= 0. || t >= 1. || knots.iter().any(|k| (k - t).abs() <= 1e-10) {
            return Err(Error::Invalid(
                "split must lie strictly inside an existing patch".into(),
            ));
        }
        let index = knots.partition_point(|k| *k < t) - 1;
        let local = (t - knots[index]) / (knots[index + 1] - knots[index]);
        let mut net = if vertical {
            transpose(&self.control_points)
        } else {
            self.control_points.clone()
        };
        for row in &mut net {
            let j = index * 3;
            let a = lerp(row[j], row[j + 1], local);
            let b = lerp(row[j + 1], row[j + 2], local);
            let c = lerp(row[j + 2], row[j + 3], local);
            let d = lerp(a, b, local);
            let e = lerp(b, c, local);
            let f = lerp(d, e, local);
            row.splice(j..=j + 3, [row[j], a, d, f, e, c, row[j + 3]]);
        }
        self.control_points = if vertical { transpose(&net) } else { net };
        if vertical {
            self.v_splits.insert(index + 1, t);
        } else {
            self.u_splits.insert(index + 1, t);
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<()> {
        let knots = |s: &[f64]| {
            s.len() >= 2
                && s[0] == 0.
                && s[s.len() - 1] == 1.
                && s.windows(2)
                    .all(|w| w[0].is_finite() && w[1].is_finite() && w[1] - w[0] > 1e-10)
        };
        if !self.width.is_finite()
            || !self.height.is_finite()
            || self.width <= 0.
            || self.height <= 0.
            || !knots(&self.u_splits)
            || !knots(&self.v_splits)
        {
            return Err(Error::Invalid(
                "invalid warp dimensions or split parameters".into(),
            ));
        }
        if self.control_points.len() != 3 * (self.v_splits.len() - 1) + 1
            || self.control_points.iter().any(|r| {
                r.len() != 3 * (self.u_splits.len() - 1) + 1
                    || r.iter().flatten().any(|x| !x.is_finite())
            })
        {
            return Err(Error::Invalid("invalid bicubic control net".into()));
        }
        let origin = self.control_points[0][0];
        let points = self.control_points.iter().flatten();
        let far = points
            .clone()
            .max_by(|a, b| {
                (a[0] - origin[0])
                    .hypot(a[1] - origin[1])
                    .total_cmp(&(b[0] - origin[0]).hypot(b[1] - origin[1]))
            })
            .unwrap();
        let length = (far[0] - origin[0]).hypot(far[1] - origin[1]);
        let direction = [(far[0] - origin[0]) / length, (far[1] - origin[1]) / length];
        if !length.is_finite()
            || length == 0.
            || !points.clone().any(|p| {
                (direction[0] * (p[1] - origin[1]) / length
                    - direction[1] * (p[0] - origin[0]) / length)
                    .abs()
                    > 1e-12
            })
        {
            return Err(Error::Invalid(
                "warp control net is collapsed or numerically singular".into(),
            ));
        }
        Ok(())
    }
    /// Map normalized UV to destination pixels. Out-of-domain input yields NaNs.
    pub fn forward(&self, uv: Point) -> Point {
        if !unit(uv) || self.validate().is_err() {
            return [f64::NAN; 2];
        }
        self.evaluate(uv).0
    }
    fn evaluate(&self, uv: Point) -> (Point, Point, Point) {
        let cell = |s: &[f64], t: f64| {
            let i = s
                .partition_point(|x| *x <= t)
                .saturating_sub(1)
                .min(s.len() - 2);
            (i, (t - s[i]) / (s[i + 1] - s[i]), s[i + 1] - s[i])
        };
        let (ix, u, du) = cell(&self.u_splits, uv[0]);
        let (iy, v, dv) = cell(&self.v_splits, uv[1]);
        let basis = |t: f64| {
            let s = 1. - t;
            (
                [s * s * s, 3. * s * s * t, 3. * s * t * t, t * t * t],
                [
                    -3. * s * s,
                    3. * s * s - 6. * s * t,
                    6. * s * t - 3. * t * t,
                    3. * t * t,
                ],
            )
        };
        let (bu, bu1) = basis(u);
        let (bv, bv1) = basis(v);
        let (mut p, mut pu, mut pv) = ([0.; 2], [0.; 2], [0.; 2]);
        for y in 0..4 {
            for x in 0..4 {
                for k in 0..2 {
                    let c = self.control_points[iy * 3 + y][ix * 3 + x][k];
                    p[k] += c * bu[x] * bv[y];
                    pu[k] += c * bu1[x] * bv[y] / du;
                    pv[k] += c * bu[x] * bv1[y] / dv;
                }
            }
        }
        (p, pu, pv)
    }
    /// Inverse returns normalized UV, not source pixels. Ambiguous folds use the
    /// first converging deterministic seed. Points outside the surface return None.
    pub fn inverse(&self, p: Point) -> Option<Point> {
        if self.validate().is_err() || p.iter().any(|x| !x.is_finite()) {
            return None;
        }
        self.newton(
            p,
            [
                (p[0] / self.width).clamp(0., 1.),
                (p[1] / self.height).clamp(0., 1.),
            ],
        )
        .or_else(|| self.inverse_field(24).ok()?.lookup(p))
    }
    /// Snapshot a pre-rasterized UV displacement field for repeated inverse queries.
    /// The snapshot is independent of subsequent edits to this mesh.
    pub fn inverse_field(&self, resolution: usize) -> Result<WarpInverseField> {
        self.validate()?;
        if !(2..=512).contains(&resolution) {
            return Err(Error::Invalid("field resolution must be 2..=512".into()));
        }
        let mut samples = Vec::with_capacity((resolution + 1) * (resolution + 1));
        for y in 0..=resolution {
            for x in 0..=resolution {
                let uv = [x as f64 / resolution as f64, y as f64 / resolution as f64];
                samples.push((self.evaluate(uv).0, uv));
            }
        }
        Ok(WarpInverseField {
            mesh: self.clone(),
            resolution,
            samples,
        })
    }
    fn newton(&self, target: Point, mut uv: Point) -> Option<Point> {
        let tolerance = 1e-9 * self.width.max(self.height).max(1.);
        for _ in 0..40 {
            let (p, a, b) = self.evaluate(uv);
            let e = [p[0] - target[0], p[1] - target[1]];
            if e[0].hypot(e[1]) <= tolerance {
                return unit(uv).then_some(uv);
            }
            let det = a[0] * b[1] - a[1] * b[0];
            if !det.is_finite() || det.abs() <= 1e-14 * a[0].hypot(a[1]) * b[0].hypot(b[1]) {
                return None;
            }
            let step = [
                (b[1] * e[0] - b[0] * e[1]) / det,
                (-a[1] * e[0] + a[0] * e[1]) / det,
            ];
            let mut found = false;
            for i in 0..16 {
                let scale = 0.5_f64.powi(i);
                let next = [
                    (uv[0] - scale * step[0]).clamp(0., 1.),
                    (uv[1] - scale * step[1]).clamp(0., 1.),
                ];
                let q = self.evaluate(next).0;
                if (q[0] - target[0]).hypot(q[1] - target[1]) < e[0].hypot(e[1]) {
                    uv = next;
                    found = true;
                    break;
                }
            }
            if !found {
                return None;
            }
        }
        None
    }
}
/// Immutable sampled surface used as a deterministic Newton fallback.
#[derive(Clone, Debug)]
pub struct WarpInverseField {
    mesh: WarpMesh,
    resolution: usize,
    samples: Vec<(Point, Point)>,
}
impl WarpInverseField {
    pub fn inverse(&self, p: Point) -> Option<Point> {
        if p.iter().any(|v| !v.is_finite()) {
            return None;
        }
        self.mesh
            .newton(
                p,
                [
                    (p[0] / self.mesh.width).clamp(0., 1.),
                    (p[1] / self.mesh.height).clamp(0., 1.),
                ],
            )
            .or_else(|| self.lookup(p))
    }
    fn lookup(&self, p: Point) -> Option<Point> {
        let n = self.resolution + 1;
        for y in 0..self.resolution {
            for x in 0..self.resolution {
                let i = y * n + x;
                for ids in [[i, i + 1, i + n + 1], [i, i + n + 1, i + n]] {
                    let [a, b, c] = ids.map(|i| self.samples[i]);
                    let ab = [b.0[0] - a.0[0], b.0[1] - a.0[1]];
                    let ac = [c.0[0] - a.0[0], c.0[1] - a.0[1]];
                    let d = ab[0] * ac[1] - ab[1] * ac[0];
                    if d.abs() < 1e-20 {
                        continue;
                    }
                    let q = [p[0] - a.0[0], p[1] - a.0[1]];
                    let u = (q[0] * ac[1] - q[1] * ac[0]) / d;
                    let v = (ab[0] * q[1] - ab[1] * q[0]) / d;
                    if u >= -1e-8 && v >= -1e-8 && u + v <= 1. + 1e-8 {
                        let seed = [
                            (a.1[0] * (1. - u - v) + b.1[0] * u + c.1[0] * v).clamp(0., 1.),
                            (a.1[1] * (1. - u - v) + b.1[1] * u + c.1[1] * v).clamp(0., 1.),
                        ];
                        if let Some(uv) = self.mesh.newton(p, seed) {
                            return Some(uv);
                        }
                    }
                }
            }
        }
        // Curved boundaries can protrude beyond raster triangles. Refine nearby
        // vertices too, but never return an unverified nearest-neighbor result.
        let mut nearest: Vec<_> = self
            .samples
            .iter()
            .map(|(q, uv)| ((q[0] - p[0]).hypot(q[1] - p[1]), *uv))
            .collect();
        nearest.sort_by(|a, b| a.0.total_cmp(&b.0));
        nearest
            .iter()
            .take(16)
            .find_map(|(_, uv)| self.mesh.newton(p, *uv))
    }
}
fn lerp(a: Point, b: Point, t: f64) -> Point {
    [a[0] * (1. - t) + b[0] * t, a[1] * (1. - t) + b[1] * t]
}
fn transpose(net: &[Vec<Point>]) -> Vec<Vec<Point>> {
    (0..net[0].len())
        .map(|x| net.iter().map(|r| r[x]).collect())
        .collect()
}
fn unit(p: Point) -> bool {
    p.iter().all(|v| v.is_finite() && (0. ..=1.).contains(v))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn close(a: Point, b: Point) {
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6, "{a:?} != {b:?}");
    }
    #[test]
    fn warp_field_recovers_singular_initial_guess() {
        let mut m = WarpMesh::identity(100., 100.);
        for row in &mut m.control_points {
            for (x, p) in row.iter_mut().enumerate() {
                p[0] = if x == 3 { -100. } else { -200. };
            }
        }
        let uv = [0.37, 0.61];
        let q = m.forward(uv);
        assert!(m.newton(q, [0., 0.61]).is_none());
        close(m.inverse(q).unwrap(), uv);
        let field = m.inverse_field(24).unwrap();
        close(field.inverse(q).unwrap(), uv);
        assert!(field.inverse([-250., 50.]).is_none());
        assert!(m.inverse_field(0).is_err());
    }
    #[test]
    fn warp_subdivide_preserves_shape_and_serde() {
        let mut m = WarpMesh::preset(200., 100., WarpPreset::Wave, 0.4).unwrap();
        let before = m.clone();
        m.subdivide(3, 2).unwrap();
        assert_eq!(m.control_points.len(), 7);
        assert_eq!(m.control_points[0].len(), 10);
        for uv in [[0., 0.], [1., 1.], [0.23, 0.81]] {
            close(m.forward(uv), before.forward(uv));
        }
        let encoded = serde_json::to_string(&m).unwrap();
        let decoded: WarpMesh = serde_json::from_str(&encoded).unwrap();
        decoded.validate().unwrap();
        for (a, b) in m
            .control_points
            .iter()
            .flatten()
            .zip(decoded.control_points.iter().flatten())
        {
            close(*a, *b);
        }
        assert_eq!(m, m.clone());
        assert!(m.subdivide(0, 2).is_err());
        let mut collapsed = WarpMesh::identity(100., 100.);
        for row in &mut collapsed.control_points {
            for p in row {
                p[1] = 0.;
            }
        }
        assert!(collapsed.validate().is_err());
    }
    #[test]
    fn warp_invalid_data_is_rejected() {
        let mut m = WarpMesh::identity(100., 100.);
        m.control_points[0].pop();
        assert!(m.validate().is_err());
        assert!(m.inverse([20., 30.]).is_none());
        assert!(WarpMesh::identity(-1., 1.).validate().is_err());
        let mut m = WarpMesh::default();
        m.control_points[1][2][0] = f64::NAN;
        assert!(m.validate().is_err());
    }
    #[test]
    fn warp_presets_have_zero_identity_and_signed_bend() {
        use WarpPreset::*;
        for preset in [
            Arc, ArcLower, ArcUpper, Arch, Bulge, Shell, Flag, Wave, Fish, Rise, Fisheye, Inflate,
            Squeeze, Twist,
        ] {
            let zero = WarpMesh::preset(200., 100., preset, 0.).unwrap();
            assert_eq!(zero, WarpMesh::identity(200., 100.));
            let positive = WarpMesh::preset(200., 100., preset, 0.25).unwrap();
            let negative = WarpMesh::preset(200., 100., preset, -0.25).unwrap();
            assert_ne!(positive, zero);
            assert_ne!(positive, negative);
            for uv in [[0.2, 0.3], [0.5, 0.5], [0.8, 0.7]] {
                close(positive.inverse(positive.forward(uv)).unwrap(), uv);
            }
        }
        assert!(WarpMesh::preset(100., 100., Arc, f64::NAN).is_err());
        assert!(WarpMesh::preset(100., 100., Arc, 1.1).is_err());
    }
    #[test]
    fn warp_split_preserves_curved_surface() {
        let mut m = WarpMesh::identity(200., 100.);
        m.control_points[1][1] = [20., -80.];
        let original = m.clone();
        m.split_u(0.3).unwrap();
        m.split_v(0.6).unwrap();
        m.split_u(0.8).unwrap();
        assert_eq!(m.control_points.len(), 7);
        assert_eq!(m.control_points[0].len(), 10);
        for y in 0..21 {
            for x in 0..21 {
                let uv = [x as f64 / 20., y as f64 / 20.];
                close(m.forward(uv), original.forward(uv));
            }
        }
        let saved = m.clone();
        assert!(m.split_u(0.3).is_err());
        assert_eq!(saved, m);
        assert!(m.split_v(f64::NAN).is_err());
    }
    #[test]
    fn warp_identity_roundtrip() {
        let mesh = WarpMesh::identity(200., 100.);
        mesh.validate().unwrap();
        close(mesh.forward([0.3, 0.7]), [60., 70.]);
        close(mesh.inverse([60., 70.]).unwrap(), [0.3, 0.7]);
        assert_eq!(mesh.inverse([-10., 30.]), None);
    }
}
