use crate::*;
use kurbo::ParamCurve;
/// Row-major homography. Mapping fails on its horizon.
#[derive(Clone, Copy, Debug)]
pub struct Perspective(pub [f64; 9]);
impl Perspective {
    pub fn from_quads(source: [Point; 4], destination: [Point; 4]) -> Result<Self> {
        fn unit(q: [Point; 4]) -> Result<Perspective> {
            let dx = q[0].x - q[1].x + q[2].x - q[3].x;
            let dy = q[0].y - q[1].y + q[2].y - q[3].y;
            let a = q[1] - q[2];
            let b = q[3] - q[2];
            let det = a.cross(b);
            if !det.is_finite() || det.abs() < 1e-12 {
                return Err(Error::Invalid("degenerate quad"));
            }
            let g = (dx * b.y - dy * b.x) / det;
            let h = (a.x * dy - a.y * dx) / det;
            let m = Perspective([
                q[1].x - q[0].x + g * q[1].x,
                q[3].x - q[0].x + h * q[3].x,
                q[0].x,
                q[1].y - q[0].y + g * q[1].y,
                q[3].y - q[0].y + h * q[3].y,
                q[0].y,
                g,
                h,
                1.,
            ]);
            m.inverse()?;
            Ok(m)
        }
        let a = unit(destination)?.0;
        let b = unit(source)?.inverse()?.0;
        let mut m = [0.; 9];
        for r in 0..3 {
            for c in 0..3 {
                m[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
            }
        }
        Ok(Self(m))
    }
    pub fn inverse(self) -> Result<Self> {
        let [a, b, c, d, e, f, g, h, i] = self.0;
        let det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g);
        if !det.is_finite() || det.abs() < 1e-14 {
            return Err(Error::Invalid("singular perspective"));
        }
        Ok(Self(
            [
                e * i - f * h,
                c * h - b * i,
                b * f - c * e,
                f * g - d * i,
                a * i - c * g,
                c * d - a * f,
                d * h - e * g,
                b * g - a * h,
                a * e - b * d,
            ]
            .map(|v| v / det),
        ))
    }
    pub fn map(self, p: Point) -> Result<Point> {
        let m = self.0;
        let z = m[6] * p.x + m[7] * p.y + m[8];
        if z.abs() < 1e-14 {
            return Err(Error::Invalid("perspective horizon"));
        }
        finite(Point::new(
            (m[0] * p.x + m[1] * p.y + m[2]) / z,
            (m[3] * p.x + m[4] * p.y + m[5]) / z,
        ))
    }
}
fn finite(p: Point) -> Result<Point> {
    if p.x.is_finite() && p.y.is_finite() {
        Ok(p)
    } else {
        Err(Error::Invalid("nonfinite transform"))
    }
}
pub trait Warp {
    fn map_point(&self, p: Point) -> Result<Point>;
}
impl Warp for Perspective {
    fn map_point(&self, p: Point) -> Result<Point> {
        self.map(p)
    }
}
/// Tensor-product bicubic patches; each [row][column] point is editable.
/// Shared patch edges must be edited together to preserve continuity.
#[derive(Clone, Debug)]
pub struct MeshWarp {
    pub domain: Rect,
    pub columns: usize,
    pub rows: usize,
    pub patches: Vec<[[Point; 4]; 4]>,
}
impl MeshWarp {
    pub fn identity(domain: Rect, columns: usize, rows: usize) -> Result<Self> {
        if domain.width() <= 0.
            || domain.height() <= 0.
            || ![domain.x0, domain.y0, domain.x1, domain.y1]
                .iter()
                .all(|v| v.is_finite())
            || columns == 0
            || rows == 0
            || columns > 64
            || rows > 64
        {
            return Err(Error::Invalid("mesh domain"));
        }
        let mut patches = vec![];
        for y in 0..rows {
            for x in 0..columns {
                patches.push(std::array::from_fn(|j| {
                    std::array::from_fn(|i| {
                        Point::new(
                            domain.x0
                                + domain.width() * (x as f64 + i as f64 / 3.) / columns as f64,
                            domain.y0 + domain.height() * (y as f64 + j as f64 / 3.) / rows as f64,
                        )
                    })
                }));
            }
        }
        Ok(Self {
            domain,
            columns,
            rows,
            patches,
        })
    }
    pub fn map(&self, p: Point) -> Result<Point> {
        if self.columns == 0
            || self.rows == 0
            || self.columns.checked_mul(self.rows) != Some(self.patches.len())
            || self.domain.width() <= 0.
            || self.domain.height() <= 0.
        {
            return Err(Error::Invalid("mesh"));
        }
        let u = (p.x - self.domain.x0) / self.domain.width();
        let v = (p.y - self.domain.y0) / self.domain.height();
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return Err(Error::Invalid("point outside mesh"));
        }
        let x = (u * self.columns as f64)
            .floor()
            .min((self.columns - 1) as f64) as usize;
        let y = (v * self.rows as f64).floor().min((self.rows - 1) as f64) as usize;
        let basis = |t: f64| {
            [
                (1. - t).powi(3),
                3. * t * (1. - t).powi(2),
                3. * t * t * (1. - t),
                t.powi(3),
            ]
        };
        let bx = basis(u * self.columns as f64 - x as f64);
        let by = basis(v * self.rows as f64 - y as f64);
        let mut out = Point::ZERO;
        for (j, wy) in by.iter().enumerate() {
            for (i, wx) in bx.iter().enumerate() {
                out += self.patches[y * self.columns + x][j][i].to_vec2() * *wx * *wy;
            }
        }
        finite(out)
    }
    /// Newton solve for locally invertible meshes; folds/nonconvergence return errors.
    pub fn inverse_map(&self, target: Point, tolerance: f64) -> Result<Point> {
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(Error::Invalid("inverse tolerance"));
        }
        let mut p = self.domain.center();
        let ex = self.domain.width() * 1e-6;
        let ey = self.domain.height() * 1e-6;
        for _ in 0..40 {
            let q = self.map(p)?;
            let r = q - target;
            if r.hypot() <= tolerance {
                return Ok(p);
            }
            let dx = if p.x + ex <= self.domain.x1 { ex } else { -ex };
            let dy = if p.y + ey <= self.domain.y1 { ey } else { -ey };
            let a = (self.map(p + Vec2::new(dx, 0.))? - q) / dx;
            let b = (self.map(p + Vec2::new(0., dy))? - q) / dy;
            let det = a.cross(b);
            if det.abs() < 1e-14 {
                break;
            }
            p -= Vec2::new(r.cross(b) / det, a.cross(r) / det);
            p.x = p.x.clamp(self.domain.x0, self.domain.x1);
            p.y = p.y.clamp(self.domain.y0, self.domain.y1);
        }
        Err(Error::Invalid("mesh inverse did not converge"))
    }
}
impl Warp for MeshWarp {
    fn map_point(&self, p: Point) -> Result<Point> {
        self.map(p)
    }
}
impl Path {
    /// Adaptive output-space subdivision. Non-affine cubics become polylines.
    pub fn warp(&self, warp: &impl Warp, tolerance: f64) -> Result<Self> {
        self.validate()?;
        if !tolerance.is_finite() || tolerance <= 0. {
            return Err(Error::Invalid("warp tolerance"));
        }
        fn subdivide(
            c: kurbo::CubicBez,
            w: &impl Warp,
            t: f64,
            depth: u8,
            out: &mut Vec<Point>,
        ) -> Result<()> {
            let a = w.map_point(c.p0)?;
            let b = w.map_point(c.p3)?;
            let mut error = 0.0_f64;
            for u in [0.25, 0.5, 0.75] {
                error = error.max(w.map_point(c.eval(u))?.distance(a.lerp(b, u)));
            }
            if error <= t {
                out.push(b);
                return Ok(());
            }
            if depth == 20 {
                return Err(Error::Invalid("warp subdivision limit"));
            }
            let (l, r) = c.subdivide();
            subdivide(l, w, t, depth + 1, out)?;
            subdivide(r, w, t, depth + 1, out)
        }
        let mut out = Path::default().with_rule(self.fill_rule);
        for s in &self.subpaths {
            if s.anchors.is_empty() {
                continue;
            }
            let mut points = vec![warp.map_point(s.anchors[0].point)?];
            let n = if s.closed {
                s.anchors.len()
            } else {
                s.anchors.len() - 1
            };
            for i in 0..n {
                let a = s.anchors[i];
                let b = s.anchors[(i + 1) % s.anchors.len()];
                subdivide(
                    kurbo::CubicBez::new(a.point, a.outgoing, b.incoming, b.point),
                    warp,
                    tolerance,
                    0,
                    &mut points,
                )?;
            }
            if s.closed {
                points.pop();
            }
            out.subpaths
                .extend(Path::polyline(&points, s.closed).subpaths);
        }
        Ok(out)
    }
}
/// Explicit placeholder, never silently substitutes ordinary scaling.
pub fn content_aware_scale(_path: &Path, _size: Vec2) -> Result<Path> {
    Err(Error::Invalid(
        "content-aware scale requires raster seam carving",
    ))
}
