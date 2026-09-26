use crate::{Error, Point, Result};
use lens::Homography;
use serde::{Deserialize, Serialize};

/// Quad corners must follow the perimeter (either winding). Corresponding
/// corners identify source/destination vertices. Coordinates are image pixels.
pub type Quad = [Point; 4];
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PerspectiveWarp {
    pub source_quads: Vec<Quad>,
    pub destination_quads: Vec<Quad>,
}
impl PerspectiveWarp {
    pub fn from_quads(source_quads: Vec<Quad>, destination_quads: Vec<Quad>) -> Result<Self> {
        let warp = Self {
            source_quads,
            destination_quads,
        };
        warp.validate()?;
        Ok(warp)
    }
    pub fn validate(&self) -> Result<()> {
        if self.source_quads.is_empty() || self.source_quads.len() != self.destination_quads.len() {
            return Err(Error::Invalid(
                "perspective requires paired nonempty quads".into(),
            ));
        }
        for q in self.source_quads.iter().chain(&self.destination_quads) {
            if !valid_quad(q) || unit_homography(q).and_then(Homography::inverse).is_none() {
                return Err(Error::Invalid(
                    "perspective quad must be finite, strictly convex and nonsingular".into(),
                ));
            }
        }
        for i in 0..self.source_quads.len() {
            for j in i + 1..self.source_quads.len() {
                let a = &self.source_quads[i];
                let b = &self.source_quads[j];
                let c = &self.destination_quads[i];
                let d = &self.destination_quads[j];
                for x in 0..4 {
                    for y in 0..4 {
                        if same(a[x], b[y]) != same(c[x], d[y]) {
                            return Err(Error::Invalid(
                                "shared quad vertices must remain shared in both spaces".into(),
                            ));
                        }
                    }
                }
                if overlaps(a, b) || overlaps(c, d) {
                    return Err(Error::Invalid(
                        "perspective quad interiors may not overlap".into(),
                    ));
                }
                if t_junction(a, b) || t_junction(b, a) || t_junction(c, d) || t_junction(d, c) {
                    return Err(Error::Invalid(
                        "split quads at T-junctions before warping".into(),
                    ));
                }
            }
        }
        Ok(())
    }
    /// Compile per-quad lens homographies once for use in a pixel loop.
    pub fn prepare(&self) -> Result<PreparedPerspectiveWarp> {
        self.validate()?;
        let mut planes = Vec::new();
        for (index, (src, dst)) in self
            .source_quads
            .iter()
            .zip(&self.destination_quads)
            .enumerate()
        {
            let h = compose(
                unit_homography(dst).unwrap(),
                unit_homography(src).unwrap().inverse().unwrap(),
            );
            let inverse = h
                .inverse()
                .ok_or_else(|| Error::Invalid("singular perspective mapping".into()))?;
            let shared = std::array::from_fn(|edge| {
                self.source_quads.iter().enumerate().any(|(other, q)| {
                    other != index
                        && (0..4).any(|e| {
                            (same(src[edge], q[e]) && same(src[(edge + 1) % 4], q[(e + 1) % 4]))
                                || (same(src[edge], q[(e + 1) % 4])
                                    && same(src[(edge + 1) % 4], q[e]))
                        })
                })
            });
            planes.push(Plane {
                source: *src,
                destination: *dst,
                homography: h,
                inverse,
                shared,
            });
        }
        Ok(PreparedPerspectiveWarp { planes })
    }
    pub fn forward(&self, source: Point) -> Option<Point> {
        self.prepare().ok()?.forward(source)
    }
    /// Map destination pixels to source pixels; outside all planes returns None.
    pub fn inverse(&self, destination: Point) -> Option<Point> {
        self.prepare().ok()?.inverse(destination)
    }
}
#[derive(Clone, Debug)]
struct Plane {
    source: Quad,
    destination: Quad,
    homography: Homography,
    inverse: Homography,
    shared: [bool; 4],
}
#[derive(Clone, Debug)]
pub struct PreparedPerspectiveWarp {
    planes: Vec<Plane>,
}
impl PreparedPerspectiveWarp {
    pub fn forward(&self, p: Point) -> Option<Point> {
        self.planes.iter().find_map(|q| {
            if !contains(&q.source, p) {
                return None;
            }
            if !q.shared.iter().any(|v| *v) {
                return q.homography.map(p);
            }
            q.map_uv(bilinear_inverse(&q.source, p)?)
        })
    }
    pub fn inverse(&self, p: Point) -> Option<Point> {
        self.planes.iter().find_map(|q| {
            if !contains(&q.destination, p) {
                return None;
            }
            if !q.shared.iter().any(|v| *v) {
                return q.inverse.map(p);
            }
            let seed = q
                .inverse
                .map(p)
                .and_then(|p| bilinear_inverse(&q.source, p))
                .unwrap_or([0.5, 0.5]);
            let f = |uv| q.map_uv(uv);
            let uv = solve_uv(&f, p, seed).or_else(|| {
                for y in 0..=4 {
                    for x in 0..=4 {
                        if let Some(uv) = solve_uv(&f, p, [x as f64 / 4., y as f64 / 4.]) {
                            return Some(uv);
                        }
                    }
                }
                None
            })?;
            Some(bilinear(&q.source, uv))
        })
    }
}
impl Plane {
    fn map_uv(&self, uv: Point) -> Option<Point> {
        let mut result = self.homography.map(bilinear(&self.source, uv))?;
        let [u, v] = uv;
        // Coons-style, linear-in-the-cross-edge-coordinate blending of boundary
        // residuals. Each shared edge becomes linearly parameterized in both
        // planes, while unrelated edges retain the original projective mapping.
        for (edge, (t, weight)) in [(u, 1. - v), (v, u), (1. - u, v), (1. - v, 1. - u)]
            .into_iter()
            .enumerate()
        {
            if !self.shared[edge] {
                continue;
            }
            let a = edge;
            let b = (edge + 1) % 4;
            let source = mix(self.source[a], self.source[b], t);
            let projective = self.homography.map(source)?;
            let linear = mix(self.destination[a], self.destination[b], t);
            for k in 0..2 {
                result[k] += weight * (linear[k] - projective[k]);
            }
        }
        result.iter().all(|v| v.is_finite()).then_some(result)
    }
}
fn mix(a: Point, b: Point, t: f64) -> Point {
    [a[0] * (1. - t) + b[0] * t, a[1] * (1. - t) + b[1] * t]
}
fn bilinear(q: &Quad, uv: Point) -> Point {
    mix(mix(q[0], q[1], uv[0]), mix(q[3], q[2], uv[0]), uv[1])
}
fn bilinear_inverse(q: &Quad, p: Point) -> Option<Point> {
    solve_uv(&|uv| Some(bilinear(q, uv)), p, [0.5, 0.5])
}
fn solve_uv(f: &impl Fn(Point) -> Option<Point>, p: Point, mut uv: Point) -> Option<Point> {
    for _ in 0..40 {
        let q = f(uv)?;
        let e = [q[0] - p[0], q[1] - p[1]];
        let distance = e[0].hypot(e[1]);
        if distance < 1e-8 {
            return Some(uv);
        }
        let h = 1e-5;
        let a0 = f([uv[0] - h, uv[1]])?;
        let a1 = f([uv[0] + h, uv[1]])?;
        let b0 = f([uv[0], uv[1] - h])?;
        let b1 = f([uv[0], uv[1] + h])?;
        let a = [(a1[0] - a0[0]) / (2. * h), (a1[1] - a0[1]) / (2. * h)];
        let b = [(b1[0] - b0[0]) / (2. * h), (b1[1] - b0[1]) / (2. * h)];
        let d = a[0] * b[1] - a[1] * b[0];
        if !d.is_finite() || d.abs() < 1e-14 {
            return None;
        }
        let step = [
            (b[1] * e[0] - b[0] * e[1]) / d,
            (a[0] * e[1] - a[1] * e[0]) / d,
        ];
        let mut improved = false;
        for i in 0..16 {
            let s = 0.5_f64.powi(i);
            let next = [
                (uv[0] - s * step[0]).clamp(0., 1.),
                (uv[1] - s * step[1]).clamp(0., 1.),
            ];
            let q = f(next)?;
            if (q[0] - p[0]).hypot(q[1] - p[1]) < distance {
                uv = next;
                improved = true;
                break;
            }
        }
        if !improved {
            return None;
        }
    }
    None
}
fn same(a: Point, b: Point) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-8
}
fn t_junction(a: &Quad, b: &Quad) -> bool {
    a.iter().any(|p| {
        (0..4).any(|i| {
            let x = b[i];
            let y = b[(i + 1) % 4];
            if same(*p, x) || same(*p, y) {
                return false;
            }
            let dx = y[0] - x[0];
            let dy = y[1] - x[1];
            let length = dx.hypot(dy);
            let t = ((p[0] - x[0]) * dx + (p[1] - x[1]) * dy) / (length * length);
            t > 0. && t < 1. && cross(x, y, *p).abs() <= 1e-8 * length
        })
    })
}
fn overlaps(a: &Quad, b: &Quad) -> bool {
    for q in [a, b] {
        for i in 0..4 {
            let x = q[i];
            let y = q[(i + 1) % 4];
            let length = (y[0] - x[0]).hypot(y[1] - x[1]);
            let axis = [(x[1] - y[1]) / length, (y[0] - x[0]) / length];
            let interval = |q: &Quad| {
                q.iter()
                    .map(|p| p[0] * axis[0] + p[1] * axis[1])
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), v| {
                        (lo.min(v), hi.max(v))
                    })
            };
            let (al, ah) = interval(a);
            let (bl, bh) = interval(b);
            if ah.min(bh) - al.max(bl) <= 1e-8 {
                return false;
            }
        }
    }
    true
}
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn valid_quad(q: &Quad) -> bool {
    if q.iter().flatten().any(|v| !v.is_finite()) {
        return false;
    }
    let area = cross(q[0], q[1], q[2]);
    let scale = q
        .iter()
        .enumerate()
        .map(|(i, a)| {
            let b = q[(i + 1) % 4];
            (a[0] - b[0]).hypot(a[1] - b[1])
        })
        .fold(0., f64::max);
    area.is_finite()
        && scale > 0.
        && (0..4).all(|i| {
            cross(q[i], q[(i + 1) % 4], q[(i + 2) % 4]) * area.signum() > 1e-12 * scale * scale
        })
}
fn contains(q: &Quad, p: Point) -> bool {
    if p.iter().any(|v| !v.is_finite()) {
        return false;
    }
    let sign = cross(q[0], q[1], q[2]).signum();
    (0..4).all(|i| {
        let a = q[i];
        let b = q[(i + 1) % 4];
        cross(a, b, p) * sign >= -1e-9 * (b[0] - a[0]).hypot(b[1] - a[1]).max(1.)
    })
}
fn compose(a: Homography, b: Homography) -> Homography {
    Homography(std::array::from_fn(|i| {
        std::array::from_fn(|j| (0..3).map(|k| a.0[i][k] * b.0[k][j]).sum())
    }))
}
fn unit_homography(q: &Quad) -> Option<Homography> {
    let dx1 = q[1][0] - q[2][0];
    let dx2 = q[3][0] - q[2][0];
    let dx3 = q[0][0] - q[1][0] + q[2][0] - q[3][0];
    let dy1 = q[1][1] - q[2][1];
    let dy2 = q[3][1] - q[2][1];
    let dy3 = q[0][1] - q[1][1] + q[2][1] - q[3][1];
    let d = dx1 * dy2 - dx2 * dy1;
    if !d.is_finite() || d == 0. {
        return None;
    }
    let g = (dx3 * dy2 - dx2 * dy3) / d;
    let h = (dx1 * dy3 - dx3 * dy1) / d;
    let m = Homography([
        [
            q[1][0] - q[0][0] + g * q[1][0],
            q[3][0] - q[0][0] + h * q[3][0],
            q[0][0],
        ],
        [
            q[1][1] - q[0][1] + g * q[1][1],
            q[3][1] - q[0][1] + h * q[3][1],
            q[0][1],
        ],
        [g, h, 1.],
    ]);
    if [1., 1. + g, 1. + h, 1. + g + h]
        .iter()
        .any(|d| *d <= 1e-12 || !d.is_finite())
    {
        return None;
    }
    m.0.iter().flatten().all(|v| v.is_finite()).then_some(m)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn rect(x: f64, width: f64) -> [Point; 4] {
        [[x, 0.], [x + width, 0.], [x + width, 100.], [x, 100.]]
    }
    fn close(a: Point, b: Point) {
        assert!((a[0] - b[0]).hypot(a[1] - b[1]) < 1e-6, "{a:?} != {b:?}");
    }
    #[test]
    fn perspective_shared_edge_is_continuous_with_bilinear_blending() {
        let dst = vec![
            [[0., 0.], [90., 15.], [110., 110.], [0., 80.]],
            [[90., 15.], [210., -10.], [180., 150.], [110., 110.]],
        ];
        let w = PerspectiveWarp::from_quads(vec![rect(0., 100.), rect(100., 100.)], dst).unwrap();
        let prepared = w.prepare().unwrap();
        for y in 0..=10 {
            let t = y as f64 / 10.;
            let expected = [90. + 20. * t, 15. + 95. * t];
            close(prepared.forward([100., 100. * t]).unwrap(), expected);
            close(prepared.inverse(expected).unwrap(), [100., 100. * t]);
            let l = prepared.forward([100. - 1e-7, 100. * t]).unwrap();
            let r = prepared.forward([100. + 1e-7, 100. * t]).unwrap();
            close(l, r);
        }
        for y in 0..11 {
            for x in 0..21 {
                let p = [x as f64 * 10., y as f64 * 10.];
                close(prepared.inverse(prepared.forward(p).unwrap()).unwrap(), p);
            }
        }
    }
    #[test]
    fn perspective_rejects_cracked_shared_vertices_and_bad_quads() {
        let src = vec![rect(0., 100.), rect(100., 100.)];
        let mut dst = src.clone();
        dst[1][0][0] += 10.;
        assert!(PerspectiveWarp::from_quads(src.clone(), dst).is_err());
        assert!(
            PerspectiveWarp::from_quads(
                vec![rect(0., 100.)],
                vec![[[0., 0.], [100., 100.], [0., 100.], [100., 0.]]]
            )
            .is_err()
        );
        assert!(PerspectiveWarp::from_quads(vec![rect(0., 100.)], vec![[[0., 0.]; 4]]).is_err());
        assert!(PerspectiveWarp::from_quads(vec![], vec![]).is_err());
        let overlapping = vec![rect(0., 100.), rect(50., 100.)];
        assert!(PerspectiveWarp::from_quads(overlapping.clone(), overlapping).is_err());
    }
    #[test]
    fn perspective_four_planes_share_corner_and_serialize() {
        let nodes = [
            [[0., 0.], [100., -10.], [200., 0.]],
            [[5., 100.], [110., 95.], [205., 110.]],
            [[0., 200.], [90., 215.], [200., 200.]],
        ];
        let mut source = Vec::new();
        let mut destination = Vec::new();
        for y in 0..2 {
            for x in 0..2 {
                source.push([
                    [x as f64 * 100., y as f64 * 100.],
                    [(x + 1) as f64 * 100., y as f64 * 100.],
                    [(x + 1) as f64 * 100., (y + 1) as f64 * 100.],
                    [x as f64 * 100., (y + 1) as f64 * 100.],
                ]);
                destination.push([
                    nodes[y][x],
                    nodes[y][x + 1],
                    nodes[y + 1][x + 1],
                    nodes[y + 1][x],
                ]);
            }
        }
        let warp = PerspectiveWarp::from_quads(source, destination).unwrap();
        let encoded = serde_json::to_string(&warp).unwrap();
        let decoded: PerspectiveWarp = serde_json::from_str(&encoded).unwrap();
        assert_eq!(warp, decoded);
        let ready = decoded.prepare().unwrap();
        for y in 0..21 {
            for x in 0..21 {
                let p = [x as f64 * 10., y as f64 * 10.];
                close(ready.inverse(ready.forward(p).unwrap()).unwrap(), p);
            }
        }
        assert!(ready.inverse([f64::NAN, 0.]).is_none());
        assert!(ready.forward([0., f64::INFINITY]).is_none());
    }
    #[test]
    fn perspective_quad_roundtrip() {
        let dst = [[10., 5.], [180., 20.], [140., 110.], [20., 90.]];
        let warp = PerspectiveWarp::from_quads(vec![rect(0., 100.)], vec![dst]).unwrap();
        warp.validate().unwrap();
        for p in [[0., 0.], [100., 100.], [30., 70.]] {
            close(warp.inverse(warp.forward(p).unwrap()).unwrap(), p);
        }
        assert_eq!(warp.inverse([-100., -100.]), None);
    }
}
