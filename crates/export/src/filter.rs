use engine_api::{EngineError, EngineResult, jobs::CancellationToken};
use image::Rgb32FImage;
use rayon::prelude::*;

#[cfg(test)]
#[path = "filter_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Default)]
pub enum Resize {
    #[default]
    None,
    LongEdge(u32),
    Fit(u32, u32),
    /// 100 is original size. Enlargement is permitted.
    Percent(f64),
}
#[derive(Clone, Copy, Debug, Default)]
pub enum SharpenFor {
    #[default]
    None,
    Screen,
    Matte,
    Glossy,
}
impl Resize {
    pub fn dimensions(self, width: u32, height: u32) -> EngineResult<(u32, u32)> {
        if width == 0 || height == 0 {
            return Err(EngineError::invalid("resize", "empty image"));
        }
        let scale = match self {
            Self::None => 1.0,
            Self::LongEdge(n) => f64::from(n) / f64::from(width.max(height)),
            Self::Fit(w, h) => {
                (f64::from(w) / f64::from(width)).min(f64::from(h) / f64::from(height))
            }
            Self::Percent(p) => p / 100.0,
        };
        let w = (f64::from(width) * scale).round().max(1.0);
        let h = (f64::from(height) * scale).round().max(1.0);
        if !scale.is_finite()
            || scale <= 0.0
            || w > f64::from(u32::MAX)
            || h > f64::from(u32::MAX)
            || w * h > 100_000_000.0
        {
            return Err(EngineError::invalid(
                "resize",
                "invalid scale or output exceeds 100 megapixels",
            ));
        }
        Ok((w as u32, h as u32))
    }
}

fn weights(src: u32, dst: u32) -> Vec<Vec<(u32, f32)>> {
    let ratio = f64::from(src) / f64::from(dst);
    let scale = ratio.max(1.0);
    let sinc = |x: f64| {
        if x.abs() < 1e-12 {
            1.0
        } else {
            let p = std::f64::consts::PI * x;
            p.sin() / p
        }
    };
    (0..dst)
        .map(|i| {
            let center = (f64::from(i) + 0.5) * ratio - 0.5;
            let left = (center - 3.0 * scale).ceil() as i64;
            let right = (center + 3.0 * scale).floor() as i64;
            let mut row: Vec<_> = (left..=right)
                .map(|j| {
                    let x = (j as f64 - center) / scale;
                    (
                        j.clamp(0, i64::from(src) - 1) as u32,
                        (sinc(x) * sinc(x / 3.0)) as f32,
                    )
                })
                .collect();
            let sum: f32 = row.iter().map(|v| v.1).sum();
            for (_, w) in &mut row {
                *w /= sum;
            }
            row
        })
        .collect()
}

/// Separable Lanczos-3 with widened support when reducing (anti-aliasing).
/// Every pass is row-parallel, with cooperative cancellation once per row.
pub(crate) fn resize(
    src: Rgb32FImage,
    mode: Resize,
    cancel: &CancellationToken,
) -> EngineResult<Rgb32FImage> {
    let (w, h) = mode.dimensions(src.width(), src.height())?;
    if (w, h) == src.dimensions() {
        return Ok(src);
    }
    if u64::from(w) * u64::from(src.height()) > 100_000_000 {
        return Err(EngineError::invalid(
            "resize",
            "intermediate exceeds 100 megapixels",
        ));
    }
    let wx = weights(src.width(), w);
    let wy = weights(src.height(), h);
    let mut mid = Rgb32FImage::new(w, src.height());
    mid.as_mut()
        .par_chunks_mut(w as usize * 3)
        .enumerate()
        .try_for_each(|(y, row)| -> EngineResult<()> {
            cancel.check()?;
            for (x, p) in row.chunks_mut(3).enumerate() {
                for &(sx, weight) in &wx[x] {
                    for (c, value) in p.iter_mut().enumerate() {
                        *value += src.get_pixel(sx, y as u32)[c] * weight;
                    }
                }
            }
            Ok(())
        })?;
    drop(src);
    let mut out = Rgb32FImage::new(w, h);
    out.as_mut()
        .par_chunks_mut(w as usize * 3)
        .enumerate()
        .try_for_each(|(y, row)| -> EngineResult<()> {
            cancel.check()?;
            for (x, p) in row.chunks_mut(3).enumerate() {
                for &(sy, weight) in &wy[y] {
                    for (c, value) in p.iter_mut().enumerate() {
                        *value += mid.get_pixel(x as u32, sy)[c] * weight;
                    }
                }
                for value in p {
                    *value = value.clamp(0.0, 1.0);
                }
            }
            Ok(())
        })?;
    Ok(out)
}

/// Small Gaussian unsharp mask in output-sized display RGB. Presets are
/// (sigma pixels, amount): Screen (.6,.5), Matte (1.2,1), Glossy (.8,.7).
pub(crate) fn sharpen(
    src: Rgb32FImage,
    preset: SharpenFor,
    cancel: &CancellationToken,
) -> EngineResult<Rgb32FImage> {
    let (sigma, amount): (f32, f32) = match preset {
        SharpenFor::None => return Ok(src),
        SharpenFor::Screen => (0.6, 0.5),
        SharpenFor::Matte => (1.2, 1.0),
        SharpenFor::Glossy => (0.8, 0.7),
    };
    let radius = (sigma * 3.0).ceil() as i32;
    let mut kernel: Vec<f32> = (-radius..=radius)
        .map(|x| (-(x * x) as f32 / (2.0 * sigma * sigma)).exp())
        .collect();
    let sum: f32 = kernel.iter().sum();
    for v in &mut kernel {
        *v /= sum;
    }
    let (w, h) = src.dimensions();
    let mut mid = Rgb32FImage::new(w, h);
    mid.as_mut()
        .par_chunks_mut(w as usize * 3)
        .enumerate()
        .try_for_each(|(y, row)| -> EngineResult<()> {
            cancel.check()?;
            for (x, p) in row.chunks_mut(3).enumerate() {
                for (k, &weight) in kernel.iter().enumerate() {
                    let sx =
                        (x as i64 + k as i64 - i64::from(radius)).clamp(0, i64::from(w) - 1) as u32;
                    for (c, v) in p.iter_mut().enumerate() {
                        *v += src.get_pixel(sx, y as u32)[c] * weight;
                    }
                }
            }
            Ok(())
        })?;
    let mut out = src;
    out.as_mut()
        .par_chunks_mut(w as usize * 3)
        .enumerate()
        .try_for_each(|(y, row)| -> EngineResult<()> {
            cancel.check()?;
            for (x, p) in row.chunks_mut(3).enumerate() {
                let mut blur = [0.0; 3];
                for (k, &weight) in kernel.iter().enumerate() {
                    let sy =
                        (y as i64 + k as i64 - i64::from(radius)).clamp(0, i64::from(h) - 1) as u32;
                    for (c, v) in blur.iter_mut().enumerate() {
                        *v += mid.get_pixel(x as u32, sy)[c] * weight;
                    }
                }
                for c in 0..3 {
                    p[c] = (p[c] + amount * (p[c] - blur[c])).clamp(0.0, 1.0);
                }
            }
            Ok(())
        })?;
    Ok(out)
}
