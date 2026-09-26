//! Focus area: in-focus regions from a local sharpness map.

use crate::filter::{box_mean, otsu};
use crate::mask::{Image, Mask};

/// Focus-area settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusOptions {
    /// Sharpness window radius, pixels.
    pub radius: usize,
    /// `0..=1`: 0.5 uses the Otsu threshold; higher selects more.
    pub in_focus_range: f32,
    /// Laplacian energy attributed to noise (subtracted), linear units.
    pub noise_level: f32,
    /// Majority-filter radius for the result.
    pub smooth: usize,
}

impl Default for FocusOptions {
    fn default() -> Self {
        Self {
            radius: 2,
            in_focus_range: 0.5,
            noise_level: 0.0,
            smooth: 2,
        }
    }
}

/// Local Laplacian energy (mean of the squared 4-neighbour Laplacian).
pub fn sharpness(img: &Image, radius: usize) -> Vec<f32> {
    let (w, h) = (img.width as usize, img.height as usize);
    let l = img.luminance();
    let at = |x: i64, y: i64| {
        l[y.clamp(0, h as i64 - 1) as usize * w + x.clamp(0, w as i64 - 1) as usize]
    };
    let mut lap2 = vec![0.0f32; w * h];
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let v = at(x - 1, y) + at(x + 1, y) + at(x, y - 1) + at(x, y + 1) - 4.0 * at(x, y);
            lap2[y as usize * w + x as usize] = v * v;
        }
    }
    box_mean(&lap2, w, h, radius)
}

/// Selects in-focus regions: log sharpness thresholded (Otsu, shifted by
/// `in_focus_range`) and majority-filtered.
pub fn focus_area(img: &Image, o: &FocusOptions) -> Mask {
    let (w, h) = (img.width as usize, img.height as usize);
    if w == 0 || h == 0 {
        return Mask::new(img.width, img.height);
    }
    let s: Vec<f32> = sharpness(img, o.radius)
        .into_iter()
        .map(|e| ((e - o.noise_level).max(0.0) + 1e-10).log10())
        .collect();
    let (lo, hi) = s
        .iter()
        .fold((f32::INFINITY, f32::NEG_INFINITY), |(a, b), v| {
            (a.min(*v), b.max(*v))
        });
    let t = otsu(&s, 256) + (0.5 - o.in_focus_range.clamp(0.0, 1.0)) * (hi - lo);
    let bin: Vec<f32> = s.iter().map(|v| f32::from(u8::from(*v > t))).collect();
    let data = if o.smooth > 0 {
        box_mean(&bin, w, h, o.smooth)
            .into_iter()
            .map(|v| f32::from(u8::from(v >= 0.5)))
            .collect()
    } else {
        bin
    };
    Mask::from_vec(img.width, img.height, data).unwrap_or_else(|_| Mask::new(img.width, img.height))
}
