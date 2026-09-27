use crate::{DepthEstimator, DepthMap, DepthStore, MODEL_VERSION, cache_key};
use anyhow::{Context, Result, ensure};
use image::RgbImage;
use ml_runtime::{ModelRegistry, SessionOptions};
use std::path::{Path, PathBuf};

/// Shared interactive/export adapter. Source pixels, dimensions, pinned model
/// and preprocessing revision determine the cache key; focus/blur settings do
/// not change inference. No render or focus call downloads model weights.
pub struct CachedDepthEstimator {
    registry: ModelRegistry,
    options: SessionOptions,
    cache: Option<DepthStore>,
    estimator: Option<DepthEstimator>,
    subject_cache: Option<PathBuf>,
    segmenter: Option<ml_segment::Segmenter>,
}
impl CachedDepthEstimator {
    pub fn new(registry: ModelRegistry, options: SessionOptions, cache: DepthStore) -> Self {
        Self {
            registry: registry.with_downloads_allowed(false),
            options,
            cache: Some(cache),
            estimator: None,
            subject_cache: None,
            segmenter: None,
        }
    }
    pub fn from_support(support: &Path) -> Result<Self> {
        // Publish the shared manifest atomically, including when weights live
        // outside the standard support directory.
        let registry = ModelRegistry::from_support(support)?;
        let registry = if let Some(weights) = std::env::var_os("TESSERA_DEPTH_MODELS") {
            ModelRegistry::open(support.join("models/models.toml"), PathBuf::from(weights))?
        } else {
            registry
        };
        let cache = DepthStore::new(support.join("previews/depth-cache"), 256 << 20)?;
        let mut result = Self::new(registry, SessionOptions::default(), cache);
        result.subject_cache = Some(support.join("mask-cache"));
        Ok(result)
    }
    pub fn estimate(&mut self, image: &RgbImage) -> Result<DepthMap> {
        ensure!(image.width() > 0 && image.height() > 0, "empty image");
        if let Some(cache) = &self.cache
            && let Some(depth) = DepthMap::cached(cache, &cache_key(image, MODEL_VERSION))
            && (depth.width(), depth.height()) == image.dimensions()
        {
            return Ok(depth);
        }
        if self.estimator.is_none() {
            self.estimator = Some(DepthEstimator::load_cached_retaining_store(
                &self.registry,
                self.options.clone(),
                &mut self.cache,
            )?);
        }
        self.estimator.as_mut().unwrap().estimate(image)
    }
    /// Uses ml-segment's subject raster and robust depth percentiles. Existing
    /// mask cache entries require no weights. Fresh inference currently needs
    /// cached U2Net and SAM models because Segmenter loads all three sessions.
    pub fn subject_focus(&mut self, image: &RgbImage) -> Result<[f32; 2]> {
        let depth = self.estimate(image)?;
        let path = self
            .subject_cache
            .as_ref()
            .context("subject focus requires from_support")?;
        let store = ml_segment::MaskStore::new(path, 256 << 20)?;
        let key = ml_segment::cache_key(
            image,
            "subject",
            ml_segment::SUBJECT_VERSION,
            &Default::default(),
            0,
        )?;
        if let Some(subject) = store.get(&key) {
            return depth.focus_range_for_subject(&subject);
        }
        if self.segmenter.is_none() {
            for (id, version) in [
                ("segment/u2net", ml_segment::SUBJECT_VERSION),
                ("segment/sam-encoder", ml_segment::SAM_VERSION),
                ("segment/sam-decoder", ml_segment::SAM_VERSION),
            ] {
                self.registry.resolve_cached_ref(&engine_api::id::ModelRef { id: id.into(), version: version.into() })?
                    .with_context(|| format!("Subject focus model is not cached: {id}; download subject models in Models"))?;
            }
            // Registry policy also prevents downloads if a verified cache entry
            // disappears between the check above and Segmenter loading it.
            self.segmenter = Some(ml_segment::Segmenter::load(
                &self.registry,
                self.options.clone(),
                store,
            )?);
        }
        let subject = self.segmenter.as_mut().unwrap().subject(image, 0)?;
        depth.focus_range_for_subject(&subject)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn from_support_preserves_weights_override() -> Result<()> {
        const CHILD: &str = "TESSERA_TEST_DEPTH_OVERRIDE_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let support = tempfile::tempdir()?;
            let estimator = CachedDepthEstimator::from_support(support.path())?;
            let spec = estimator
                .registry
                .models()
                .iter()
                .find(|m| m.id == "depth/anything-v2-small")
                .unwrap();
            let model = engine_api::id::ModelRef {
                id: spec.id.as_str().into(),
                version: spec.version.clone(),
            };
            // A corrupt file in the override directory must be observed rather
            // than silently looking in the default (empty) weights directory.
            std::fs::write(
                PathBuf::from(std::env::var_os("TESSERA_DEPTH_MODELS").unwrap())
                    .join(format!("{}.onnx", spec.sha256)),
                b"corrupt",
            )?;
            assert!(estimator.registry.resolve_cached_ref(&model).is_err());
        } else {
            // Isolate environment mutation from parallel Rust test threads.
            let weights = tempfile::tempdir()?;
            let status = std::process::Command::new(std::env::current_exe()?)
                .args([
                    "--exact",
                    "cached::tests::from_support_preserves_weights_override",
                ])
                .env(CHILD, "1")
                .env("TESSERA_DEPTH_MODELS", weights.path())
                .status()?;
            assert!(status.success());
        }
        Ok(())
    }

    #[test]
    fn from_support_replaces_manifest_without_truncating_existing_readers() -> Result<()> {
        let support = tempfile::tempdir()?;
        let models = support.path().join("models");
        std::fs::create_dir_all(&models)?;
        let manifest = models.join("models.toml");
        let previous = "# previous manifest snapshot\n";
        std::fs::write(&manifest, previous)?;
        let mut reader = std::fs::File::open(&manifest)?;
        let _estimator = CachedDepthEstimator::from_support(support.path())?;
        let mut snapshot = String::new();
        reader.read_to_string(&mut snapshot)?;
        assert_eq!(
            snapshot, previous,
            "existing readers must retain their snapshot"
        );
        assert_eq!(
            std::fs::read_to_string(manifest)?,
            include_str!("../../ml-runtime/models.toml")
        );
        Ok(())
    }
}
