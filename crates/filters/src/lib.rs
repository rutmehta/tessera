//! Raster filters; see README.md for contracts and approximation bounds.
mod compositor_adapter;
pub use compositor_adapter::{CompositorFilters, detect_distractions, neural_catalog};
pub mod adaptive_lattice;
pub mod adjust;
pub mod caf;
#[cfg(feature = "camera-raw-filter")]
pub mod camera_raw;
#[cfg(feature = "camera-raw-filter")]
pub mod camera_raw_gpu;
mod cpu;
pub mod distort;
pub mod distraction;
mod evaluation;
pub mod gpu;
mod large;
pub mod liquify;
pub mod liquify_gpu;
pub mod registry;
pub mod remove;
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
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
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
    use std::time::Instant;
    fn fixture(w: u32, h: u32) -> Raster {
        let mut r = Raster::new(Extent::new(w, h), 4, Depth::F32, 0.0);
        r.edit_region(Rect::of_extent(r.extent()), 1, |x, y, p| {
            *p = [((x * 13 + y * 7) % 31) as f32 / 31.0, 0.2, 0.4, 1.0]
        })
        .unwrap();
        r
    }

    /// Frozen copy of the pre-PERF-4 scalar kernel and pass order. Keep this
    /// independent of `gaussian_kernel` and `convolve` so loop changes are
    /// checked against the baseline arithmetic.
    fn baseline_gaussian(src: &Buffer, sigma: f32) -> Vec<[f32; 4]> {
        // Explicit oracle extension: the documented sigma-zero identity path
        // avoids the baseline kernel's otherwise undefined 0/0 exponent.
        if sigma == 0.0 {
            return src.pixels.clone();
        }
        let radius = (3.0 * sigma).ceil() as i32;
        let mut kernel: Vec<f32> = (-radius..=radius)
            .map(|x| (-0.5 * (x as f32 / sigma).powi(2)).exp())
            .collect();
        let total: f32 = kernel.iter().sum();
        for weight in &mut kernel {
            *weight /= total;
        }

        let mut a = src.pixels.clone();
        let mut b = src.pixels.clone();
        for vertical in [false, true] {
            for y in 0..src.h {
                for x in 0..src.w {
                    let mut sum = [0.0; 4];
                    for (j, &weight) in kernel.iter().enumerate() {
                        let d = j as i32 - radius;
                        let sx = x as i32 + if vertical { 0 } else { d };
                        let sy = y as i32 + if vertical { d } else { 0 };
                        let index = sy.clamp(0, src.h as i32 - 1) as usize * src.w
                            + sx.clamp(0, src.w as i32 - 1) as usize;
                        let pixel = a[index];
                        for c in 0..4 {
                            sum[c] += weight * pixel[c];
                        }
                    }
                    b[y * src.w + x] = sum;
                }
            }
            std::mem::swap(&mut a, &mut b);
        }
        a
    }

    fn gaussian_fixture(w: usize, h: usize) -> Buffer {
        Buffer {
            w,
            h,
            pixels: (0..w * h)
                .map(|i| {
                    let x = (i % w) as f32;
                    let y = (i / w) as f32;
                    [
                        (x * 0.17 - y * 0.09).sin() * 1.8,
                        ((x * 7.0 + y * 11.0) % 29.0) / 13.0 - 0.7,
                        (y * 0.21).cos() * 2.2,
                        0.13 + ((x + 2.0 * y) % 17.0) / 23.0,
                    ]
                })
                .collect(),
        }
    }

    fn assert_close_pixels(actual: &[[f32; 4]], expected: &[[f32; 4]], context: &str) {
        assert_eq!(actual.len(), expected.len());
        let mut max_error = 0.0_f32;
        let mut worst = (0, 0, 0.0_f32, 0.0_f32);
        for (i, (a, e)) in actual.iter().zip(expected).enumerate() {
            for c in 0..4 {
                assert!(a[c].is_finite(), "{context}: nonfinite actual at {i}/{c}");
                assert!(e[c].is_finite(), "{context}: nonfinite expected at {i}/{c}");
                let error = (a[c] - e[c]).abs();
                if error > max_error {
                    max_error = error;
                    worst = (i, c, a[c], e[c]);
                }
            }
        }
        assert!(
            max_error <= 1.0 / 65_535.0,
            "{context}: max error {max_error} at pixel/channel {:?}",
            worst
        );
    }

    #[test]
    fn gaussian_matches_frozen_baseline_for_edges_hdr_and_fractional_alpha() {
        let cancel = AtomicBool::new(false);
        for (w, h, sigma) in [
            (1, 1, 0.0),
            (1, 7, 0.5),
            (8, 1, 1.25),
            (3, 2, 0.5),
            (9, 5, 1.25),
            (47, 23, 12.0),
        ] {
            let src = gaussian_fixture(w, h);
            let original = src.pixels.clone();
            let expected = baseline_gaussian(&src, sigma);
            let actual = convolve(&src, &gaussian_kernel(sigma), &cancel).unwrap();
            assert_close_pixels(
                &actual.pixels,
                &expected,
                &format!("{w}x{h}, sigma={sigma}"),
            );
            let repeated = convolve(&src, &gaussian_kernel(sigma), &cancel).unwrap();
            assert_eq!(actual.pixels, repeated.pixels);
            assert_eq!(
                src.pixels, original,
                "input mutated for {w}x{h}, sigma={sigma}"
            );
            if sigma == 0.0 {
                assert_eq!(actual.pixels, src.pixels, "sigma zero must be identity");
            }
        }
    }

    #[test]
    fn gaussian_returns_cancelled_when_pre_cancelled_before_first_row() {
        let src = gaussian_fixture(7, 5);
        let cancel = AtomicBool::new(true);
        assert!(matches!(
            convolve(&src, &gaussian_kernel(1.25), &cancel),
            Err(EngineError::Cancelled)
        ));
    }

    fn raster_digest(raster: &Raster) -> u64 {
        let (nx, ny) = raster.grid();
        let mut samples = Vec::new();
        let mut digest = 0xcbf2_9ce4_8422_2325_u64;
        for ty in 0..ny {
            for tx in 0..nx {
                raster.read_tile(tx, ty, &mut samples).unwrap();
                for value in &samples {
                    digest ^= u64::from(value.to_bits());
                    digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
        }
        digest
    }

    fn buffer_digest(pixels: &[[f32; 4]]) -> u64 {
        let mut digest = 0xcbf2_9ce4_8422_2325_u64;
        for pixel in pixels {
            for value in pixel {
                digest ^= u64::from(value.to_bits());
                digest = digest.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        digest
    }

    fn gaussian_raster_fixture(w: u32, h: u32) -> Raster {
        let extent = Extent::new(w, h);
        let mut input = Raster::new(extent, 4, Depth::F32, 0.0);
        input
            .edit_region(Rect::of_extent(extent), 1, |x, y, p| {
                *p = [
                    (x % 31) as f32 / 8.0 - 1.0,
                    (y % 37) as f32 / 12.0 - 0.5,
                    ((x * 13 + y * 7) % 31) as f32 / 9.0 - 0.8,
                    0.17 + ((x + y) % 23) as f32 / 37.0,
                ]
            })
            .unwrap();
        input
    }

    #[test]
    #[ignore = "24 MP r12 public apply benchmark; run before and after PERF-4 on the same host/load"]
    fn benchmark_gaussian_apply_r12_24mp() {
        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
        const W: u32 = 6000;
        const H: u32 = 4000;
        const TRIALS: usize = 3;
        let host = std::env::var("PERF4_HOST").unwrap_or_else(|_| "unspecified".into());
        let load = std::env::var("PERF4_LOAD").unwrap_or_else(|_| "unspecified".into());
        eprintln!(
            "PERF4 apply host={host} load={load} dimensions={W}x{H} sigma=12 trials={TRIALS}"
        );

        let input = gaussian_raster_fixture(W, H);
        let params = FilterParams {
            amount: 1.0,
            radius: 12.0,
            ..Default::default()
        };
        let cancel = AtomicBool::new(false);
        let mut timings = Vec::with_capacity(TRIALS);
        let mut expected_digest = None;
        for trial in 0..TRIALS {
            let start = Instant::now();
            let output = Effect::Gaussian.apply(&input, &params, &cancel).unwrap();
            let elapsed = start.elapsed();
            let digest = raster_digest(&output);
            if let Some(expected) = expected_digest {
                assert_eq!(digest, expected, "output digest changed at trial {trial}");
            }
            expected_digest = Some(digest);
            timings.push(elapsed.as_secs_f64() * 1000.0);
            eprintln!(
                "PERF4 apply trial={trial} wall_ms={:.3} digest={digest:016x}",
                timings[trial]
            );
            drop(output);
        }
        timings.sort_by(f64::total_cmp);
        eprintln!("PERF4 apply median_ms={:.3}", timings[1]);
    }

    #[test]
    #[ignore = "24 MP r12 paired CPU benchmark; diagnostic under load, not a CI timing gate"]
    fn benchmark_gaussian_r12_24mp_against_frozen_baseline() {
        assert!(!cfg!(debug_assertions), "run this benchmark with --release");
        const W: usize = 6000;
        const H: usize = 4000;
        const SIGMA: f32 = 12.0;
        const TRIALS: usize = 3;
        let host = std::env::var("PERF4_HOST").unwrap_or_else(|_| "unspecified".into());
        let load = std::env::var("PERF4_LOAD").unwrap_or_else(|_| "unspecified".into());
        eprintln!("PERF4 host={host} load={load} dimensions={W}x{H} sigma={SIGMA} trials={TRIALS}");

        let src = gaussian_fixture(W, H);
        let kernel = gaussian_kernel(SIGMA);
        let cancel = AtomicBool::new(false);
        let mut baseline_ms = Vec::with_capacity(TRIALS);
        let mut optimized_ms = Vec::with_capacity(TRIALS);

        for trial in 0..TRIALS {
            let (baseline_elapsed, baseline_digest, optimized_elapsed, optimized_digest) =
                if trial % 2 == 0 {
                    let start = Instant::now();
                    let expected = baseline_gaussian(&src, SIGMA);
                    let baseline_elapsed = start.elapsed();
                    let baseline_digest = buffer_digest(&expected);
                    drop(expected);

                    let start = Instant::now();
                    let actual = convolve(&src, &kernel, &cancel).unwrap();
                    let optimized_elapsed = start.elapsed();
                    let optimized_digest = buffer_digest(&actual.pixels);
                    drop(actual);
                    (
                        baseline_elapsed,
                        baseline_digest,
                        optimized_elapsed,
                        optimized_digest,
                    )
                } else {
                    let start = Instant::now();
                    let actual = convolve(&src, &kernel, &cancel).unwrap();
                    let optimized_elapsed = start.elapsed();
                    let optimized_digest = buffer_digest(&actual.pixels);
                    drop(actual);

                    let start = Instant::now();
                    let expected = baseline_gaussian(&src, SIGMA);
                    let baseline_elapsed = start.elapsed();
                    let baseline_digest = buffer_digest(&expected);
                    drop(expected);
                    (
                        baseline_elapsed,
                        baseline_digest,
                        optimized_elapsed,
                        optimized_digest,
                    )
                };
            assert_eq!(
                optimized_digest, baseline_digest,
                "trial {trial}: whole-output digest mismatch"
            );
            baseline_ms.push(baseline_elapsed.as_secs_f64() * 1000.0);
            optimized_ms.push(optimized_elapsed.as_secs_f64() * 1000.0);
            eprintln!(
                "PERF4 trial={trial} baseline_ms={:.3} optimized_ms={:.3} digest={baseline_digest:016x}",
                baseline_ms[trial], optimized_ms[trial],
            );
        }
        // Keep full pixel-by-pixel parity independent from the timed pairs.
        // The two 384 MiB outputs coexist only here, after all trial timers.
        let expected = baseline_gaussian(&src, SIGMA);
        let actual = convolve(&src, &kernel, &cancel).unwrap();
        assert_close_pixels(&actual.pixels, &expected, "24MP post-timing parity");
        drop(actual);
        drop(expected);

        baseline_ms.sort_by(f64::total_cmp);
        optimized_ms.sort_by(f64::total_cmp);
        let speedup = baseline_ms[1] / optimized_ms[1];
        eprintln!(
            "PERF4 median_baseline_ms={:.3} median_optimized_ms={:.3} speedup={speedup:.3}x",
            baseline_ms[1], optimized_ms[1]
        );
        assert!(
            speedup >= 2.0,
            "PERF-4 target is at least 2x; got {speedup:.3}x"
        );
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
