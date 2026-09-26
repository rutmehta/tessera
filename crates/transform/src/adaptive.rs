//! Adaptive wide angle: camera -> unit sphere -> rectilinear -> constrained mesh.
//!
//! # Geometry and objective
//! Equidistant fisheye uses `theta = radius / focal_px`; rectilinear uses
//! `theta = atan(radius / focal_px)`. Both lift to the unit sphere and reproject
//! as `output_focal_px * scale * crop_factor * ray.xy / ray.z + source_size/2 - crop`.
//! Profiles first invert the complete native lens calibration in axis-normalized
//! coordinates; resolved samples are embedded in the recipe for reproducibility.
//!
//! The fitted map is `F(p) = p + B(p)d`, with bilinear control weights `B`.
//! For each traced line, its projected centroid is fixed and its normal is the
//! TLS normal (Straight), `[0,1]` (Horizontal), or `[1,0]` (Vertical). We minimize
//! `1000 sum(weight * (normal dot (F(p)-centroid))^2)` plus `smoothness` times
//! squared first differences, `4*smoothness` times squared second differences,
//! and `regularization * sum(|d|^2)`. The positive anchor removes the nullspace.
//! Sparse Jacobi-preconditioned conjugate gradients have deterministic reduction
//! order and verify the true normal-equation residual before accepting a solve.
//!
//! # Limits and failure semantics
//! - Output lattice: at most 16,777,216 vertices; source axes at most 1,000,000.
//! - Control vertices: 3..=65 per axis, default 17; padded domain is
//!   `[-width/2, 3*width/2] x [-height/2, 3*height/2]` in projected output space.
//! - At most 256 lines / 16,384 input samples / 65,536 densified samples.
//!   Source segments are densified every four pixels, capped at 64 subdivisions;
//!   trace large or highly curved edges with additional input samples.
//! - Default maximum fitted normal residual is 0.25 output pixels. This is a
//!   sample-wise mesh bound, not a universal bound on interpolated image edges.
//! - Every cell corner must have Jacobian symmetric part greater than `0.05 I`.
//!   This conservative global-injectivity criterion rejects strong rotations,
//!   collapses, and folds rather than returning ambiguous inverse geometry.
//! - The camera horizon/rear hemisphere is not representable. Constraints there,
//!   invalid samples, conflicting lines, insufficient resolution, and failed CG
//!   convergence return descriptive errors. Newton inverses are damped and bounded.
//! - Source-exterior / noninvertible profile regions are transparent holes; an
//!   entirely uncovered output is an error. Crop chooses a fixed canvas; it does
//!   not perform automatic maximal-inscribed-rectangle cropping.
//! - The emitted displacement field is a one-pixel lattice sampled bilinearly,
//!   in the absolute pixel-center convention used by the shared CPU/GPU path.
use crate::{Error, Point, Result, displacement::Displacement};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Projection {
    Rectilinear,
    Equidistant,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum CameraModel {
    /// Resolved rectilinear profile sample, embedded for replay independent of
    /// database updates. Native odd radial terms and axis scales are preserved.
    Profile {
        focal_px: f64,
        center: Point,
        calibration: lens::CalibrationSample,
    },
    /// Focal length is in pixels, center in source pixel-center coordinates.
    Manual {
        focal_px: f64,
        center: Point,
        projection: Projection,
    },
}
impl CameraModel {
    /// Resolve focal/aperture/focus interpolation and convert mm to pixels using
    /// the active sensor width (caller must account for sensor/camera crop).
    /// Existing lens profiles are rectilinear; fisheye uses Manual explicitly.
    pub fn from_profile(
        profile: &lens::Profile,
        focal_mm: f64,
        aperture: f64,
        distance: f64,
        sensor_width_mm: f64,
        image_size: [usize; 2],
    ) -> Result<Self> {
        if !sensor_width_mm.is_finite() || sensor_width_mm <= 0. || image_size.contains(&0) {
            return Err(invalid("invalid sensor width or profile image size"));
        }
        let calibration = profile
            .sample(focal_mm, aperture, distance)
            .ok_or_else(|| invalid("invalid or empty lens profile"))?;
        let camera = Self::Profile {
            focal_px: focal_mm / sensor_width_mm * image_size[0] as f64,
            center: [image_size[0] as f64 / 2., image_size[1] as f64 / 2.],
            calibration,
        };
        camera.validate()?;
        Ok(camera)
    }
    fn focal(&self) -> f64 {
        match self {
            Self::Manual { focal_px, .. } | Self::Profile { focal_px, .. } => *focal_px,
        }
    }
    fn center(&self) -> Point {
        match self {
            Self::Manual { center, .. } | Self::Profile { center, .. } => *center,
        }
    }
    fn projection(&self) -> Projection {
        match self {
            Self::Manual { projection, .. } => *projection,
            Self::Profile { .. } => Projection::Rectilinear,
        }
    }
    fn validate(&self) -> Result<()> {
        if let Self::Profile { calibration, .. } = self {
            lens::Profile {
                model: "embedded".into(),
                samples: vec![calibration.clone()],
                ..Default::default()
            }
            .validate()
            .map_err(|_| invalid("invalid embedded lens calibration"))?;
        }
        if !self.focal().is_finite()
            || !(1e-3..=1e9).contains(&self.focal())
            || !self
                .center()
                .iter()
                .all(|x| x.is_finite() && x.abs() <= 1e9)
        {
            return Err(invalid("invalid camera focal length or optical center"));
        }
        Ok(())
    }
    fn ray(&self, p: Point, size: Point) -> Option<[f64; 3]> {
        let p = if let Self::Profile { calibration, .. } = self {
            let n = [2. * p[0] / size[0] - 1., 2. * p[1] / size[1] - 1.];
            let q = profile_inverse(calibration, n)?;
            [(q[0] + 1.) * size[0] / 2., (q[1] + 1.) * size[1] / 2.]
        } else {
            p
        };
        let c = self.center();
        let x = (p[0] - c[0]) / self.focal();
        let y = (p[1] - c[1]) / self.focal();
        let r = x.hypot(y);
        let theta = match self.projection() {
            Projection::Rectilinear => r.atan(),
            Projection::Equidistant => r,
        };
        // Rectilinear reprojection cannot represent the horizon or rear hemisphere.
        if !theta.is_finite() || theta >= std::f64::consts::FRAC_PI_2 - 1e-5 {
            return None;
        }
        let s = if r < 1e-14 { 1. } else { theta.sin() / r };
        Some([s * x, s * y, theta.cos()])
    }
    fn observe(&self, ray: [f64; 3], size: Point) -> Option<Point> {
        if ray[2] <= 0. {
            return None;
        }
        let r = ray[0].hypot(ray[1]);
        let factor = match self.projection() {
            Projection::Rectilinear => self.focal() / ray[2],
            Projection::Equidistant => {
                if r < 1e-14 {
                    self.focal() / ray[2]
                } else {
                    self.focal() * r.atan2(ray[2]) / r
                }
            }
        };
        let c = self.center();
        let p = [c[0] + factor * ray[0], c[1] + factor * ray[1]];
        if let Self::Profile { calibration, .. } = self {
            let n = [2. * p[0] / size[0] - 1., 2. * p[1] / size[1] - 1.];
            let q = calibration.distort(n);
            // Reject noninvertible/out-of-domain extrapolated profile regions.
            let recovered = profile_inverse(calibration, q)?;
            if (n[0] - recovered[0]).hypot(n[1] - recovered[1]) > 1e-7 {
                return None;
            }
            Some([(q[0] + 1.) * size[0] / 2., (q[1] + 1.) * size[1] / 2.])
        } else {
            Some(p)
        }
    }
}
/// Desired orientation of a traced scene edge after correction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineOrientation {
    Straight,
    Horizontal,
    Vertical,
}
/// Observed source-pixel polyline. Use >=3 samples to describe a curved edge;
/// two endpoints describe a source-image chord, automatically densified.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LineConstraint {
    pub points: Vec<Point>,
    pub orientation: LineOrientation,
    pub weight: f64,
}
/// A serializable recipe. Pixel geometry is always level zero.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Adaptive {
    pub source_width: usize,
    pub source_height: usize,
    pub output_width: usize,
    pub output_height: usize,
    pub camera: CameraModel,
    pub output_focal_px: f64,
    pub scale: f64,
    /// Additional output field-of-view crop multiplier. Use 1 when already
    /// accounted for in output_focal_px or the profile's active sensor width.
    pub crop_factor: f64,
    /// Top-left crop offset relative to a source-sized centered rectilinear view.
    pub crop: Point,
    pub lines: Vec<LineConstraint>,
    /// Number of control vertices per axis, each 3..=65.
    pub mesh_size: [usize; 2],
    /// Membrane/bending penalty and positive identity anchor respectively.
    pub smoothness: f64,
    pub regularization: f64,
    /// Maximum permitted normal line error in output pixels; conflicting or
    /// under-resolved constraints fail instead of silently returning a bad warp.
    pub line_tolerance: f64,
    pub max_iterations: usize,
}
impl Adaptive {
    pub fn new(width: usize, height: usize, camera: CameraModel) -> Self {
        Self {
            source_width: width,
            source_height: height,
            output_width: width,
            output_height: height,
            output_focal_px: camera.focal(),
            camera,
            scale: 1.,
            crop_factor: 1.,
            crop: [0.; 2],
            lines: Vec::new(),
            mesh_size: [17, 17],
            smoothness: 0.1,
            regularization: 1e-4,
            line_tolerance: 0.25,
            max_iterations: 2000,
        }
    }
    pub fn validate(&self) -> Result<()> {
        self.camera.validate()?;
        if !self.mesh_size.iter().all(|n| (3..=65).contains(n))
            || !self.smoothness.is_finite()
            || !(1e-6..=1e6).contains(&self.smoothness)
            || !self.regularization.is_finite()
            || !(1e-8..=1e3).contains(&self.regularization)
            || !self.line_tolerance.is_finite()
            || !(1e-3..=10.).contains(&self.line_tolerance)
            || !(1..=10000).contains(&self.max_iterations)
            || self.lines.len() > 256
            || self.lines.iter().map(|l| l.points.len()).sum::<usize>() > 16384
        {
            return Err(invalid(
                "invalid adaptive mesh, regularization, iteration or constraint limits",
            ));
        }
        for line in &self.lines {
            if line.points.len() < 2
                || !line.weight.is_finite()
                || !(1e-4..=1e4).contains(&line.weight)
                || line.points.iter().any(|p| {
                    !p.iter().all(|x| x.is_finite())
                        || p[0] < 0.
                        || p[1] < 0.
                        || p[0] > self.source_width as f64
                        || p[1] > self.source_height as f64
                })
                || line
                    .points
                    .windows(2)
                    .any(|p| (p[0][0] - p[1][0]).hypot(p[0][1] - p[1][1]) < 1e-6)
            {
                return Err(invalid(
                    "degenerate line: distinct finite in-bounds samples and positive weight required",
                ));
            }
        }
        if self.source_width == 0
            || self.source_height == 0
            || self.source_width > 1_000_000
            || self.source_height > 1_000_000
            || self.output_width == 0
            || self.output_height == 0
            || self
                .output_width
                .checked_add(1)
                .and_then(|w| {
                    self.output_height
                        .checked_add(1)
                        .and_then(|h| w.checked_mul(h))
                })
                .is_none_or(|n| n > 16_777_216)
            || !self.output_focal_px.is_finite()
            || !(1e-3..=1e9).contains(&self.output_focal_px)
            || !self.scale.is_finite()
            || !(1e-4..=1e4).contains(&self.scale)
            || !self.crop_factor.is_finite()
            || !(0.1..=10.).contains(&self.crop_factor)
            || !self.crop.iter().all(|x| x.is_finite() && x.abs() < 1e9)
        {
            return Err(invalid(
                "invalid adaptive canvas, focal length, scale, or crop",
            ));
        }
        Ok(())
    }
    /// Camera-only forward reprojection, before the fitted constraint warp.
    pub fn project(&self, source: Point) -> Option<Point> {
        let ray = self.camera.ray(
            source,
            [self.source_width as f64, self.source_height as f64],
        )?;
        let f = self.output_focal_px * self.scale * self.crop_factor;
        let q = [
            f * ray[0] / ray[2] + self.source_width as f64 / 2. - self.crop[0],
            f * ray[1] / ray[2] + self.source_height as f64 / 2. - self.crop[1],
        ];
        q.iter()
            .all(|x| x.is_finite() && x.abs() < 1e12)
            .then_some(q)
    }
    fn unproject(&self, q: Point) -> Option<Point> {
        let f = self.output_focal_px * self.scale * self.crop_factor;
        let p = self.camera.observe(
            [
                (q[0] + self.crop[0] - self.source_width as f64 / 2.) / f,
                (q[1] + self.crop[1] - self.source_height as f64 / 2.) / f,
                1.,
            ],
            [self.source_width as f64, self.source_height as f64],
        )?;
        (p.iter().all(|x| x.is_finite())
            && p[0] >= 0.
            && p[1] >= 0.
            && p[0] <= self.source_width as f64
            && p[1] <= self.source_height as f64)
            .then_some(p)
    }
    pub fn solve(&self) -> Result<Displacement> {
        self.validate()?;
        let mesh = Mesh::fit(self)?;
        let mut coordinates =
            Vec::with_capacity((self.output_width + 1) * (self.output_height + 1));
        for y in 0..=self.output_height {
            for x in 0..=self.output_width {
                let q = [x as f64, y as f64];
                coordinates.push(mesh.inverse(q).and_then(|p| self.unproject(p)));
            }
        }
        let out = Displacement {
            width: self.output_width,
            height: self.output_height,
            coordinates,
        };
        out.validate()?;
        let stride = out.width + 1;
        let covered = (0..out.height).any(|y| {
            (0..out.width).any(|x| {
                let i = y * stride + x;
                [i, i + 1, i + stride, i + stride + 1]
                    .iter()
                    .all(|i| out.coordinates[*i].is_some())
            })
        });
        if !covered {
            return Err(invalid(
                "adaptive output has no source coverage; check camera, crop and scale",
            ));
        }
        Ok(out)
    }
}

/// Bilinear displacement over a padded rectilinear domain. The identity anchor
/// fixes the nullspace; first and second differences suppress kinks and drift.
struct Mesh {
    size: [usize; 2],
    extent: Point,
    values: Vec<Point>,
}
struct Row {
    terms: Vec<(usize, f64)>,
    rhs: f64,
}
impl Row {
    fn new(terms: Vec<(usize, f64)>, rhs: f64, weight: f64) -> Self {
        let s = weight.sqrt();
        Self {
            terms: terms.into_iter().map(|(i, v)| (i, v * s)).collect(),
            rhs: rhs * s,
        }
    }
    fn dot(&self, x: &[f64]) -> f64 {
        self.terms.iter().map(|(i, v)| v * x[*i]).sum()
    }
}
impl Mesh {
    fn weights(&self, p: Point) -> Option<[(usize, f64); 4]> {
        let uv: Point = std::array::from_fn(|a| {
            (p[a] / self.extent[a] + 0.5) * 0.5 * (self.size[a] - 1) as f64
        });
        if uv
            .iter()
            .enumerate()
            .any(|(a, v)| !v.is_finite() || *v < 0. || *v > (self.size[a] - 1) as f64)
        {
            return None;
        }
        let x = (uv[0].floor() as usize).min(self.size[0] - 2);
        let y = (uv[1].floor() as usize).min(self.size[1] - 2);
        let u = uv[0] - x as f64;
        let v = uv[1] - y as f64;
        let i = y * self.size[0] + x;
        Some([
            (i, (1. - u) * (1. - v)),
            (i + 1, u * (1. - v)),
            (i + self.size[0], (1. - u) * v),
            (i + self.size[0] + 1, u * v),
        ])
    }
    fn map(&self, p: Point) -> Option<Point> {
        let mut q = p;
        for (i, w) in self.weights(p)? {
            for (a, v) in q.iter_mut().enumerate() {
                *v += w * self.values[i][a];
            }
        }
        Some(q)
    }
    // Exact bilinear Jacobian. Evaluating every cell corner bounds its symmetric
    // part everywhere, proving strict monotonicity/global injectivity.
    fn jacobian(&self, x: usize, y: usize, u: f64, v: f64) -> [f64; 4] {
        let i = y * self.size[0] + x;
        let [a, b, c, d] = [
            self.values[i],
            self.values[i + 1],
            self.values[i + self.size[0]],
            self.values[i + self.size[0] + 1],
        ];
        let sx = (self.size[0] - 1) as f64 / (2. * self.extent[0]);
        let sy = (self.size[1] - 1) as f64 / (2. * self.extent[1]);
        [
            1. + sx * ((b[0] - a[0]) * (1. - v) + (d[0] - c[0]) * v),
            sy * ((c[0] - a[0]) * (1. - u) + (d[0] - b[0]) * u),
            sx * ((b[1] - a[1]) * (1. - v) + (d[1] - c[1]) * v),
            1. + sy * ((c[1] - a[1]) * (1. - u) + (d[1] - b[1]) * u),
        ]
    }
    fn derivative(&self, p: Point) -> Option<[f64; 4]> {
        self.weights(p)?;
        let u = (p[0] / self.extent[0] + 0.5) * 0.5 * (self.size[0] - 1) as f64;
        let v = (p[1] / self.extent[1] + 0.5) * 0.5 * (self.size[1] - 1) as f64;
        let x = (u.floor() as usize).min(self.size[0] - 2);
        let y = (v.floor() as usize).min(self.size[1] - 2);
        Some(self.jacobian(x, y, u - x as f64, v - y as f64))
    }
    fn inverse(&self, target: Point) -> Option<Point> {
        let mut q = target;
        for _ in 0..40 {
            let f = self.map(q)?;
            let e = [f[0] - target[0], f[1] - target[1]];
            let norm = e[0].hypot(e[1]);
            if norm < 1e-7 {
                return Some(q);
            }
            let j = self.derivative(q)?;
            let det = j[0] * j[3] - j[1] * j[2];
            if !det.is_finite() || det <= 1e-10 {
                return None;
            }
            let step = [
                (j[3] * e[0] - j[1] * e[1]) / det,
                (-j[2] * e[0] + j[0] * e[1]) / det,
            ];
            let mut accepted = false;
            for k in 0..20 {
                let s = 0.5f64.powi(k);
                let t = [q[0] - s * step[0], q[1] - s * step[1]];
                if self
                    .map(t)
                    .is_some_and(|p| (p[0] - target[0]).hypot(p[1] - target[1]) < norm)
                {
                    q = t;
                    accepted = true;
                    break;
                }
            }
            if !accepted {
                return None;
            }
        }
        None
    }
    fn fit(a: &Adaptive) -> Result<Self> {
        let mut mesh = Self {
            size: a.mesh_size,
            extent: [a.output_width as f64, a.output_height as f64],
            values: vec![[0.; 2]; a.mesh_size[0] * a.mesh_size[1]],
        };
        if a.lines.is_empty() {
            return Ok(mesh);
        }
        let mut rows = Vec::new();
        let mut checks = Vec::new();
        for line in &a.lines {
            let mut points = Vec::new();
            // Densify each traced source edge, not its projected chord.
            for pair in line.points.windows(2) {
                let length = (pair[1][0] - pair[0][0]).hypot(pair[1][1] - pair[0][1]);
                let steps = (length / 4.).ceil().clamp(1., 64.) as usize;
                for i in 0..steps {
                    let t = i as f64 / steps as f64;
                    let p = [
                        pair[0][0] * (1. - t) + pair[1][0] * t,
                        pair[0][1] * (1. - t) + pair[1][1] * t,
                    ];
                    points.push(a.project(p).ok_or_else(|| {
                        invalid("constraint reaches camera horizon or invalid profile inverse")
                    })?);
                }
            }
            points.push(
                a.project(*line.points.last().unwrap())
                    .ok_or_else(|| invalid("constraint reaches camera horizon"))?,
            );
            if points.len() > 32768 || checks.len() + points.len() > 65536 {
                return Err(invalid("too many densified constraint samples"));
            }
            let center: Point = std::array::from_fn(|axis| {
                points.iter().map(|p| p[axis]).sum::<f64>() / points.len() as f64
            });
            let mut cov = [0.; 3];
            for p in &points {
                let x = p[0] - center[0];
                let y = p[1] - center[1];
                cov[0] += x * x;
                cov[1] += x * y;
                cov[2] += y * y;
            }
            if cov[0] + cov[2] < 1e-6 {
                return Err(invalid("degenerate projected constraint"));
            }
            let angle = 0.5 * (2. * cov[1]).atan2(cov[0] - cov[2]);
            let normal = match line.orientation {
                LineOrientation::Straight => [-angle.sin(), angle.cos()],
                LineOrientation::Horizontal => [0., 1.],
                LineOrientation::Vertical => [1., 0.],
            };
            for p in points {
                let w = mesh.weights(p).ok_or_else(|| {
                    invalid("constraint outside padded output mesh; adjust crop, scale or canvas")
                })?;
                let rhs = normal[0] * (center[0] - p[0]) + normal[1] * (center[1] - p[1]);
                let terms = w
                    .into_iter()
                    .flat_map(|(i, w)| [(2 * i, w * normal[0]), (2 * i + 1, w * normal[1])])
                    .collect();
                rows.push(Row::new(terms, rhs, 1000. * line.weight));
                checks.push((p, normal, center));
            }
        }
        let [nx, ny] = mesh.size;
        for y in 0..ny {
            for x in 0..nx {
                for axis in 0..2 {
                    let i = 2 * (y * nx + x) + axis;
                    rows.push(Row::new(vec![(i, 1.)], 0., a.regularization));
                    for (has_next, has_both, step) in [
                        (x + 1 < nx, x > 0 && x + 1 < nx, 2),
                        (y + 1 < ny, y > 0 && y + 1 < ny, 2 * nx),
                    ] {
                        if has_next {
                            rows.push(Row::new(vec![(i, 1.), (i + step, -1.)], 0., a.smoothness));
                        }
                        if has_both {
                            rows.push(Row::new(
                                vec![(i - step, 1.), (i, -2.), (i + step, 1.)],
                                0.,
                                4. * a.smoothness,
                            ));
                        }
                    }
                }
            }
        }
        let solution = least_squares(&rows, 2 * nx * ny, a.max_iterations)?;
        for (p, v) in mesh.values.iter_mut().zip(solution.as_chunks::<2>().0) {
            *p = [v[0], v[1]];
        }
        for y in 0..ny - 1 {
            for x in 0..nx - 1 {
                for (u, v) in [(0., 0.), (1., 0.), (0., 1.), (1., 1.)] {
                    let j = mesh.jacobian(x, y, u, v);
                    let off = (j[1] + j[2]) * 0.5;
                    if j.iter().any(|v| !v.is_finite())
                        || j[0] <= 0.05
                        || j[3] <= 0.05
                        || (j[0] - 0.05) * (j[3] - 0.05) - off * off <= 0.
                    {
                        return Err(invalid(
                            "constraints produce a folded or non-monotone mesh; reduce correction",
                        ));
                    }
                }
            }
        }
        for (p, n, c) in checks {
            let q = mesh.map(p).ok_or_else(|| invalid("constraint left mesh"))?;
            if (n[0] * (q[0] - c[0]) + n[1] * (q[1] - c[1])).abs() > a.line_tolerance {
                return Err(invalid(
                    "constraint residual exceeds tolerance; conflicting lines or insufficient mesh resolution",
                ));
            }
        }
        Ok(mesh)
    }
}
/// Deterministic Jacobi-preconditioned CG on AᵀA, accumulated in row order.
/// No dense normal matrix, random choices, or parallel floating-point reductions.
fn least_squares(rows: &[Row], n: usize, max_iterations: usize) -> Result<Vec<f64>> {
    let mut b = vec![0.; n];
    let mut diag = vec![0.; n];
    for row in rows {
        for &(i, v) in &row.terms {
            b[i] += v * row.rhs;
            diag[i] += v * v;
        }
    }
    let apply = |x: &[f64]| {
        let mut y = vec![0.; n];
        for row in rows {
            let t = row.dot(x);
            for &(i, v) in &row.terms {
                y[i] += v * t;
            }
        }
        y
    };
    let dot = |a: &[f64], b: &[f64]| a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>();
    let mut x = vec![0.; n];
    let mut r = b.clone();
    let tolerance = 1e-9 * dot(&b, &b).sqrt().max(1.);
    if dot(&r, &r).sqrt() <= tolerance {
        return Ok(x);
    }
    let mut z: Vec<_> = r.iter().zip(&diag).map(|(r, d)| r / d).collect();
    let mut p = z.clone();
    let mut rz = dot(&r, &z);
    for _ in 0..max_iterations {
        let ap = apply(&p);
        let denom = dot(&p, &ap);
        if !denom.is_finite() || denom <= 0. {
            return Err(invalid("singular adaptive least-squares system"));
        }
        let alpha = rz / denom;
        for i in 0..n {
            x[i] += alpha * p[i];
            r[i] -= alpha * ap[i];
        }
        if dot(&r, &r).sqrt() <= tolerance {
            // Verify the true residual, not merely the accumulated CG recurrence.
            let ax = apply(&x);
            if ax
                .iter()
                .zip(&b)
                .map(|(a, b)| (a - b).powi(2))
                .sum::<f64>()
                .sqrt()
                <= tolerance * 4.
                && x.iter().all(|v| v.is_finite())
            {
                return Ok(x);
            }
        }
        for i in 0..n {
            z[i] = r[i] / diag[i];
        }
        let next = dot(&r, &z);
        let beta = next / rz;
        for i in 0..n {
            p[i] = z[i] + beta * p[i];
        }
        rz = next;
    }
    Err(invalid(
        "adaptive least-squares solver did not converge within iteration limit",
    ))
}

/// Bounded damped Newton inversion for the complete native profile mapping.
fn profile_inverse(s: &lens::CalibrationSample, target: Point) -> Option<Point> {
    if target.iter().any(|v| !v.is_finite() || v.abs() > 1e4) {
        return None;
    }
    let mut q = target;
    for _ in 0..50 {
        let f = s.distort(q);
        let e = [f[0] - target[0], f[1] - target[1]];
        let norm = e[0].hypot(e[1]);
        let h = 1e-6;
        let x = s.distort([q[0] + h, q[1]]);
        let y = s.distort([q[0], q[1] + h]);
        let j = [
            (x[0] - f[0]) / h,
            (y[0] - f[0]) / h,
            (x[1] - f[1]) / h,
            (y[1] - f[1]) / h,
        ];
        let det = j[0] * j[3] - j[1] * j[2];
        // A positive determinant alone permits 180-degree reversal; require
        // positive symmetric part as well for a physically useful local inverse.
        let off = (j[1] + j[2]) * 0.5;
        if !det.is_finite() || j[0] <= 1e-6 || j[3] <= 1e-6 || j[0] * j[3] - off * off <= 1e-10 {
            return None;
        }
        if norm < 1e-11 {
            return Some(q);
        }
        let step = [
            (j[3] * e[0] - j[1] * e[1]) / det,
            (-j[2] * e[0] + j[0] * e[1]) / det,
        ];
        let mut accepted = false;
        for k in 0..20 {
            let a = 0.5f64.powi(k);
            let p = [q[0] - a * step[0], q[1] - a * step[1]];
            if p.iter().any(|v| !v.is_finite() || v.abs() > 1e4) {
                continue;
            }
            let g = s.distort(p);
            if (g[0] - target[0]).hypot(g[1] - target[1]) < norm {
                q = p;
                accepted = true;
                break;
            }
        }
        if !accepted {
            return None;
        }
    }
    None
}

fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
