//! Perspective editing in a shared, unfolded plane-coordinate atlas.
//! All mappings are true homographies: no bilinear seam correction is used.
use crate::{Error, Image, Kernel, Point, Result, sample};
use lens::Homography;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

pub type Quad = [Point; 4];

/// Pinhole intrinsics in canvas pixels; square pixels, zero skew, +Z forward.
/// The default is explicit, not an estimate of an image's real camera.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    pub focal_length: f64,
    pub principal_point: Point,
}
impl Default for Camera {
    fn default() -> Self {
        Self {
            focal_length: 1000.,
            principal_point: [0., 0.],
        }
    }
}
impl Camera {
    pub fn validate(&self) -> Result<()> {
        if !self.focal_length.is_finite()
            || self.focal_length <= 0.
            || self.principal_point.iter().any(|v| !v.is_finite())
        {
            return Err(invalid("invalid pinhole camera intrinsics"));
        }
        Ok(())
    }
}

/// Serializable document payload. Quads may touch only at whole edges or
/// vertices; overlapping interiors and T-junctions are deliberately rejected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct VanishingPoint {
    pub planes: Vec<PlaneSpace>,
    pub camera: Camera,
}
impl VanishingPoint {
    /// Resample a flat premultiplied image into a transparent canvas overlay.
    /// `origin` is its top-left corner in the unfolded atlas, `pixel_size` is
    /// positive atlas units per source texel. Coordinates use pixel centers.
    /// This does not composite onto a background or alter source alpha.
    pub fn paste(
        &self,
        source: &Image,
        width: usize,
        height: usize,
        origin: Point,
        pixel_size: Point,
        kernel: Kernel,
    ) -> Result<Image> {
        source.validate()?;
        if origin.iter().any(|v| !v.is_finite())
            || pixel_size.iter().any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err(invalid(
                "paste requires finite origin and positive pixel size",
            ));
        }
        let n = width
            .checked_mul(height)
            .filter(|n| *n > 0 && *n <= 100_000_000)
            .ok_or_else(|| invalid("paste output must be nonempty and at most 100 MP"))?;
        let ready = self.prepare()?;
        let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| vec![0.; n]);
        let [r, g, b, a] = &mut planes;
        r.par_iter_mut()
            .zip(g.par_iter_mut())
            .zip(b.par_iter_mut())
            .zip(a.par_iter_mut())
            .enumerate()
            .for_each(|(i, (((r, g), b), a))| {
                let rgba = ready
                    .canvas_to_plane([(i % width) as f64 + 0.5, (i / width) as f64 + 0.5])
                    .map(|p| {
                        sample::sample(
                            source,
                            [
                                ((p[0] - origin[0]) / pixel_size[0]) as f32,
                                ((p[1] - origin[1]) / pixel_size[1]) as f32,
                            ],
                            kernel,
                        )
                    })
                    .unwrap_or([0.; 4]);
                *r = rgba[0];
                *g = rgba[1];
                *b = rgba[2];
                *a = rgba[3];
            });
        Image::new(width, height, planes)
    }

    /// Extend an edge into a new plane, rotating its camera-space continuation
    /// about the directed edge by a signed right-handed angle in degrees.
    /// Zero means coplanar; 90 means perpendicular. Width is in atlas units.
    /// K^-1 H lifts the existing plane into camera space up to common scale;
    /// Rodrigues rotation of its outward derivative preserves every hinge point.
    /// No camera calibration is inferred: the supplied intrinsics define the
    /// reconstruction. Horizon crossings, edge-on and occluding folds fail.
    pub fn tear_off(
        &mut self,
        parent: usize,
        edge: usize,
        width: f64,
        angle_degrees: f64,
    ) -> Result<usize> {
        let prepared = self.prepare()?;
        if edge >= 4
            || parent >= self.planes.len()
            || !width.is_finite()
            || width <= 0.
            || !angle_degrees.is_finite()
            || angle_degrees.abs() > 180.
        {
            return Err(invalid("invalid tear-off parent, edge, width or angle"));
        }
        let plane = &prepared.planes[parent];
        let a = plane.plane_quad[edge];
        let b = plane.plane_quad[(edge + 1) % 4];
        let length = (b[0] - a[0]).hypot(b[1] - a[1]);
        let tangent = [(b[0] - a[0]) / length, (b[1] - a[1]) / length];
        let sign = cross(
            plane.plane_quad[0],
            plane.plane_quad[1],
            plane.plane_quad[2],
        )
        .signum();
        let n = [sign * tangent[1], -sign * tangent[0]];
        let atlas = [
            a,
            b,
            [b[0] + n[0] * width, b[1] + n[1] * width],
            [a[0] + n[0] * width, a[1] + n[1] * width],
        ];
        let f = self.camera.focal_length;
        let [cx, cy] = self.camera.principal_point;
        let k = Homography([[f, 0., cx], [0., f, cy], [0., 0., 1.]]);
        let ki = Homography([[1. / f, 0., -cx / f], [0., 1. / f, -cy / f], [0., 0., 1.]]);
        let mut lift = compose(ki, plane.forward).0;
        // Homographies have arbitrary sign. Orient the entire lift toward +Z.
        let depth = lift[2][0] * a[0] + lift[2][1] * a[1] + lift[2][2];
        if depth < 0. {
            for row in &mut lift {
                for v in row {
                    *v = -*v;
                }
            }
        }
        let axis = std::array::from_fn(|i| lift[i][0] * tangent[0] + lift[i][1] * tangent[1]);
        let norm = dot3(axis, axis).sqrt();
        if !norm.is_finite() || norm <= 1e-12 {
            return Err(invalid("degenerate camera-space hinge"));
        }
        let axis = axis.map(|v| v / norm);
        let v = std::array::from_fn(|i| lift[i][0] * n[0] + lift[i][1] * n[1]);
        let (s, c) = angle_degrees.to_radians().sin_cos();
        let cross = [
            axis[1] * v[2] - axis[2] * v[1],
            axis[2] * v[0] - axis[0] * v[2],
            axis[0] * v[1] - axis[1] * v[0],
        ];
        let delta: [f64; 3] = std::array::from_fn(|i| {
            v[i] * c + cross[i] * s + axis[i] * dot3(axis, v) * (1. - c) - v[i]
        });
        // Rank-one update vanishes on n dot (p-a) == 0 (the shared edge).
        for i in 0..3 {
            lift[i][0] += delta[i] * n[0];
            lift[i][1] += delta[i] * n[1];
            lift[i][2] -= delta[i] * (n[0] * a[0] + n[1] * a[1]);
        }
        if atlas.iter().any(|p| {
            let z = lift[2][0] * p[0] + lift[2][1] * p[1] + lift[2][2];
            !z.is_finite() || z <= 1e-12
        }) {
            return Err(invalid("tear-off crosses or lies behind the camera"));
        }
        let h = compose(k, Homography(lift));
        let mut quad = [[0.; 2]; 4];
        for i in 0..4 {
            quad[i] = h.map(atlas[i]).ok_or_else(|| invalid("tear-off horizon"))?;
        }
        // Preserve exact serialized endpoint identity rather than roundoff.
        quad[0] = plane.canvas_quad[edge];
        quad[1] = plane.canvas_quad[(edge + 1) % 4];
        let mut candidate = self.clone();
        candidate.planes.push(PlaneSpace {
            canvas_quad: quad,
            plane_quad: atlas,
        });
        candidate.validate()?;
        let index = self.planes.len();
        *self = candidate;
        Ok(index)
    }

    pub fn validate(&self) -> Result<()> {
        self.prepare().map(|_| ())
    }
    pub fn prepare(&self) -> Result<PreparedVanishingPoint> {
        self.camera.validate()?;
        if self.planes.is_empty() {
            return Err(invalid("at least one plane is required"));
        }
        let planes: Vec<_> = self
            .planes
            .iter()
            .map(PlaneSpace::prepare)
            .collect::<Result<_>>()?;
        for (i, a) in planes.iter().enumerate() {
            for b in &planes[i + 1..] {
                if overlaps(&a.canvas_quad, &b.canvas_quad)
                    || overlaps(&a.plane_quad, &b.plane_quad)
                {
                    return Err(invalid(
                        "plane interiors may not overlap in canvas or atlas",
                    ));
                }
                for (qa, qb) in [
                    (&a.canvas_quad, &b.canvas_quad),
                    (&a.plane_quad, &b.plane_quad),
                ] {
                    if t_junction(qa, qb) || t_junction(qb, qa) {
                        return Err(invalid("split planes at T-junctions before editing"));
                    }
                }
                for x in 0..4 {
                    for y in 0..4 {
                        if same(a.canvas_quad[x], b.canvas_quad[y])
                            != same(a.plane_quad[x], b.plane_quad[y])
                        {
                            return Err(invalid("shared vertices must match in canvas and atlas"));
                        }
                        let x1 = (x + 1) % 4;
                        let y1 = (y + 1) % 4;
                        if (same(a.plane_quad[x], b.plane_quad[y])
                            && same(a.plane_quad[x1], b.plane_quad[y1]))
                            || (same(a.plane_quad[x], b.plane_quad[y1])
                                && same(a.plane_quad[x1], b.plane_quad[y]))
                        {
                            // Three matching points fix the entire projective line,
                            // not merely its endpoints (which permit seam cracks).
                            let p = mix(a.plane_quad[x], a.plane_quad[x1], 0.5);
                            if !same(
                                a.forward.map(p).ok_or_else(|| invalid("seam horizon"))?,
                                b.forward.map(p).ok_or_else(|| invalid("seam horizon"))?,
                            ) {
                                return Err(invalid(
                                    "shared edge projective parameterizations differ",
                                ));
                            }
                        }
                    }
                }
            }
        }
        Ok(PreparedVanishingPoint { planes })
    }
    /// Offset is source minus destination in the shared unfolded plane atlas.
    /// Returns None outside the atlas, or for any invalid document/input.
    /// Brush loops should call prepare() once and reuse its clone_source().
    pub fn clone_source(&self, canvas: Point, offset: Point) -> Option<Point> {
        self.prepare().ok()?.clone_source(canvas, offset)
    }
}
#[derive(Clone, Debug)]
pub struct PreparedVanishingPoint {
    planes: Vec<PreparedPlaneSpace>,
}
/// A regular atlas-space brush dab; None explicitly marks gaps outside planes.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StrokeSample {
    pub plane: Point,
    pub canvas: Option<Point>,
}
impl PreparedVanishingPoint {
    /// Sample an atlas polyline at uniform arc-length spacing across all planes.
    /// Phase carries across vertices and plane boundaries; the final endpoint
    /// is included once. Callers must not connect across `canvas == None` gaps.
    /// This provides dab centers, not brush coverage/rasterization. Maximum 1M.
    pub fn plane_stroke(&self, path: &[Point], spacing: f64) -> Result<Vec<StrokeSample>> {
        if !spacing.is_finite() || spacing <= 0. || path.iter().flatten().any(|v| !v.is_finite()) {
            return Err(invalid(
                "stroke requires finite points and positive spacing",
            ));
        }
        if path.is_empty() {
            return Ok(Vec::new());
        }
        let lengths: Vec<_> = path
            .windows(2)
            .map(|p| (p[1][0] - p[0][0]).hypot(p[1][1] - p[0][1]))
            .collect();
        let total: f64 = lengths.iter().sum();
        let count = (total / spacing).floor();
        if !total.is_finite() || !count.is_finite() || count > 999_998. {
            return Err(invalid("stroke exceeds one million samples"));
        }
        let mut result = Vec::with_capacity(count as usize + 2);
        let mut segment = 0;
        let mut start = 0.;
        for i in 0..=count as usize {
            let distance = i as f64 * spacing;
            while segment < lengths.len()
                && (lengths[segment] == 0. || start + lengths[segment] < distance)
            {
                start += lengths[segment];
                segment += 1;
            }
            let p = if segment < lengths.len() {
                mix(
                    path[segment],
                    path[segment + 1],
                    ((distance - start) / lengths[segment]).clamp(0., 1.),
                )
            } else {
                *path.last().unwrap()
            };
            result.push(StrokeSample {
                plane: p,
                canvas: self.plane_to_canvas(p),
            });
        }
        if count * spacing < total {
            let p = *path.last().unwrap();
            result.push(StrokeSample {
                plane: p,
                canvas: self.plane_to_canvas(p),
            });
        }
        Ok(result)
    }

    pub fn canvas_to_plane(&self, p: Point) -> Option<Point> {
        self.planes
            .iter()
            .find_map(|plane| plane.canvas_to_plane(p))
    }
    pub fn plane_to_canvas(&self, p: Point) -> Option<Point> {
        self.planes
            .iter()
            .find_map(|plane| plane.plane_to_canvas(p))
    }
    pub fn clone_source(&self, canvas: Point, offset: Point) -> Option<Point> {
        let p = self.canvas_to_plane(canvas)?;
        self.plane_to_canvas([p[0] + offset[0], p[1] + offset[1]])
    }
}
fn same(a: Point, b: Point) -> bool {
    (a[0] - b[0]).hypot(a[1] - b[1]) <= 1e-8
}
fn mix(a: Point, b: Point, t: f64) -> Point {
    [a[0] * (1. - t) + b[0] * t, a[1] * (1. - t) + b[1] * t]
}
fn overlaps(a: &Quad, b: &Quad) -> bool {
    for q in [a, b] {
        for i in 0..4 {
            let p = q[i];
            let r = q[(i + 1) % 4];
            let length = (r[0] - p[0]).hypot(r[1] - p[1]);
            let axis = [(p[1] - r[1]) / length, (r[0] - p[0]) / length];
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

/// Corresponding perimeter-ordered corners in canvas and unfolded plane space.
/// Plane units are user chosen (usually source pixels). Public payloads must be
/// validated after deserialization; prepared mappings are immutable snapshots.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PlaneSpace {
    pub canvas_quad: Quad,
    pub plane_quad: Quad,
}
impl PlaneSpace {
    pub fn from_quad(canvas_quad: Quad, size: Point) -> Result<Self> {
        if size.iter().any(|x| !x.is_finite() || *x <= 0.) {
            return Err(invalid("plane dimensions must be positive and finite"));
        }
        let plane = Self {
            canvas_quad,
            plane_quad: [[0., 0.], [size[0], 0.], size, [0., size[1]]],
        };
        plane.validate()?;
        Ok(plane)
    }
    pub fn validate(&self) -> Result<()> {
        self.prepare().map(|_| ())
    }
    pub fn prepare(&self) -> Result<PreparedPlaneSpace> {
        let canvas = unit_homography(&self.canvas_quad)?;
        let plane = unit_homography(&self.plane_quad)?;
        let forward = compose(
            canvas,
            plane
                .inverse()
                .ok_or_else(|| invalid("singular atlas quad"))?,
        );
        let inverse = forward
            .inverse()
            .ok_or_else(|| invalid("singular plane mapping"))?;
        Ok(PreparedPlaneSpace {
            canvas_quad: self.canvas_quad,
            plane_quad: self.plane_quad,
            forward,
            inverse,
        })
    }
}
#[derive(Clone, Debug)]
pub struct PreparedPlaneSpace {
    canvas_quad: Quad,
    plane_quad: Quad,
    forward: Homography,
    inverse: Homography,
}
impl PreparedPlaneSpace {
    pub fn canvas_to_plane(&self, p: Point) -> Option<Point> {
        contains(&self.canvas_quad, p)
            .then(|| self.inverse.map(p))
            .flatten()
    }
    pub fn plane_to_canvas(&self, p: Point) -> Option<Point> {
        contains(&self.plane_quad, p)
            .then(|| self.forward.map(p))
            .flatten()
    }
}
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
fn cross(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}
fn contains(q: &Quad, p: Point) -> bool {
    p.iter().all(|v| v.is_finite())
        && (0..4).all(|i| {
            cross(q[i], q[(i + 1) % 4], p) * cross(q[0], q[1], q[2]).signum()
                >= -1e-9
                    * (q[i][0] - q[(i + 1) % 4][0])
                        .hypot(q[i][1] - q[(i + 1) % 4][1])
                        .max(1.)
        })
}
fn compose(a: Homography, b: Homography) -> Homography {
    Homography(std::array::from_fn(|i| {
        std::array::from_fn(|j| (0..3).map(|k| a.0[i][k] * b.0[k][j]).sum())
    }))
}
fn unit_homography(q: &Quad) -> Result<Homography> {
    let scale = (0..4)
        .map(|i| (q[i][0] - q[(i + 1) % 4][0]).hypot(q[i][1] - q[(i + 1) % 4][1]))
        .fold(0., f64::max);
    let sign = cross(q[0], q[1], q[2]).signum();
    if q.iter().flatten().any(|v| !v.is_finite())
        || !scale.is_finite()
        || scale <= 0.
        || !(0..4)
            .all(|i| cross(q[i], q[(i + 1) % 4], q[(i + 2) % 4]) * sign > 1e-12 * scale * scale)
    {
        return Err(invalid(
            "quad must be finite, strictly convex and nonsingular",
        ));
    }
    let dx1 = q[1][0] - q[2][0];
    let dx2 = q[3][0] - q[2][0];
    let dx3 = q[0][0] - q[1][0] + q[2][0] - q[3][0];
    let dy1 = q[1][1] - q[2][1];
    let dy2 = q[3][1] - q[2][1];
    let dy3 = q[0][1] - q[1][1] + q[2][1] - q[3][1];
    let det = dx1 * dy2 - dx2 * dy1;
    let g = (dx3 * dy2 - dx2 * dy3) / det;
    let h = (dx1 * dy3 - dx3 * dy1) / det;
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
        .any(|d| !d.is_finite() || *d <= 1e-12)
        || m.0.iter().flatten().any(|v| !v.is_finite())
        || m.inverse().is_none()
    {
        return Err(invalid("quad crosses a projective horizon"));
    }
    Ok(m)
}
