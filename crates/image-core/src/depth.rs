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
    store: Option<ml_depth::DepthStore>,
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
            store: Some(
                ml_depth::DepthStore::new(support.join("previews/depth-cache"), 256 << 20)
                    .map_err(error)?,
            ),
        })
    }

    /// Supply camera depth or another already computed raster without inference.
    pub fn from_map(depth: DepthMap) -> Self {
        Self {
            source: Mutex::new(Source::Ready(depth.clone())),
            latest: Mutex::new(Some(depth)),
            store: None,
        }
    }

    /// Use this host-owned mask-store for persisted imported depth references.
    /// The supplied source remains the fallback for missing/evicted resources.
    pub fn with_store(mut self, store: ml_depth::DepthStore) -> Self {
        self.store = Some(store);
        self
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
        // Rendering only reads imported resources; estimates never alter the recipe.
        let imported = settings
            .effects
            .lens_blur
            .as_ref()
            .and_then(|blur| blur.depth.as_ref())
            .and_then(|depth| depth.mask_key)
            .and_then(|key| {
                provider
                    .store
                    .as_ref()
                    .and_then(|store| DepthMap::cached(store, &key))
            })
            .filter(|depth| {
                let full = engine_api::tile::Extent::new(depth.width(), depth.height());
                let wanted = engine_api::tile::Extent::new(input.width(), input.height());
                (0..32).any(|level| full.at_level(level) == wanted)
            });
        let depth = match imported {
            Some(depth) if (depth.width(), depth.height()) == (input.width(), input.height()) => {
                depth
            }
            Some(depth) => {
                // Imported rasters are full-size; preview levels sample the same
                // pre-geometry coordinate system without creating another file.
                let raster = image::ImageBuffer::<image::Luma<f32>, _>::from_raw(
                    depth.width(),
                    depth.height(),
                    depth.inverse_depth().to_vec(),
                )
                .expect("validated depth extent");
                let resized = image::imageops::resize(
                    &raster,
                    input.width(),
                    input.height(),
                    image::imageops::FilterType::Triangle,
                );
                DepthMap::from_normalized_inverse(
                    input.width(),
                    input.height(),
                    resized
                        .into_raw()
                        .into_iter()
                        .map(|v| v.clamp(0., 1.))
                        .collect(),
                )
                .map_err(error)?
            }
            None => {
                provider.validate_model(
                    settings
                        .effects
                        .lens_blur
                        .as_ref()
                        .and_then(|b| b.depth_model.as_ref()),
                )?;
                provider.estimate(input)?
            }
        };
        *provider.latest.lock().map_err(error)? = Some(depth.clone());
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

/// One durable depth slot per image. Reimport replaces the slot rather than
/// accumulating content-addressed pins. The host removes it with the image record.
pub fn imported_depth_key(id: engine_api::id::ImageId) -> [u8; 32] {
    engine_api::id::Digest::derive("tessera imported lens depth image v1", &id.0.to_le_bytes()).0
}

/// Import-apply only: resolve opaque IDs through the existing caller-owned
/// association and store an independently decodable grayscale PNG/TIFF raster.
/// No inference, path interpretation, history entry, or diagnostic is performed.
/// Missing/invalid resources leave regeneration pending and remove a prior pin.
pub fn import_lens_blur_depth(
    recipe: &mut engine_api::recipe::Recipe,
    extent: (u32, u32),
    store: &ml_depth::DepthStore,
    mut resolve: impl FnMut(&str) -> Option<Vec<u8>>,
) -> EngineResult<Option<DepthMap>> {
    let id = recipe
        .image_id
        .ok_or_else(|| error("depth import requires image identity"))?;
    let key = imported_depth_key(id);
    let Some(blur) = recipe.settings.effects.lens_blur.as_ref() else {
        store.remove_pinned(&key).map_err(error)?;
        return Ok(None);
    };
    if !recipe
        .history
        .head
        .and_then(|head| recipe.history.entry(head))
        .is_some_and(|entry| matches!(entry.meta.author, engine_api::recipe::Author::Import { .. }))
    {
        return Err(error(
            "depth resources must be attached during import apply",
        ));
    }
    let mut state = blur.depth.clone().unwrap_or_default();
    let mut imported = None;
    for id in [&state.base_layered_depth_table, &state.base_raw_depth_table]
        .into_iter()
        .flatten()
    {
        let Some(bytes) = resolve(id) else { continue };
        if bytes.len() as u64 > ml_depth::DepthStore::MAX_PINNED_BYTES {
            continue;
        }
        let Ok(format) = image::guess_format(&bytes) else {
            continue;
        };
        if !matches!(format, image::ImageFormat::Png | image::ImageFormat::Tiff) {
            continue;
        }
        // Inspect dimensions before decoding, bounding both stored and decoded rasters.
        let reader = image::ImageReader::with_format(std::io::Cursor::new(&bytes), format);
        let Ok((width, height)) = reader.into_dimensions() else {
            continue;
        };
        if (width, height) != extent
            || u64::from(width)
                .saturating_mul(u64::from(height))
                .saturating_mul(4)
                .saturating_add(48)
                > ml_depth::DepthStore::MAX_PINNED_BYTES
        {
            continue;
        }
        let Ok(decoded) = image::load_from_memory_with_format(&bytes, format) else {
            continue;
        };
        if !matches!(
            decoded.color(),
            image::ColorType::L8 | image::ColorType::L16
        ) {
            continue;
        }
        imported = Some(
            DepthMap::from_normalized_inverse(width, height, decoded.to_luma32f().into_raw())
                .map_err(error)?,
        );
        break;
    }
    state.mask_key = imported.as_ref().map(|_| key);
    state.regenerate = imported.is_none();
    // Verify history and all recipe invariants before changing any owned resource.
    let mut next = recipe.clone();
    next.set_lens_blur_depth(state)?;
    next.validate()?;
    if let Some(depth) = &imported {
        depth.store_pinned(store, &key).map_err(error)?;
    } else {
        store.remove_pinned(&key).map_err(error)?;
    }
    *recipe = next;
    Ok(imported)
}
