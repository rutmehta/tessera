//! Cached on-demand depth shared by interactive rendering and export.
use crate::Renderer;
use engine_api::{EngineError, EngineResult, recipe::DevelopSettings};
use ml_depth::{CachedDepthEstimator, DepthMap};
use pipeline_cpu::Image;
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

enum Source {
    Cached(Box<CachedDepthEstimator>),
    Ready(DepthMap),
}

/// A caller-owned source. Inference is serialized and disk rasters are keyed by
/// displayed image content and the pinned model/refinement version.
pub struct DepthProvider {
    source: Mutex<Source>,
    latest: Mutex<Option<DepthMap>>,
}

fn error(e: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("depth", e.to_string())
}

impl DepthProvider {
    pub fn from_support(support: &Path) -> EngineResult<Self> {
        Ok(Self {
            source: Mutex::new(Source::Cached(Box::new(
                CachedDepthEstimator::from_support(support).map_err(error)?,
            ))),
            latest: Mutex::new(None),
        })
    }

    /// Supply camera depth or another already computed raster without inference.
    pub fn from_map(depth: DepthMap) -> Self {
        Self {
            source: Mutex::new(Source::Ready(depth.clone())),
            latest: Mutex::new(Some(depth)),
        }
    }

    pub fn estimate(&self, image: &Image) -> EngineResult<DepthMap> {
        let mut source = self.source.lock().map_err(error)?;
        // A failed request for a new image must not leave an older histogram
        // visible as if it belonged to the current render.
        *self.latest.lock().map_err(error)? = None;
        let depth = match &mut *source {
            Source::Cached(estimator) => estimator.estimate(&model_input(image)?).map_err(error)?,
            Source::Ready(depth) => {
                if (depth.width(), depth.height()) != (image.width(), image.height()) {
                    return Err(error("depth extent does not match the pre-geometry image"));
                }
                depth.clone()
            }
        };
        *self.latest.lock().map_err(error)? = Some(depth.clone());
        Ok(depth)
    }

    fn validate_model(&self, model: Option<&engine_api::id::ModelRef>) -> EngineResult<()> {
        if let Some(model) = model
            && matches!(&*self.source.lock().map_err(error)?, Source::Cached(_))
            && (model.id.as_str() != ml_depth::MODEL_ID || model.version != ml_depth::MODEL_VERSION)
        {
            return Err(error(
                "unsupported Lens Blur depth model provenance; cached inference uses the pinned Depth Anything V2 Small model",
            ));
        }
        Ok(())
    }

    /// Histogram for the latest successful estimate. Failed estimation clears it.
    pub fn histogram(&self) -> EngineResult<Vec<u64>> {
        self.latest
            .lock()
            .map_err(error)?
            .as_ref()
            .map(|d| d.histogram().to_vec())
            .ok_or_else(|| {
                error("depth has not been estimated; render Lens Blur or Visualize Depth first")
            })
    }

    pub fn subject_focus(&self, image: &Image) -> EngineResult<[f32; 2]> {
        // Subject focus also produces depth, so publish its current histogram.
        self.estimate(image)?;
        match &mut *self.source.lock().map_err(error)? {
            Source::Cached(estimator) => {
                estimator.subject_focus(&model_input(image)?).map_err(error)
            }
            Source::Ready(_) => Err(error(
                "subject segmentation requires a model-backed depth provider",
            )),
        }
    }
}

/// Convert pre-geometry scene-linear Rec.2020 to the same bounded sRGB model
/// input as the display pipeline, without ever applying geometry to the raster.
pub fn model_input(input: &Image) -> EngineResult<image::RgbImage> {
    let mut output = image::RgbImage::new(input.width(), input.height());
    for coord in input.coords() {
        let tile = pipeline_cpu::display_float(
            &input.tile(coord, 0, 1)?,
            Default::default(),
            Default::default(),
        )?;
        let e = tile.layout().extent;
        let n = e.area() as usize;
        let data = tile.samples::<f32>()?;
        let (ox, oy) = coord.pixel_origin(engine_api::tile::TILE_SIZE);
        for y in 0..e.height {
            for x in 0..e.width {
                let i = (y * e.width + x) as usize;
                output.put_pixel(
                    ox + x,
                    oy + y,
                    image::Rgb(std::array::from_fn(|c| {
                        (data[c * n + i].clamp(0., 1.) * 255.).round() as u8
                    })),
                );
            }
        }
    }
    Ok(output)
}

impl Renderer {
    pub fn with_depth(mut self, provider: Arc<DepthProvider>) -> Self {
        self.depth = Some(provider);
        self
    }
    pub fn with_depth_visualisation(mut self, enabled: bool) -> Self {
        self.depth_visualisation = enabled;
        self
    }
    /// Apply depth effects at the pre-geometry barrier. Focused pixels retain
    /// their exact input bits; a visualization is a session option, not a recipe.
    pub fn apply_depth_effects(
        &self,
        input: &Image,
        settings: &DevelopSettings,
    ) -> EngineResult<Image> {
        if settings.effects.lens_blur.is_none() && !self.depth_visualisation {
            return Ok(input.clone());
        }
        if !self.depth_visualisation
            && let Some(blur) = &settings.effects.lens_blur
            && blur.amount == 0.
        {
            // Validate the ordinary blur contract without acquiring depth or
            // weights. The operator's zero-amount branch preserves exact bits.
            return pipeline_cpu::lens_blur(
                input,
                &vec![0.; input.width() as usize * input.height() as usize],
                blur,
                Default::default(),
            );
        }
        let provider = self
            .depth
            .as_ref()
            .ok_or_else(|| error("depth provider is not installed"))?;
        provider.validate_model(
            settings
                .effects
                .lens_blur
                .as_ref()
                .and_then(|blur| blur.depth_model.as_ref()),
        )?;
        let depth = provider.estimate(input)?;
        if self.depth_visualisation {
            return Image::new(
                input.width(),
                input.height(),
                vec![depth.inverse_depth().to_vec(); 3],
            );
        }
        pipeline_cpu::lens_blur(
            input,
            &depth.near_to_far(),
            settings.effects.lens_blur.as_ref().expect("checked above"),
            Default::default(),
        )
    }
}
