//! Forward source-to-destination homographies in level-zero pixel coordinates.
use crate::{Error, Point, Result};
use lens::Homography;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FreeTransform {
    pub matrix: [[f64; 3]; 3],
}
impl FreeTransform {
    pub const fn identity() -> Self {
        Self {
            matrix: Homography::IDENTITY.0,
        }
    }
    pub fn translate(x: f64, y: f64) -> Result<Self> {
        Self::checked([[1., 0., x], [0., 1., y], [0., 0., 1.]])
    }
    pub fn scale(x: f64, y: f64, reference: Point) -> Result<Self> {
        Self::around([[x, 0.], [0., y]], reference)
    }
    /// Positive radians rotate clockwise in a conventional y-down image.
    pub fn rotate(radians: f64, reference: Point) -> Result<Self> {
        let (s, c) = radians.sin_cos();
        Self::around([[c, -s], [s, c]], reference)
    }
    /// Horizontal and vertical shear angles in radians, applied simultaneously.
    pub fn skew(x_radians: f64, y_radians: f64, reference: Point) -> Result<Self> {
        if x_radians.cos().abs() < 1e-12 || y_radians.cos().abs() < 1e-12 {
            return Err(Error::Invalid("singular skew angle".into()));
        }
        Self::around([[1., x_radians.tan()], [y_radians.tan(), 1.]], reference)
    }
    pub fn flip(horizontal: bool, vertical: bool, reference: Point) -> Result<Self> {
        Self::scale(
            if horizontal { -1. } else { 1. },
            if vertical { -1. } else { 1. },
            reference,
        )
    }
    fn around(a: [[f64; 2]; 2], p: Point) -> Result<Self> {
        Self::checked([
            [a[0][0], a[0][1], p[0] - a[0][0] * p[0] - a[0][1] * p[1]],
            [a[1][0], a[1][1], p[1] - a[1][0] * p[0] - a[1][1] * p[1]],
            [0., 0., 1.],
        ])
    }
    fn checked(matrix: [[f64; 3]; 3]) -> Result<Self> {
        let t = Self { matrix };
        t.validate()?;
        Ok(t)
    }

    pub fn validate(&self) -> Result<()> {
        self.inverse().map(|_| ())
    }
    pub fn map(&self, point: Point) -> Option<Point> {
        Homography(self.matrix).map(point)
    }
    pub fn inverse(&self) -> Result<Self> {
        if self.matrix.iter().flatten().any(|v| !v.is_finite()) {
            return Err(Error::Invalid("nonfinite homography".into()));
        }
        let h = Homography(self.matrix)
            .inverse()
            .filter(|h| h.0.iter().flatten().all(|v| v.is_finite()))
            .ok_or_else(|| Error::Invalid("singular homography".into()))?;
        Ok(Self { matrix: h.0 })
    }
    /// Exact axis-aligned [minimum, maximum] of the transformed source rectangle.
    /// Rejects a projective pole anywhere in the closed rectangle, not just at corners.
    pub fn bounds(&self, width: f64, height: f64) -> Result<[Point; 2]> {
        self.validate()?;
        if !width.is_finite() || !height.is_finite() || width <= 0. || height <= 0. {
            return Err(Error::Invalid("invalid source rectangle".into()));
        }
        let corners = [[0., 0.], [width, 0.], [width, height], [0., height]];
        let denominators = corners
            .map(|p| self.matrix[2][0] * p[0] + self.matrix[2][1] * p[1] + self.matrix[2][2]);
        if !(denominators.iter().all(|d| *d >= 1e-12) || denominators.iter().all(|d| *d <= -1e-12))
        {
            return Err(Error::Invalid(
                "projective pole intersects source rectangle".into(),
            ));
        }
        let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
        for corner in corners {
            let p = self
                .map(corner)
                .ok_or_else(|| Error::Invalid("unbounded homography".into()))?;
            for (axis, value) in p.into_iter().enumerate() {
                bounds[0][axis] = bounds[0][axis].min(value);
                bounds[1][axis] = bounds[1][axis].max(value);
            }
        }
        Ok(bounds)
    }
}
