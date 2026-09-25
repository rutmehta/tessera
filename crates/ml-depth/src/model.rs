use crate::{DepthMap, DepthStore, cache_key};
use anyhow::{Context, Result, ensure};
use engine_api::id::ModelRef;
use image::{RgbImage, imageops};
use ml_runtime::{ModelRegistry, PartitionReport, Session, SessionOptions, TensorInput};

pub const MODEL_VERSION: &str = "4472b7362082ad9968fee890ca0f1e5aca36b93d";
pub const MODEL_SHA256: &str = "afb6a5c28f3b6bf1618c6e43f02073ef9dfdc70e937502d51603e57b0a1df10c";
pub const MODEL_ID: &str = "depth/anything-v2-small";

pub struct DepthEstimator {
    session: Session,
    cache: DepthStore,
}
impl DepthEstimator {
    /// Explicit load may download missing weights. estimate never uses network.
    pub fn load(
        registry: &ModelRegistry,
        options: SessionOptions,
        cache: DepthStore,
    ) -> Result<Self> {
        let model = registry.resolve_ref(&ModelRef {
            id: MODEL_ID.into(),
            version: MODEL_VERSION.into(),
        })?;
        ensure!(
            model.spec().sha256 == MODEL_SHA256,
            "unexpected depth weights"
        );
        Ok(Self {
            session: Session::load(model.path(), options)?,
            cache,
        })
    }
    pub fn estimate(&mut self, image: &RgbImage) -> Result<DepthMap> {
        ensure!(image.width() > 0 && image.height() > 0, "empty image");
        let key = cache_key(image, MODEL_VERSION);
        if let Some(depth) = DepthMap::cached(&self.cache, &key)
            && (depth.width(), depth.height()) == image.dimensions()
        {
            return Ok(depth);
        }
        // Aspect-preserving bounded resize, nearest multiple of ViT patch size.
        let scale = 518. / image.width().max(image.height()) as f64;
        let size = |d: u32| ((d as f64 * scale / 14.).round().max(1.) as u32) * 14;
        let (w, h) = (size(image.width()), size(image.height()));
        let small = imageops::resize(image, w, h, imageops::FilterType::CatmullRom);
        let mut data = Vec::with_capacity(3 * w as usize * h as usize);
        for c in 0..3 {
            data.extend(small.pixels().map(|p| {
                (p[c] as f32 / 255. - [0.485, 0.456, 0.406][c]) / [0.229, 0.224, 0.225][c]
            }));
        }
        let output = self
            .session
            .run_tensors(&[(
                "pixel_values",
                TensorInput::F32 {
                    shape: vec![1, 3, h as usize, w as usize],
                    data,
                },
            )])?
            .into_iter()
            .find(|o| o.name == "predicted_depth")
            .context("missing predicted_depth")?;
        ensure!(
            output.shape == [1, h as usize, w as usize],
            "invalid depth output shape"
        );
        let depth = DepthMap::from_prediction(w, h, output.data)?.refined(image)?;
        depth.store(&self.cache, &key)?;
        Ok(depth)
    }
    /// Finalizes profiling after representative inference (cache hits do not run nodes).
    pub fn partition_report(&mut self) -> Result<PartitionReport> {
        self.session.partition_report()
    }
}
