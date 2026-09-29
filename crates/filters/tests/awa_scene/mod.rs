//! Shared B5-20b scene: an equidistant fisheye photo of two slightly tilted
//! straight stripes, with both edges traced as Vertical / Horizontal
//! constraints. Everything scales with the image size.
#![allow(dead_code)]
use compositor::{
    document::SmartFilter,
    geom::Rect,
    raster::{Depth, Raster},
    render::smart_filters::FilterContext,
};
use engine_api::tile::Extent;
use transform::adaptive::{Adaptive, CameraModel, LineConstraint, LineOrientation, Projection};

pub struct Scene {
    pub w: u32,
    pub h: u32,
    pub center: [f64; 2],
    /// Fisheye focal length in pixels (also the recipe's).
    pub focal: f64,
    /// Output = center + SCALE · world (output_focal_px = focal).
    pub scale: f64,
    /// Stripe width (Gaussian sigma) in world pixels.
    pub sigma: f64,
    /// Vertical stripe: world x = vx + vslope · y, traced over world y in vy.
    pub vx: f64,
    pub vslope: f64,
    pub vy: [f64; 2],
    /// Horizontal stripe: world y = hy + hslope · x, traced over world x in hx.
    pub hy: f64,
    pub hslope: f64,
    pub hx: [f64; 2],
}

impl Scene {
    pub fn new(w: u32, h: u32) -> Self {
        let (wf, hf) = (f64::from(w), f64::from(h));
        Self {
            w,
            h,
            center: [wf / 2., hf / 2.],
            focal: 0.4 * wf,
            scale: 0.9,
            sigma: 0.004 * wf,
            vx: -0.25 * wf,
            vslope: 0.008,
            vy: [-0.2 * hf, 0.35 * hf],
            hy: -0.3 * hf,
            hslope: -0.006,
            hx: [-0.1 * wf, 0.35 * wf],
        }
    }

    /// World (rectilinear, focal `focal`, centre-relative) -> fisheye source.
    pub fn observe(&self, p: [f64; 2]) -> [f64; 2] {
        let r = p[0].hypot(p[1]);
        let k = if r < 1e-12 {
            1.
        } else {
            self.focal * (r / self.focal).atan() / r
        };
        [self.center[0] + k * p[0], self.center[1] + k * p[1]]
    }

    /// Fisheye source -> world (centre-relative); `None` at/after the horizon.
    pub fn undistort(&self, s: [f64; 2]) -> Option<[f64; 2]> {
        let (x, y) = (s[0] - self.center[0], s[1] - self.center[1]);
        let r = x.hypot(y);
        let theta = r / self.focal;
        if theta >= std::f64::consts::FRAC_PI_2 - 1e-3 {
            return None;
        }
        let k = if r < 1e-12 {
            1.
        } else {
            self.focal * theta.tan() / r
        };
        Some([k * x, k * y])
    }

    /// Opaque grey image: white with two dark Gaussian stripes.
    pub fn raster(&self) -> Raster {
        let mut r = Raster::new(Extent::new(self.w, self.h), 4, Depth::F32, 0.0);
        r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
            let v = self.darkness([f64::from(x) + 0.5, f64::from(y) + 0.5]);
            let g = (1.0 - 0.8 * v) as f32;
            *p = [g, g, g, 1.0];
        })
        .unwrap();
        r
    }

    fn darkness(&self, s: [f64; 2]) -> f64 {
        let Some(p) = self.undistort(s) else {
            return 0.0;
        };
        let dv = p[0] - self.vx - self.vslope * p[1];
        let dh = p[1] - self.hy - self.hslope * p[0];
        let g = |d: f64| (-(d / self.sigma).powi(2)).exp();
        g(dv).max(g(dh))
    }

    pub fn recipe(&self) -> Adaptive {
        let mut a = Adaptive::new(
            self.w as usize,
            self.h as usize,
            CameraModel::Manual {
                focal_px: self.focal,
                center: self.center,
                projection: Projection::Equidistant,
            },
        );
        a.scale = self.scale;
        let trace = |f: &dyn Fn(f64) -> [f64; 2]| -> Vec<[f64; 2]> {
            (0..=16).map(|i| self.observe(f(f64::from(i) / 16.))).collect()
        };
        a.lines.push(LineConstraint {
            points: trace(&|t| {
                let y = self.vy[0] + (self.vy[1] - self.vy[0]) * t;
                [self.vx + self.vslope * y, y]
            }),
            orientation: LineOrientation::Vertical,
            weight: 1.,
        });
        a.lines.push(LineConstraint {
            points: trace(&|t| {
                let x = self.hx[0] + (self.hx[1] - self.hx[0]) * t;
                [x, self.hy + self.hslope * x]
            }),
            orientation: LineOrientation::Horizontal,
            weight: 1.,
        });
        a
    }

    /// Spread (max − min, output px) of the vertical stripe's x-centroid over
    /// output rows inside its traced range, and of the horizontal stripe's
    /// y-centroid over output columns inside its range.
    pub fn straightness(&self, out: &Raster) -> (f64, f64) {
        let s = self.scale;
        let half = (6.0 * self.sigma * s).ceil() as i64;
        let (cx, cy) = (self.center[0], self.center[1]);
        let centroid = |along: &dyn Fn(i64) -> [u32; 2], mid: f64| -> f64 {
            let (mut sw, mut sx) = (0.0, 0.0);
            for k in -half..=half {
                let i = mid.floor() as i64 + k;
                let [x, y] = along(i);
                let p = out.pixel(x, y);
                let v = f64::from(1.0 - p[0]).max(0.0) * f64::from(p[3]);
                sw += v;
                sx += v * (i as f64 + 0.5);
            }
            assert!(sw > 1.0, "stripe not found");
            sx / sw
        };
        let spread = |v: Vec<f64>| {
            v.iter().copied().fold(f64::MIN, f64::max) - v.iter().copied().fold(f64::MAX, f64::min)
        };
        let n = 24;
        let margin = 0.05;
        let rows: Vec<f64> = (0..=n)
            .map(|i| {
                let t = margin + (1. - 2. * margin) * f64::from(i) / f64::from(n);
                let yw = self.vy[0] + (self.vy[1] - self.vy[0]) * t;
                let y = (cy + s * yw) as u32;
                centroid(&|x| [x as u32, y], cx + s * self.vx)
            })
            .collect();
        let cols: Vec<f64> = (0..=n)
            .map(|i| {
                let t = margin + (1. - 2. * margin) * f64::from(i) / f64::from(n);
                let xw = self.hx[0] + (self.hx[1] - self.hx[0]) * t;
                let x = (cx + s * xw) as u32;
                centroid(&|y| [x, y as u32], cy + s * self.hy)
            })
            .collect();
        (spread(rows), spread(cols))
    }
}

pub fn node(a: &Adaptive) -> SmartFilter {
    SmartFilter {
        name: "adaptive_wide_angle".into(),
        enabled: true,
        params: serde_json::to_value(a).unwrap(),
        ..Default::default()
    }
}

pub fn context(w: u32, h: u32) -> FilterContext {
    FilterContext {
        profile: None,
        level: 0,
        canvas: Extent::new(w, h),
    }
}

/// Every `step`-th row of `r` as straight RGBA, for byte comparisons.
pub fn rows(r: &Raster, step: u32) -> Vec<[f32; 4]> {
    let e = r.extent();
    (0..e.height)
        .step_by(step as usize)
        .flat_map(|y| (0..e.width).map(move |x| (x, y)))
        .map(|(x, y)| r.pixel(x, y))
        .collect()
}
