//! Pipeline-independent lens calibration in centered, axis-normalized coordinates.
pub mod opcodes;
pub mod upright;
pub use upright::*;
pub mod vignette;
pub use vignette::*;
pub mod ca;
pub use ca::*;
pub mod image;
pub use image::*;
pub mod calibration;
pub use calibration::*;
pub mod profiles;
pub use profiles::*;
pub mod data_pack;
pub use data_pack::*;
use serde::{Deserialize, Serialize};
pub type Point = [f64; 2];
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid optics data: {0}")]
    Invalid(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}
pub type Result<T> = std::result::Result<T, Error>;
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct BrownConrady {
    pub k1: f64,
    pub k2: f64,
    pub k3: f64,
    pub p1: f64,
    pub p2: f64,
    pub cx: f64,
    pub cy: f64,
}
impl BrownConrady {
    /// Forward ideal-to-observed mapping, not clamped to the image bounds.
    pub fn distort(self, p: Point) -> Point {
        let x = p[0] - self.cx;
        let y = p[1] - self.cy;
        let r = x * x + y * y;
        let s = 1.0 + r * (self.k1 + r * (self.k2 + r * self.k3));
        [
            self.cx + x * s + 2.0 * self.p1 * x * y + self.p2 * (r + 2.0 * x * x),
            self.cy + y * s + self.p1 * (r + 2.0 * y * y) + 2.0 * self.p2 * x * y,
        ]
    }
    /// Newton inverse. Non-convergence/singular mappings return None.
    pub fn undistort(self, p: Point) -> Option<Point> {
        let mut q = p;
        for _ in 0..40 {
            let f = self.distort(q);
            let e = [f[0] - p[0], f[1] - p[1]];
            if e[0].hypot(e[1]) < 1e-12 {
                return Some(q);
            }
            let h = 1e-6;
            let a = self.distort([q[0] + h, q[1]]);
            let b = self.distort([q[0], q[1] + h]);
            let j = [
                (a[0] - f[0]) / h,
                (b[0] - f[0]) / h,
                (a[1] - f[1]) / h,
                (b[1] - f[1]) / h,
            ];
            let d = j[0] * j[3] - j[1] * j[2];
            if !d.is_finite() || d.abs() < 1e-14 {
                return None;
            }
            q[0] -= (j[3] * e[0] - j[1] * e[1]) / d;
            q[1] -= (-j[2] * e[0] + j[0] * e[1]) / d;
        }
        None
    }
}
