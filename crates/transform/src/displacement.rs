//! Serializable absolute inverse lookup field, shared by CPU and GPU renderers.
use crate::{Error, Point, Result};
use serde::{Deserialize, Serialize};

/// Source coordinates at integer destination lattice vertices, including the
/// right/bottom canvas edge: `(width + 1) * (height + 1)` row-major entries.
/// Coordinates use pixel centers (first pixel at 0.5), NOT texel indices or
/// relative offsets. `None` represents a hole; cells touching holes are transparent.
/// Integer vertices make the field usable at arbitrary mip levels and crops.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Displacement {
    pub width: usize,
    pub height: usize,
    pub coordinates: Vec<Option<Point>>,
}
impl Displacement {
    /// Bound allocation and reject malformed/untrusted serialized fields.
    pub fn validate(&self) -> Result<()> {
        let n = self
            .width
            .checked_add(1)
            .and_then(|w| self.height.checked_add(1).and_then(|h| w.checked_mul(h)));
        if self.width == 0
            || self.height == 0
            || n.is_none_or(|n| n > 16_777_216 || n != self.coordinates.len())
            || self
                .coordinates
                .iter()
                .flatten()
                .flatten()
                .any(|x| !x.is_finite() || x.abs() > 1e12)
        {
            return Err(Error::Invalid("displacement requires finite bounded coordinates and a nonempty lattice of at most 16M vertices".into()));
        }
        Ok(())
    }
    /// Bilinear inverse map. Safe even on a malformed public payload; validate
    /// once before rendering to report errors rather than treating them as holes.
    pub fn inverse(&self, p: Point) -> Option<Point> {
        if self.width == 0
            || self.height == 0
            || !p.iter().all(|v| v.is_finite())
            || p[0] < 0.
            || p[1] < 0.
            || p[0] > self.width as f64
            || p[1] > self.height as f64
        {
            return None;
        }
        let x = (p[0].floor() as usize).min(self.width - 1);
        let y = (p[1].floor() as usize).min(self.height - 1);
        let stride = self.width.checked_add(1)?;
        let i = y.checked_mul(stride)?.checked_add(x)?;
        let u = p[0] - x as f64;
        let v = p[1] - y as f64;
        let weights = [(1. - u) * (1. - v), u * (1. - v), (1. - u) * v, u * v];
        let ids = [
            i,
            i.checked_add(1)?,
            i.checked_add(stride)?,
            i.checked_add(stride)?.checked_add(1)?,
        ];
        let mut out = [0.; 2];
        for (id, w) in ids.into_iter().zip(weights) {
            if w == 0. {
                continue;
            }
            let q = self.coordinates.get(id).copied().flatten()?;
            for a in 0..2 {
                out[a] += w * q[a];
            }
        }
        out.iter()
            .all(|x| x.is_finite() && x.abs() <= 1e12)
            .then_some(out)
    }
}
