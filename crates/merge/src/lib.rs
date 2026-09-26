//! Scene-linear camera RGB photo merging. No white balance or tone is baked in.
pub mod alignment;
pub(crate) mod blend;
pub mod dng;
pub(crate) mod features;
pub mod hdr;
mod layer_cut;
mod layer_lens;
pub mod layers;
pub mod pano;
mod raw;
mod recipe;
pub use raw::from_cfa;
pub use recipe::{auto_recipe, recipe_xmp, write_dng};

pub type Result<T> = std::result::Result<T, String>;

#[derive(Debug)]
pub struct HdrPanoramaResult {
    pub panorama: pano::PanoramaResult,
    /// Each mask stays in its bracket reference coordinates.
    pub bracket_masks: Vec<Vec<bool>>,
}

/// Merge each bracket, normalize all radiances to the first reference exposure,
/// then stitch. Bracket grouping/order is explicit, never inferred from filenames.
pub fn hdr_panorama(
    groups: &[Vec<hdr::BracketFrame>],
    hdr_options: &hdr::HdrOptions,
    pano_options: &pano::PanoramaOptions,
) -> Result<HdrPanoramaResult> {
    if groups.is_empty() || groups.len() > 128 {
        return Err("HDR panorama requires 1..128 brackets".into());
    }
    let first = groups[0]
        .get(hdr_options.reference)
        .ok_or("invalid bracket reference")?
        .exposure
        .value()?;
    let mut images = Vec::new();
    let mut masks = Vec::new();
    for group in groups {
        let mut merged = hdr::hdr(group, hdr_options)?;
        let scale = first / group[hdr_options.reference].exposure.value()?;
        if !(1e-6..=1e6).contains(&scale) {
            return Err("bracket reference ratio outside supported range".into());
        }
        for pixel in &mut merged.image.pixels {
            for v in pixel {
                *v = (*v as f64 * scale) as f32;
            }
        }
        images.push(merged.image);
        masks.push(merged.deghost_mask);
    }
    Ok(HdrPanoramaResult {
        panorama: pano::panorama(&images, pano_options)?,
        bracket_masks: masks,
    })
}

/// Unbalanced, black-subtracted camera RGB; sensor clipping is at 1 for inputs.
#[derive(Clone, Debug)]
pub struct LinearImage {
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<[f32; 3]>,
    /// DNG ColorMatrix1: XYZ (D65) to camera, NOT camera-to-XYZ.
    pub color_matrix: [[f64; 3]; 3],
    pub as_shot_neutral: [f64; 3],
}

impl LinearImage {
    pub fn validate(&self) -> Result<()> {
        let n = self
            .width
            .checked_mul(self.height)
            .ok_or("image overflow")?;
        if n == 0
            || n > 64 * 1024 * 1024
            || n != self.pixels.len()
            || self.pixels.iter().flatten().any(|v| !v.is_finite())
            || self.color_matrix.iter().flatten().any(|v| !v.is_finite())
            || self
                .as_shot_neutral
                .iter()
                .any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err("invalid linear camera image".into());
        }
        Ok(())
    }
    /// Bilinear camera sample, None outside the image.
    pub fn sample(&self, x: f64, y: f64) -> Option<[f32; 3]> {
        if !x.is_finite()
            || !y.is_finite()
            || x < 0.
            || y < 0.
            || self.width == 0
            || self.height == 0
            || x > (self.width - 1) as f64
            || y > (self.height - 1) as f64
        {
            return None;
        }
        let (ix, iy) = (x as usize, y as usize);
        let (jx, jy) = ((ix + 1).min(self.width - 1), (iy + 1).min(self.height - 1));
        let (fx, fy) = ((x - ix as f64) as f32, (y - iy as f64) as f32);
        let a = self.pixels.get(iy * self.width + ix)?;
        let b = self.pixels.get(iy * self.width + jx)?;
        let c = self.pixels.get(jy * self.width + ix)?;
        let d = self.pixels.get(jy * self.width + jx)?;
        Some(std::array::from_fn(|k| {
            (a[k] * (1. - fx) + b[k] * fx) * (1. - fy) + (c[k] * (1. - fx) + d[k] * fx) * fy
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checked_image_and_bilinear_sample() {
        let mut image = LinearImage {
            width: 2,
            height: 2,
            pixels: vec![[0.; 3], [1.; 3], [2.; 3], [3.; 3]],
            color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            as_shot_neutral: [1.; 3],
        };
        assert!(image.validate().is_ok());
        assert_eq!(image.sample(0.5, 0.5), Some([1.5; 3]));
        assert_eq!(image.sample(-1., 0.), None);
        image.pixels[0][0] = f32::NAN;
        assert!(image.validate().is_err());
    }
}
