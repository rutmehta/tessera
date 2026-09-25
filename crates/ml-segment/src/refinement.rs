use crate::MaskRaster;
use anyhow::{Result, ensure};
use image::{ImageBuffer, Luma, RgbImage, imageops};

/// M2-08 guided-filter equations (truncated windows, f64 moments), adapted to
/// an sRGB preview guide. The pipeline implementation is private and uses
/// linear ProPhoto instead. Radius is in output pixels; zero means resize only.
pub fn refine(
    mask: &MaskRaster,
    guide: &RgbImage,
    radius: u32,
    epsilon: f32,
) -> Result<MaskRaster> {
    ensure!(guide.width() > 0 && guide.height() > 0, "empty guide");
    ensure!(epsilon.is_finite() && epsilon > 0., "invalid epsilon");
    let (w, h) = (guide.width() as usize, guide.height() as usize);
    let image: ImageBuffer<Luma<f32>, Vec<f32>> =
        ImageBuffer::from_raw(mask.width(), mask.height(), mask.data().to_vec()).unwrap();
    // Avoid mixing foreground/background before the guide can constrain edges.
    // The guided filter smooths nearest-neighbour blocks using image structure.
    let filter = if w > mask.width() as usize || h > mask.height() as usize {
        imageops::FilterType::Nearest
    } else {
        imageops::FilterType::Triangle
    };
    let p = imageops::resize(&image, w as u32, h as u32, filter).into_raw();
    if radius == 0 {
        return Ok(MaskRaster::new(w as u32, h as u32, p)?);
    }
    let linear = |v: u8| {
        let v = v as f64 / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let guide: Vec<f64> = guide
        .pixels()
        .map(|p| 0.2126 * linear(p[0]) + 0.7152 * linear(p[1]) + 0.0722 * linear(p[2]))
        .collect();
    let p: Vec<f64> = p.iter().map(|&p| p as f64).collect();
    let mean = |v: &[f64]| box_mean(v, w, h, radius as usize);
    let mi = mean(&guide);
    let mp = mean(&p);
    let ii = mean(&guide.iter().map(|v| v * v).collect::<Vec<_>>());
    let ip = mean(&guide.iter().zip(&p).map(|(i, p)| i * p).collect::<Vec<_>>());
    let a: Vec<_> = (0..p.len())
        .map(|i| (ip[i] - mi[i] * mp[i]) / ((ii[i] - mi[i] * mi[i]).max(0.) + epsilon as f64))
        .collect();
    let b: Vec<_> = (0..p.len()).map(|i| mp[i] - a[i] * mi[i]).collect();
    let a = mean(&a);
    let b = mean(&b);
    Ok(MaskRaster::new(
        w as u32,
        h as u32,
        (0..p.len())
            .map(|i| (a[i] * guide[i] + b[i]).clamp(0., 1.) as f32)
            .collect(),
    )?)
}
fn box_mean(v: &[f64], w: usize, h: usize, r: usize) -> Vec<f64> {
    let stride = w + 1;
    let mut sums = vec![0.; stride * (h + 1)];
    for y in 0..h {
        let mut row = 0.;
        for x in 0..w {
            row += v[y * w + x];
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
        }
    }
    let mut out = vec![0.; w * h];
    for y in 0..h {
        for x in 0..w {
            let (x0, y0) = (x.saturating_sub(r), y.saturating_sub(r));
            let (x1, y1) = (
                x.saturating_add(r).saturating_add(1).min(w),
                y.saturating_add(r).saturating_add(1).min(h),
            );
            out[y * w + x] =
                (sums[y1 * stride + x1] - sums[y0 * stride + x1] - sums[y1 * stride + x0]
                    + sums[y0 * stride + x0])
                    / ((x1 - x0) * (y1 - y0)) as f64;
        }
    }
    out
}
