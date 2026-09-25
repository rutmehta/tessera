//! Relative inverse depth, not metric distance.
mod model;
use anyhow::{Result, ensure};
use ml_segment::MaskRaster;
pub use ml_segment::MaskStore as DepthStore;
pub use model::{DepthEstimator, MODEL_ID, MODEL_SHA256, MODEL_VERSION};

/// Normalized relative inverse depth: 1 is near, 0 is far.
#[derive(Clone, Debug, PartialEq)]
pub struct DepthMap(MaskRaster);
/// Content, dimensions, pinned model and preprocessing/refinement revision.
pub fn cache_key(image: &image::RgbImage, version: &str) -> [u8; 32] {
    let mut hash = blake3::Hasher::new();
    hash.update(b"depth-anything-v2-small-rgb-v1-guided8-e1e-4");
    hash.update(&ml_segment::image_hash(image));
    hash.update(version.as_bytes());
    *hash.finalize().as_bytes()
}
impl DepthMap {
    pub fn refined(&self, guide: &image::RgbImage) -> Result<Self> {
        Ok(Self(ml_segment::refine(&self.0, guide, 8, 0.0001)?))
    }
    pub fn store(&self, store: &DepthStore, key: &[u8; 32]) -> Result<()> {
        Ok(store.put(key, &self.0)?)
    }
    pub fn cached(store: &DepthStore, key: &[u8; 32]) -> Option<Self> {
        store.get(key).map(Self)
    }
    pub fn from_prediction(width: u32, height: u32, data: Vec<f32>) -> Result<Self> {
        ensure!(
            width > 0
                && height > 0
                && (width as usize).checked_mul(height as usize) == Some(data.len()),
            "invalid depth dimensions"
        );
        ensure!(data.iter().all(|v| v.is_finite()), "nonfinite depth");
        let lo = data.iter().copied().fold(f32::INFINITY, f32::min) as f64;
        let hi = data.iter().copied().fold(f32::NEG_INFINITY, f32::max) as f64;
        let data = data
            .into_iter()
            .map(|v| {
                if hi > lo {
                    ((v as f64 - lo) / (hi - lo)) as f32
                } else {
                    0.
                }
            })
            .collect();
        Ok(Self(MaskRaster::new(width, height, data)?))
    }
    pub fn width(&self) -> u32 {
        self.0.width()
    }
    pub fn height(&self) -> u32 {
        self.0.height()
    }
    pub fn inverse_depth(&self) -> &[f32] {
        self.0.data()
    }
    /// Supply this converted plane to pipeline_cpu::masks::MaskOptions::depth
    /// and lens_blur; those APIs use near=0, far=1, unlike the network.
    pub fn near_to_far(&self) -> Vec<f32> {
        self.0.data().iter().map(|v| 1. - v).collect()
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn normalization_and_direction() {
        let d = super::DepthMap::from_prediction(3, 1, vec![2., 4., 6.]).unwrap();
        assert_eq!(d.inverse_depth(), &[0., 0.5, 1.]);
        assert_eq!(d.near_to_far(), vec![1., 0.5, 0.]);
        assert!(super::DepthMap::from_prediction(1, 1, vec![f32::NAN]).is_err());
        assert!(super::DepthMap::from_prediction(2, 1, vec![1.]).is_err());
        assert_eq!(
            super::DepthMap::from_prediction(2, 1, vec![3.; 2])
                .unwrap()
                .inverse_depth(),
            &[0.; 2]
        );
    }
}
