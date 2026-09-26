//! Raster filters; see README.md for contracts and approximation bounds.
pub mod adjust;
mod cpu;
pub mod distort;
mod evaluation;
pub mod gpu;
mod large;
pub use evaluation::{CameraRawFilter, CameraRawProcessor, SmartFilter, SmartFilters};

use compositor::{geom::Rect, raster::Raster};
use engine_api::{EngineError, EngineResult};
use std::sync::atomic::{AtomicBool, Ordering};

/// Explicit full-image barrier avoids misrepresenting global operators as local.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halo {
    Radius(u32),
    WholeImage,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    Gaussian,
    Box,
    Motion,
    RadialSpin,
    RadialZoom,
    LensBlur,
    SurfaceBlur,
    UnsharpMask,
    SmartSharpen,
    HighPass,
    AddNoise,
    ReduceNoise,
    Median,
    DustScratches,
    Distort(distort::Distortion),
    Emboss,
    FindEdges,
    Solarize,
    OilPaint,
    Clouds,
    DifferenceClouds,
    LensFlare,
    Adjust,
    CameraRaw,
}
impl Effect {
    pub fn inventory() -> Vec<Self> {
        use distort::Distortion::*;
        let mut list = vec![
            Self::Gaussian,
            Self::Box,
            Self::Motion,
            Self::RadialSpin,
            Self::RadialZoom,
            Self::LensBlur,
            Self::SurfaceBlur,
            Self::UnsharpMask,
            Self::SmartSharpen,
            Self::HighPass,
            Self::AddNoise,
            Self::ReduceNoise,
            Self::Median,
            Self::DustScratches,
            Self::Emboss,
            Self::FindEdges,
            Self::Solarize,
            Self::OilPaint,
            Self::Clouds,
            Self::DifferenceClouds,
            Self::LensFlare,
            Self::Adjust,
            Self::CameraRaw,
        ];
        list.extend(
            [
                Pinch,
                Spherize,
                Twirl,
                Wave,
                Ripple,
                PolarToRectangular,
                RectangularToPolar,
                Offset,
            ]
            .map(Self::Distort),
        );
        list
    }
}

/// `amount` is the final effect opacity, in [0,1]; zero is an exact COW identity.
#[derive(Clone, Debug)]
pub struct FilterParams {
    /// Gaussian sigma (pixels); other neighbourhood filters use support radius.
    pub radius: f32,
    pub amount: f32,
    /// Radians for motion direction and radial spin; zoom fraction for zoom blur.
    pub angle: f32,
    /// Range sigma for bilateral; threshold for unsharp/dust/solarize.
    pub threshold: f32,
    /// Sharpen gain or noise standard deviation/amplitude.
    pub strength: f32,
    pub seed: u32,
    pub monochrome: bool,
    pub gaussian_noise: bool,
    /// Same-size row-major near=0 far=1 depth, e.g. DepthMap::near_to_far().
    pub depth: Option<Vec<f32>>,
    pub focus: [f32; 2],
    pub adjust: adjust::Adjustment,
    pub distort: distort::DistortParams,
}
impl Default for FilterParams {
    fn default() -> Self {
        Self {
            radius: 1.0,
            amount: 0.0,
            angle: 0.0,
            threshold: 0.1,
            strength: 0.1,
            seed: 0,
            monochrome: false,
            gaussian_noise: false,
            depth: None,
            focus: [0.0, 0.0],
            adjust: Default::default(),
            distort: Default::default(),
        }
    }
}

/// Fallible, immutable evaluation: invalid controls/cancellation never publish
/// partially rendered rasters. The source and all of its revisions are retained.
pub trait Filter: Send + Sync {
    fn apply(
        &self,
        input: &Raster,
        params: &FilterParams,
        cancel: &AtomicBool,
    ) -> EngineResult<Raster>;
    fn halo(&self, params: &FilterParams) -> Halo;
}

pub(crate) fn checkpoint(cancel: &AtomicBool) -> EngineResult<()> {
    if cancel.load(Ordering::Relaxed) {
        Err(EngineError::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Clone)]
pub(crate) struct Buffer {
    pub w: usize,
    pub h: usize,
    pub pixels: Vec<[f32; 4]>,
}
impl Buffer {
    fn read(input: &Raster, cancel: &AtomicBool) -> EngineResult<Self> {
        let e = input.extent();
        if e.width == 0 || e.height == 0 || !matches!(input.channels(), 1 | 3 | 4) {
            return Err(EngineError::invalid(
                "filters",
                "nonempty 1/3/4-channel raster required",
            ));
        }
        let mut pixels = vec![[0.0; 4]; e.area() as usize];
        let mut samples = Vec::new();
        let (nx, ny) = input.grid();
        for ty in 0..ny {
            for tx in 0..nx {
                checkpoint(cancel)?;
                input.read_tile(tx, ty, &mut samples)?;
                let l = input.layout(tx, ty);
                for y in 0..l.extent.height as usize {
                    for x in 0..l.extent.width as usize {
                        let p = &mut pixels
                            [(ty as usize * 256 + y) * e.width as usize + tx as usize * 256 + x];
                        for c in 0..input.channels() as usize {
                            p[c] = samples[c * l.plane_len() + y * l.stride() + x];
                        }
                        if input.channels() == 1 {
                            p[1] = p[0];
                            p[2] = p[0];
                        }
                        if input.channels() != 4 {
                            p[3] = 1.0;
                        }
                    }
                }
            }
        }
        if pixels.iter().flatten().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid("filters", "finite pixels required"));
        }
        Ok(Self {
            w: e.width as usize,
            h: e.height as usize,
            pixels,
        })
    }
    fn write(&self, input: &Raster, cancel: &AtomicBool) -> EngineResult<Raster> {
        checkpoint(cancel)?;
        if self.pixels.iter().flatten().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid("filters", "nonfinite result"));
        }
        let mut out = input.clone();
        let rev = input
            .max_rev()
            .checked_add(1)
            .ok_or_else(|| EngineError::invalid("filters", "revision overflow"))?;
        let (nx, ny) = input.grid();
        for ty in 0..ny {
            for tx in 0..nx {
                checkpoint(cancel)?;
                out.edit_region(
                    Rect::new(
                        i64::from(tx) * 256,
                        i64::from(ty) * 256,
                        i64::from(tx + 1) * 256,
                        i64::from(ty + 1) * 256,
                    ),
                    rev,
                    |x, y, p| *p = self.pixels[y as usize * self.w + x as usize],
                )?;
            }
        }
        checkpoint(cancel)?;
        Ok(out)
    }
    pub(crate) fn at(&self, x: i32, y: i32) -> [f32; 4] {
        self.pixels[y.clamp(0, self.h as i32 - 1) as usize * self.w
            + x.clamp(0, self.w as i32 - 1) as usize]
    }
}

pub(crate) fn gaussian_kernel(sigma: f32) -> Vec<f32> {
    if sigma == 0.0 {
        return vec![1.0];
    }
    let r = (3.0 * sigma).ceil() as i32;
    let mut k: Vec<_> = (-r..=r)
        .map(|x| (-0.5 * (x as f32 / sigma).powi(2)).exp())
        .collect();
    let total: f32 = k.iter().sum();
    for v in &mut k {
        *v /= total;
    }
    k
}
pub(crate) fn convolve(src: &Buffer, k: &[f32], cancel: &AtomicBool) -> EngineResult<Buffer> {
    let mut a = src.clone();
    let mut b = src.clone();
    let r = (k.len() / 2) as i32;
    for vertical in [false, true] {
        for y in 0..src.h {
            checkpoint(cancel)?;
            for x in 0..src.w {
                let mut sum = [0.0; 4];
                for (j, &weight) in k.iter().enumerate() {
                    let d = j as i32 - r;
                    let v = a.at(
                        x as i32 + if vertical { 0 } else { d },
                        y as i32 + if vertical { d } else { 0 },
                    );
                    for c in 0..4 {
                        sum[c] += weight * v[c];
                    }
                }
                b.pixels[y * src.w + x] = sum;
            }
        }
        std::mem::swap(&mut a, &mut b);
    }
    Ok(a)
}

impl Filter for Effect {
    fn halo(&self, p: &FilterParams) -> Halo {
        if p.amount == 0.0 {
            return Halo::Radius(0);
        }
        match self {
            Self::Gaussian | Self::UnsharpMask | Self::HighPass if p.radius > 32.0 => {
                Halo::WholeImage
            }
            Self::Gaussian | Self::UnsharpMask | Self::HighPass => {
                Halo::Radius((3.0 * p.radius).ceil() as u32)
            }
            Self::Box | Self::Median | Self::DustScratches | Self::SurfaceBlur => {
                Halo::Radius(p.radius.ceil() as u32)
            }
            Self::Motion => Halo::Radius(p.radius.ceil() as u32 + 3),
            Self::SmartSharpen | Self::ReduceNoise => Halo::Radius(9),
            Self::Emboss | Self::FindEdges => Halo::Radius(1),
            Self::Adjust if !matches!(p.adjust, adjust::Adjustment::MatchColour { .. }) => {
                Halo::Radius(0)
            }
            Self::Solarize | Self::OilPaint | Self::LensFlare => Halo::Radius(0),
            _ => Halo::WholeImage,
        }
    }
    fn apply(&self, input: &Raster, p: &FilterParams, cancel: &AtomicBool) -> EngineResult<Raster> {
        checkpoint(cancel)?;
        validate(p)?;
        if p.amount == 0.0 {
            return Ok(input.clone());
        }
        let src = Buffer::read(input, cancel)?;
        let mut dst = cpu::run(*self, &src, p, cancel)?;
        for (a, b) in src.pixels.iter().zip(&mut dst.pixels) {
            for c in 0..4 {
                b[c] = a[c] + p.amount * (b[c] - a[c]);
            }
        }
        dst.write(input, cancel)
    }
}

pub(crate) fn validate(p: &FilterParams) -> EngineResult<()> {
    if !p.radius.is_finite()
        || !(0.0..=250.0).contains(&p.radius)
        || !p.amount.is_finite()
        || !(0.0..=1.0).contains(&p.amount)
        || !p.angle.is_finite()
        || !p.threshold.is_finite()
        || p.threshold < 0.0
        || !p.strength.is_finite()
        || !(0.0..=10.0).contains(&p.strength)
    {
        return Err(EngineError::invalid(
            "filters",
            "invalid radius/amount/angle/threshold/strength",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use compositor::{
        geom::Rect,
        raster::{Depth, Raster},
    };
    use engine_api::tile::Extent;
    use std::sync::atomic::AtomicBool;
    fn fixture(w: u32, h: u32) -> Raster {
        let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
        r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
            *p = [((x * 13 + y * 7) % 31) as f32 / 31.0, 0.2, 0.4, 1.0]
        })
        .unwrap();
        r
    }
    #[test]
    fn gaussian_identity_and_scalar_reference() {
        let r = fixture(11, 9);
        let cancel = AtomicBool::new(false);
        assert!(
            Effect::Gaussian
                .apply(&r, &FilterParams::default(), &cancel)
                .unwrap()
                .shares_all_tiles_with(&r)
        );
        let p = FilterParams {
            radius: 1.25,
            amount: 1.0,
            ..Default::default()
        };
        let out = Effect::Gaussian.apply(&r, &p, &cancel).unwrap();
        let k = gaussian_kernel(p.radius);
        let rad = (k.len() / 2) as i32;
        for y in 0..9 {
            for x in 0..11 {
                let mut v = 0.0;
                for dy in -rad..=rad {
                    for dx in -rad..=rad {
                        v += r.pixel((x + dx).clamp(0, 10) as u32, (y + dy).clamp(0, 8) as u32)[0]
                            * k[(dx + rad) as usize]
                            * k[(dy + rad) as usize];
                    }
                }
                assert!((out.pixel(x as u32, y as u32)[0] - v).abs() < 1e-5);
            }
        }
    }
}
