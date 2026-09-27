//! Destructive-to-a-new-DNG enhancement; the source and its calibration survive.
use crate::merge::{PhotoJob, PhotoJobListener, PhotoPublication, load_linear};
use crate::{Engine, Result, failure};
use engine_api::{id::ModelRef, recipe::Recipe};
use ml_enhance::{CameraSrgb, Denoiser, SuperResolution};
use ml_runtime::{ModelRegistry, SessionOptions, Tensor};
use std::{path::PathBuf, sync::Arc};

#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct EnhanceOptions {
    pub denoise_amount: Option<u8>,
    pub super_resolution: bool,
    /// Reserved for compatibility. True is rejected: no supported Apache/MIT
    /// learned-demosaic model is available; Raw Details is out of scope.
    pub raw_details: bool,
    /// Explicit consent; cache-only by default.
    pub allow_model_download: bool,
}
impl EnhanceOptions {
    pub fn validate(&self) -> Result<()> {
        if self.raw_details {
            return Err(failure(
                "Raw Details is unavailable: no learned Raw Details model is installed or supported",
            ));
        }
        if self.denoise_amount.is_some_and(|v| v > 100) {
            return Err(failure("denoise amount must be 0..=100"));
        }
        if self.denoise_amount.is_none() && !self.super_resolution {
            return Err(failure("select denoise or super resolution"));
        }
        Ok(())
    }
}

#[uniffi::export]
impl Engine {
    /// Each selected photo produces its own float LinearRaw DNG, stacked with
    /// that source. SR doubles each dimension. Zero NR is a bit-exact bypass.
    pub fn enhance(
        self: Arc<Self>,
        image_ids: Vec<String>,
        options: EnhanceOptions,
        listener: Arc<dyn PhotoJobListener>,
    ) -> Result<Arc<PhotoJob>> {
        options.validate()?;
        let sources = self.photo_sources(&image_ids)?;
        PhotoJob::spawn(listener, move |job| {
            let mut outputs = Vec::new();
            let mut denoiser = None;
            let mut upscaler = None;
            let total = sources.len() as u32;
            for (i, source) in sources.iter().enumerate() {
                job.check()?;
                job.progress("decode", i as u32, total);
                let (mut image, _) = load_linear(source)?;
                let recipe: Recipe =
                    serde_json::from_str(&self.get_recipe(source.id.clone())?).map_err(failure)?;
                if options.denoise_amount.unwrap_or(0) > 0 || options.super_resolution {
                    image.validate().map_err(failure)?;
                    if options.super_resolution && image.pixels.len() > 16 * 1024 * 1024 {
                        return Err(failure("super resolution output exceeds 64 Mi pixels"));
                    }
                    let adapter = CameraSrgb::new(image.color_matrix, image.as_shot_neutral)
                        .map_err(failure)?;
                    let data = (0..3)
                        .flat_map(|c| image.pixels.iter().map(move |p| p[c]))
                        .collect();
                    let camera =
                        Tensor::new(3, image.height, image.width, data).map_err(failure)?;
                    // Validate gamut/HDR before any model download or load.
                    let mut rgb = adapter.to_srgb(&camera).map_err(failure)?;
                    job.check()?;
                    if (options.denoise_amount.unwrap_or(0) > 0 && denoiser.is_none())
                        || (options.super_resolution && upscaler.is_none())
                    {
                        let registry = self.enhance_registry()?;
                        if options.denoise_amount.unwrap_or(0) > 0 && denoiser.is_none() {
                            prepare_model(
                                &registry,
                                ml_enhance::DENOISE_MODEL_ID,
                                ml_enhance::DENOISE_VERSION,
                                options.allow_model_download,
                                job,
                            )?;
                            denoiser = Some(
                                Denoiser::load_cached(&registry, SessionOptions::default())
                                    .map_err(failure)?,
                            );
                            job.progress("model-ready:denoise", 1, 1);
                        }
                        if options.super_resolution && upscaler.is_none() {
                            prepare_model(
                                &registry,
                                "enhance/realesrgan-x2",
                                ml_enhance::SR_VERSION,
                                options.allow_model_download,
                                job,
                            )?;
                            upscaler = Some(
                                SuperResolution::load_cached(
                                    &registry,
                                    2,
                                    SessionOptions::default(),
                                )
                                .map_err(failure)?,
                            );
                            job.progress("model-ready:super-resolution", 1, 1);
                        }
                    }
                    job.check()?;
                    if let Some(model) = &mut denoiser {
                        job.progress("denoise", i as u32, total);
                        rgb = model
                            .denoise(&rgb, f32::from(options.denoise_amount.unwrap_or(0)), None)
                            .map_err(failure)?;
                    }
                    job.check()?;
                    if let Some(model) = &mut upscaler {
                        job.progress("super-resolution", i as u32, total);
                        let [_, _, h, w] = rgb.shape();
                        let display = Tensor::new(
                            3,
                            h,
                            w,
                            rgb.data().iter().map(|&v| encode_srgb(v)).collect(),
                        )
                        .map_err(failure)?;
                        let result = model.super_resolution(&display, 2).map_err(failure)?;
                        let [_, _, h, w] = result.shape();
                        // Preserve finite model overshoot with the extended sRGB
                        // transfer, rather than clipping generated highlights.
                        rgb = Tensor::new(
                            3,
                            h,
                            w,
                            result.data().iter().map(|&v| decode_srgb(v)).collect(),
                        )
                        .map_err(failure)?;
                    }
                    job.check()?;
                    let restored = adapter.to_camera(&rgb).map_err(failure)?;
                    let [_, _, h, w] = restored.shape();
                    image.width = w;
                    image.height = h;
                    image.pixels = (0..h * w)
                        .map(|i| std::array::from_fn(|c| restored.data()[c * h * w + i]))
                        .collect();
                    image.validate().map_err(failure)?;
                }
                job.check()?;
                let suffix = match (options.denoise_amount.is_some(), options.super_resolution) {
                    (true, true) => "-Enhanced-NR-SR",
                    (true, false) => "-Enhanced-NR",
                    _ => "-Enhanced-SR",
                };
                outputs.push(self.publish_photo(
                    source,
                    &image,
                    &recipe,
                    PhotoPublication {
                        suffix,
                        source_ids: std::slice::from_ref(&source.id),
                        create_stack: true,
                    },
                    job,
                )?);
                job.progress("enhance", i as u32 + 1, total);
            }
            Ok(outputs)
        })
    }
}

impl Engine {
    fn enhance_registry(&self) -> Result<ModelRegistry> {
        let dir = self.support_dir()?.join("models");
        std::fs::create_dir_all(&dir)?;
        // Private immutable manifest avoids concurrent writes with export jobs.
        let mut manifest = tempfile::NamedTempFile::new_in(&dir)?;
        std::io::Write::write_all(
            &mut manifest,
            include_bytes!("../../ml-runtime/models.toml"),
        )?;
        let cache = std::env::var_os("TESSERA_ENHANCE_MODEL_CACHE")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.join("cache"));
        ModelRegistry::open(manifest.path(), cache).map_err(failure)
    }
}
fn prepare_model(
    registry: &ModelRegistry,
    id: &str,
    version: &str,
    allow_download: bool,
    job: &PhotoJob,
) -> Result<()> {
    job.check()?;
    let model = ModelRef {
        id: id.into(),
        version: version.into(),
    };
    if registry
        .resolve_cached_ref(&model)
        .map_err(failure)?
        .is_none()
    {
        if !allow_download {
            return Err(failure(format!(
                "missing enhancement weights {id}@{version} (offline); set allow_model_download=true to download"
            )));
        }
        // No invented byte percentage: only start and fully verified ready.
        job.progress(&format!("model-download:{id}"), 0, 1);
        registry
            .resolve_ref(&model)
            .map_err(|e| failure(format!("model download {id}: {e}")))?;
    }
    // All actual session loading remains cache-only even if the file vanished
    // after resolution. Offline mode can never enter resolve_ref indirectly.
    job.check()
}
fn encode_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}
fn decode_srgb(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
