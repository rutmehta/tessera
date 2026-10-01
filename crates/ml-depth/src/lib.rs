//! Relative inverse depth, not metric distance.
mod cached;
mod model;
use anyhow::{Result, ensure};
pub use cached::CachedDepthEstimator;
use ml_segment::MaskRaster;
pub use ml_segment::MaskStore as DepthStore;
pub use model::{DepthEstimator, MISSING_MODEL_MESSAGE, MODEL_ID, MODEL_SHA256, MODEL_VERSION};

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
    /// Content key for independently decoded or regenerated inverse depth.
    pub fn resource_key(&self) -> [u8; 32] {
        let mut hash = blake3::Hasher::new();
        hash.update(b"tessera-normalized-inverse-depth-v1");
        hash.update(&self.width().to_le_bytes());
        hash.update(&self.height().to_le_bytes());
        for value in self.inverse_depth() {
            hash.update(&value.to_bits().to_le_bytes());
        }
        *hash.finalize().as_bytes()
    }

    /// Already normalized inverse depth; preserve absolute samples and flat maps.
    pub fn from_normalized_inverse(width: u32, height: u32, data: Vec<f32>) -> Result<Self> {
        Ok(Self(MaskRaster::new(width, height, data)?))
    }

    pub fn refined(&self, guide: &image::RgbImage) -> Result<Self> {
        Ok(Self(ml_segment::refine(&self.0, guide, 8, 0.0001)?))
    }
    pub fn store(&self, store: &DepthStore, key: &[u8; 32]) -> Result<()> {
        Ok(store.put(key, &self.0)?)
    }
    /// Persist imported depth independently of preview-cache eviction.
    pub fn store_pinned(&self, store: &DepthStore, key: &[u8; 32]) -> Result<()> {
        Ok(store.put_pinned(key, &self.0)?)
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
    /// Histogram in the recipe's near=0, far=1 convention, including both endpoints.
    pub fn histogram(&self) -> [u64; 256] {
        let mut bins = [0; 256];
        for &inverse in self.inverse_depth() {
            bins[(((1.0 - inverse) * 256.0) as usize).min(255)] += 1;
        }
        bins
    }

    /// Grayscale inverse depth: white is near, black is far.
    pub fn visualisation(&self) -> image::RgbImage {
        image::RgbImage::from_fn(self.width(), self.height(), |x, y| {
            image::Rgb(
                [(self.inverse_depth()[(y * self.width() + x) as usize] * 255.).round() as u8; 3],
            )
        })
    }

    /// Alpha-weighted 5th–95th depth percentiles select the main subject while
    /// rejecting isolated mask/depth outliers. Output uses near=0, far=1.
    pub fn focus_range_for_subject(&self, subject: &MaskRaster) -> Result<[f32; 2]> {
        ensure!(
            (self.width(), self.height()) == (subject.width(), subject.height()),
            "subject/depth dimensions differ"
        );
        let mut samples: Vec<_> = self
            .inverse_depth()
            .iter()
            .zip(subject.data())
            .filter(|(_, alpha)| **alpha > 0.)
            .map(|(&d, &a)| (1. - d, a as f64))
            .collect();
        ensure!(!samples.is_empty(), "no subject selected");
        samples.sort_by(|a, b| a.0.total_cmp(&b.0));
        let total: f64 = samples.iter().map(|s| s.1).sum();
        let percentile = |fraction: f64| {
            let mut sum = 0.;
            for &(d, a) in &samples {
                sum += a;
                if sum >= total * fraction {
                    return d;
                }
            }
            samples.last().unwrap().0
        };
        Ok([percentile(0.05), percentile(0.95)])
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

#[cfg(test)]
mod m2_49_tests {
    #[test]
    fn histogram_counts_near_to_far_including_endpoints() {
        let depth = super::DepthMap::from_prediction(4, 1, vec![0., 1., 2., 2.]).unwrap();
        let bins = depth.histogram();
        assert_eq!(bins.len(), 256);
        assert_eq!(bins.iter().sum::<u64>(), 4);
        assert_eq!(bins[0], 2);
        assert_eq!(bins[128], 1);
        assert_eq!(bins[255], 1);
    }
}
