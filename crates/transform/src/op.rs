//! Serializable transform recipe and shared inverse-mapped CPU renderer.
use crate::{
    Error, Image, Point, Result, check_cancel,
    displacement::Displacement,
    free::FreeTransform,
    perspective::{PerspectiveWarp, PreparedPerspectiveWarp},
    puppet::{PreparedPuppetWarp, PuppetWarp},
    sample::{Kernel, sample},
    seam::{self, ContentAwareScale},
    warp::{WarpInverseField, WarpMesh},
};
use engine_api::jobs::CancellationToken;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum Operation {
    Displacement(Displacement),
    Free(FreeTransform),
    Warp(WarpMesh),
    Perspective(PerspectiveWarp),
    Puppet(PuppetWarp),
    ContentAwareScale(ContentAwareScale),
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TransformOp {
    pub version: u32,
    pub operation: Operation,
    pub kernel: Kernel,
}
impl TransformOp {
    /// Choose nearest for exact pixel-lattice affine maps, Lanczos for affine
    /// area reduction, and bicubic otherwise. This does not widen kernel support.
    pub fn effective_kernel(&self, level: u8) -> Kernel {
        if self.kernel != Kernel::Automatic {
            return self.kernel;
        }
        if let Operation::Free(t) = &self.operation {
            let m = t.matrix;
            if m[2] == [0., 0., 1.] {
                let lattice =
                    |a: f64, b: f64| (a.abs() == 1. && b == 0.) || (a == 0. && b.abs() == 1.);
                let scale = 2.0f64.powi(i32::from(level));
                let det = m[0][0] * m[1][1] - m[0][1] * m[1][0];
                if lattice(m[0][0], m[0][1])
                    && lattice(m[1][0], m[1][1])
                    && det.abs() == 1.
                    && (m[0][2] / scale).fract() == 0.
                    && (m[1][2] / scale).fract() == 0.
                {
                    return Kernel::Nearest;
                }
                if det.abs() < 1. - 1e-12 {
                    return Kernel::Lanczos3;
                }
            }
        }
        Kernel::Bicubic
    }
    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            return Err(invalid("unsupported transform version"));
        }
        match &self.operation {
            Operation::Displacement(t) => t.validate(),
            Operation::Free(t) => t.validate(),
            Operation::Warp(t) => t.validate(),
            Operation::Perspective(t) => t.validate(),
            Operation::Puppet(t) => t.validate(),
            Operation::ContentAwareScale(t) => {
                canvas(t.target_width, t.target_height)?;
                if !t.amount.is_finite()
                    || !(0. ..=1.).contains(&t.amount)
                    || t.protect.as_ref().is_some_and(|p| {
                        p.iter().any(|v| !v.is_finite() || !(0. ..=1.).contains(v))
                    })
                {
                    return Err(invalid("invalid content-aware parameters"));
                }
                Ok(())
            }
        }
    }
    /// Render into a fixed canvas. Input and output are at `level`; transform geometry is level zero.
    pub fn apply(&self, input: &Image, width: usize, height: usize, level: u8) -> Result<Image> {
        self.apply_with_cancel(input, width, height, level, &CancellationToken::new())
    }

    /// Render with cooperative cancellation, including during content-aware seam work.
    pub fn apply_with_cancel(
        &self,
        input: &Image,
        width: usize,
        height: usize,
        level: u8,
        cancel: &CancellationToken,
    ) -> Result<Image> {
        self.apply_checked(input, width, height, level, cancel, &|_| {
            check_cancel(cancel)
        })
    }

    // `completed_in_row` lets the tiny regression prove a check occurs after
    // actual mapping, rather than only at entry or before the first pixel.
    fn apply_checked(
        &self,
        input: &Image,
        width: usize,
        height: usize,
        level: u8,
        cancel: &CancellationToken,
        check: &(impl Fn(usize) -> Result<()> + Sync),
    ) -> Result<Image> {
        check(0)?;
        self.validate()?;
        let n = canvas(width, height)?;
        input.validate()?;
        let scale = 2.0f64.powi(i32::from(level));
        if let Operation::ContentAwareScale(t) = &self.operation {
            if width != (t.target_width as f64 / scale).ceil() as usize
                || height != (t.target_height as f64 / scale).ceil() as usize
            {
                return Err(invalid(
                    "content-aware canvas must match target dimensions at level",
                ));
            }

            let mut params = t.clone();
            params.target_width = width;
            params.target_height = height;
            return seam::apply_with_cancel(input, &params, cancel);
        }
        if let Operation::Free(t) = &self.operation {
            t.bounds(input.width as f64 * scale, input.height as f64 * scale)?;
        }
        let prepared = self.prepare()?;
        check(0)?;
        let kernel = self.effective_kernel(level);
        let mut planes = std::array::from_fn(|_| vec![0.; n]);
        let [r, g, b, a] = &mut planes;
        r.par_chunks_mut(width)
            .zip(g.par_chunks_mut(width))
            .zip(b.par_chunks_mut(width))
            .zip(a.par_chunks_mut(width))
            .enumerate()
            .try_for_each(|(y, (((r, g), b), a))| -> Result<()> {
                for x in 0..width {
                    if x & 1023 == 0 {
                        check(x)?;
                    }
                    let p = coordinate(&prepared, x, y, scale);
                    let rgba = sample(input, p, kernel);
                    r[x] = rgba[0];
                    g[x] = rgba[1];
                    b[x] = rgba[2];
                    a[x] = rgba[3];
                }
                check(width)?;
                Ok(())
            })?;
        check(0)?;
        Ok(Image {
            width,
            height,
            planes,
        })
    }
    /// Source pixel-center coordinates at `level`, row-major. Invalid geometry uses [-1e20; 2].
    /// This is an absolute lookup field, not a displacement offset or texel-index field.
    pub fn displacement(&self, width: usize, height: usize, level: u8) -> Result<Vec<[f32; 2]>> {
        self.validate()?;
        let n = canvas(width, height)?;
        let prepared = self.prepare()?;
        let scale = 2.0f64.powi(i32::from(level));
        Ok((0..n)
            .into_par_iter()
            .map(|i| coordinate(&prepared, i % width, i / width, scale))
            .collect())
    }
    fn prepare(&self) -> Result<Prepared> {
        Ok(match &self.operation {
            Operation::Displacement(t) => Prepared::Displacement(t.clone()),
            Operation::Free(t) => Prepared::Free(lens::Homography(t.inverse()?.matrix)),
            Operation::Warp(t) => Prepared::Warp(t.inverse_field(24)?, [t.width, t.height]),
            Operation::Perspective(t) => Prepared::Perspective(t.prepare()?),
            Operation::Puppet(t) => Prepared::Puppet(t.solve()?),
            Operation::ContentAwareScale(_) => {
                return Err(invalid(
                    "content-aware scaling has no geometry-only displacement field",
                ));
            }
        })
    }
}
enum Prepared {
    Displacement(Displacement),
    Free(lens::Homography),
    Warp(WarpInverseField, Point),
    Perspective(PreparedPerspectiveWarp),
    Puppet(PreparedPuppetWarp),
}
impl Prepared {
    #[inline]
    fn map(&self, p: Point) -> Option<Point> {
        match self {
            Self::Displacement(t) => t.inverse(p),
            Self::Free(h) => h.map(p),
            Self::Warp(f, size) => f.inverse(p).map(|uv| [uv[0] * size[0], uv[1] * size[1]]),
            Self::Perspective(t) => t.inverse(p),
            Self::Puppet(t) => t.inverse_map(p),
        }
    }
}
#[inline]
fn coordinate(prepared: &Prepared, x: usize, y: usize, scale: f64) -> [f32; 2] {
    prepared
        .map([(x as f64 + 0.5) * scale, (y as f64 + 0.5) * scale])
        .map(|p| [(p[0] / scale) as f32, (p[1] / scale) as f32])
        .filter(|p| p.iter().all(|v| v.is_finite()))
        .unwrap_or([-1e20; 2])
}
fn invalid(s: &str) -> Error {
    Error::Invalid(s.into())
}
fn canvas(width: usize, height: usize) -> Result<usize> {
    width
        .checked_mul(height)
        .filter(|n| width > 0 && height > 0 && *n <= crate::MAX_IMAGE_PIXELS)
        .ok_or_else(|| invalid("nonempty canvas of at most 100 MP required"))
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn nonseam_row_checks_a_token_after_mapping_begins() {
        const WIDTH: usize = 1025;
        let image = Image::new(WIDTH, 1, std::array::from_fn(|_| vec![0.25; WIDTH])).unwrap();
        let op = TransformOp {
            version: 1,
            operation: Operation::Free(FreeTransform::identity()),
            kernel: Kernel::Nearest,
        };
        let token = CancellationToken::new();
        let mapped_in_row = AtomicUsize::new(0);
        let result = op.apply_checked(&image, WIDTH, 1, 0, &token, &|done| {
            mapped_in_row.store(done, Ordering::SeqCst);
            if done >= 1024 {
                token.cancel();
            }
            check_cancel(&token)
        });
        assert!(matches!(result, Err(Error::Cancelled)));
        assert_eq!(mapped_in_row.load(Ordering::SeqCst), 1024);
        assert!(mapped_in_row.load(Ordering::SeqCst) < WIDTH);
    }
}
