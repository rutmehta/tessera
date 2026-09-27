//! Export adapters share the preview model policy and pre-geometry depth cache.
use engine_api::{
    EngineError, EngineResult,
    recipe::{DevelopSettings, settings::DenoiseMethod},
};
use image_core::{depth::DepthProvider, ml_depth};
use pipeline_cpu::{Image, PostDemosaicDenoise, RenderSource};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

pub(crate) fn active(source: &RenderSource<'_>, settings: &DevelopSettings) -> bool {
    settings.effects.lens_blur.is_some()
        || (matches!(source, RenderSource::Cfa { .. })
            && pipeline_cpu::denoise_active(&settings.denoise))
}

pub(crate) fn support() -> EngineResult<PathBuf> {
    std::env::var_os("TESSERA_APP_SUPPORT")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .map(|p| PathBuf::from(p).join("Library/Application Support/Tessera"))
        })
        .ok_or_else(|| {
            EngineError::invalid(
                "models",
                "set TESSERA_APP_SUPPORT to the model support directory",
            )
        })
}

fn validate_provenance(settings: &DevelopSettings) -> EngineResult<()> {
    if let Some(model) = settings
        .effects
        .lens_blur
        .as_ref()
        .and_then(|b| b.depth_model.as_ref())
        && (model.id.as_str() != ml_depth::MODEL_ID || model.version != ml_depth::MODEL_VERSION)
    {
        return Err(EngineError::invalid(
            "depth provenance",
            "Lens Blur requires the pinned Depth Anything V2 Small model",
        ));
    }
    Ok(())
}

/// Missing weights alone are recoverable. Cached rasters remain usable without
/// weights; corrupt weights, invalid models, and inference errors fail closed.
pub(crate) fn estimate(
    provider: &DepthProvider,
    input: &Image,
    warnings: &mut Vec<String>,
) -> EngineResult<Option<Vec<f32>>> {
    match provider.estimate(input) {
        Ok(depth) => Ok(Some(depth.near_to_far())),
        Err(EngineError::InvalidArgument { name, reason })
            if name == "depth" && reason == ml_depth::MISSING_MODEL_MESSAGE =>
        {
            warnings.push(format!("Lens Blur skipped: {reason}"));
            Ok(None)
        }
        Err(error) => Err(error),
    }
}

pub(crate) fn render(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    scale: u32,
    support: &Path,
    segmenter: Option<&mut dyn mask_ai::MaskSegmenter>,
    supplied_depth: Option<&DepthProvider>,
) -> EngineResult<(image::Rgb32FImage, Vec<String>)> {
    validate_provenance(settings)?;
    let mut settings = settings.clone();
    settings.output.proof_profile = None;
    let adapter = if matches!(source, RenderSource::Cfa { .. })
        && pipeline_cpu::denoise_active(&settings.denoise)
    {
        let DenoiseMethod::Neural { model, .. } = &settings.denoise.method else {
            unreachable!("active neural denoise")
        };
        let registry =
            ml_runtime::ModelRegistry::from_support(support).map_err(crate::encode_error)?;
        Some(image_core::MlCfaDenoise::automatic(
            Arc::new(registry),
            ml_runtime::SessionOptions::default(),
            model.clone(),
        ))
    } else {
        None
    };
    let denoiser = adapter.as_ref().map(|a| a as &dyn PostDemosaicDenoise);
    let owned_depth;
    let provider = if settings
        .effects
        .lens_blur
        .as_ref()
        .is_some_and(|b| b.amount > 0.)
    {
        Some(match supplied_depth {
            Some(provider) => provider,
            None => {
                owned_depth = DepthProvider::from_support(support)?;
                &owned_depth
            }
        })
    } else {
        settings.effects.lens_blur = None;
        None
    };
    let mut warnings = Vec::new();
    if crate::ai_masks::active(&settings) {
        let rgb = crate::ai_masks::render_with_hooks(
            source,
            &settings,
            segmenter,
            denoiser,
            provider,
            &mut warnings,
        )?;
        return Ok((rgb, warnings));
    }
    let plane = if let Some(provider) = provider {
        let mut pre = settings.clone();
        pre.effects = Default::default();
        let input = pipeline_cpu::render_linear_before_geometry(&pre, source, denoiser)?;
        let plane = estimate(provider, &input, &mut warnings)?;
        if plane.is_none() {
            settings.effects.lens_blur = None;
        }
        plane
    } else {
        None
    };
    let rgb = pipeline_cpu::render_linear_scaled_with_hooks(
        &settings,
        source,
        scale,
        &Default::default(),
        plane.as_deref().map(|p| (p, Default::default())),
        denoiser,
    )?;
    Ok((tone_map(rgb), warnings))
}

/// Resident CFA when the installed backend supports the recipe. Unavailable
/// Metal or unsupported recipe capability falls back before any model inference.
pub(crate) fn try_resident(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    scale: u32,
    cancel: &engine_api::jobs::CancellationToken,
) -> EngineResult<Option<image::Rgb32FImage>> {
    cancel.check()?;
    if std::env::var("TESSERA_EXPORT_BACKEND").as_deref() == Ok("cpu")
        || !matches!(source, RenderSource::Cfa { .. })
        || !pipeline_cpu::cfa_denoise_selected(&settings.denoise)
        || settings.effects.lens_blur.is_some()
        || crate::ai_masks::active(settings)
    {
        return Ok(None);
    }
    let context = match pipeline_gpu::GpuContext::new() {
        Ok(context) => Arc::new(context),
        Err(_) => return Ok(None),
    };
    let DenoiseMethod::Neural { model, .. } = &settings.denoise.method else {
        return Ok(None);
    };
    let registry =
        ml_runtime::ModelRegistry::from_support(&support()?).map_err(crate::encode_error)?;
    let adapter = Arc::new(image_core::MlCfaDenoise::automatic(
        Arc::new(registry),
        ml_runtime::SessionOptions::default(),
        model.clone(),
    ));
    render_resident_cfa(source, settings, scale, adapter, context, cancel)
        .map(|image| image.map(tone_map))
}

fn render_resident_cfa(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    scale: u32,
    adapter: Arc<dyn image_core::cfa::CfaDenoise>,
    context: Arc<pipeline_gpu::GpuContext>,
    cancel: &engine_api::jobs::CancellationToken,
) -> EngineResult<Option<Image>> {
    cancel.check()?;
    let RenderSource::Cfa { image, metadata } = source else {
        return Ok(None);
    };
    if !matches!(scale, 1 | 2 | 4 | 8) {
        return Err(EngineError::invalid("render_scale", "must be 1, 2, 4 or 8"));
    }
    let mut settings = settings.clone();
    settings.output.proof_profile = None;
    let samples = image.pyramid().pixels().to_vec();
    let raw = image_core::RawImage::new(
        engine_api::id::ImageId(1),
        Arc::new(raw_decode::CfaImage::from_linear(
            metadata.width,
            metadata.height,
            samples,
        )?),
        Arc::new((*metadata).clone()),
    )?;
    let config = image_core::RendererConfig {
        cache_budget_bytes: 128 << 20,
        threads: 1,
        ..Default::default()
    };
    let renderer = image_core::Renderer::with_ops(
        Arc::new(pipeline_gpu::GpuStageOp::with_cache_budget(
            context,
            128 << 20,
        )),
        Arc::new(image_core::TileCache::new(config.cache_budget_bytes)),
        config,
    )
    .with_cfa_denoise(adapter);
    if !renderer.can_render_resident(&raw, &settings)? {
        return Ok(None);
    }
    let frame = image_core::Renderer::output_extent(&raw, &settings, 0)?;
    let mut output = Image::new(
        frame.width,
        frame.height,
        vec![vec![0.; frame.area() as usize]; 3],
    )?;
    let mut tile_error = None;
    // Yield between bounded tile transactions just like the existing export
    // scheduler. Full-resolution inference is memoized across these tiles.
    for coord in output.coords() {
        jobs::yield_to_interactive(
            cancel,
            pipeline_gpu::EXPORT_QUIET,
            pipeline_gpu::EXPORT_MAX_YIELD,
        )?;
        renderer.render_tiles(
            &raw,
            &settings,
            &[coord],
            image_core::RenderOutput::SceneLinear,
            cancel,
            &mut |tile| {
                if let Err(error) = output.put(&tile) {
                    tile_error = Some(error);
                }
            },
        )?;
        if let Some(error) = tile_error.take() {
            return Err(error);
        }
    }
    cancel.check()?;
    Ok(Some(output.downsample_crop(
        [0, 0, frame.width, frame.height],
        scale,
    )?))
}

pub(crate) fn tone_map(rgb: Image) -> image::Rgb32FImage {
    image::Rgb32FImage::from_fn(rgb.width(), rgb.height(), |x, y| {
        let i = (y * rgb.width() + x) as usize;
        let v: [f32; 3] = std::array::from_fn(|c| rgb.planes()[c][i]);
        let y = 0.2627 * v[0] + 0.6780 * v[1] + 0.0593 * v[2];
        image::Rgb(if y <= 0. {
            [0.; 3]
        } else {
            v.map(|c| c * pipeline_cpu::sigmoid(y, Default::default()) / y)
        })
    })
}

#[cfg(test)]
#[path = "../../image-core/tests/common/cfa.rs"]
mod cfa_test;

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::recipe::settings::LensBlur;
    fn raw_fixture() -> (raw_decode::CfaImage, raw_decode::RawMetadata) {
        let cfa = raw_decode::CfaImage::from_linear(64, 64, vec![0.18; 4096]).unwrap();
        let metadata = raw_decode::RawMetadata {
            make: "test".into(),
            model: "test".into(),
            lens: None,
            iso: 100.,
            shutter_s: 0.01,
            aperture: 4.,
            focal_mm: 50.,
            capture_time: 0,
            orientation: 1,
            width: 64,
            height: 64,
            cfa_layout: raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]]),
            black_levels: [0.; 4],
            white_level: 65535,
            as_shot_wb: [1.; 4],
            camera_to_xyz: engine_api::color::ColorMatrix3::IDENTITY,
            cam_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.], [0., 0., 0.]],
            rgb_cam: [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.]],
            default_crop: [0, 0, 64, 64],
            has_gain_map: false,
            has_opcode_list: false,
            opcode_lists: [None, None, None],
        };

        (cfa, metadata)
    }

    #[test]
    fn resident_cfa_export_matches_reference_and_cancels() {
        let context = match pipeline_gpu::GpuContext::new() {
            Ok(context) => Arc::new(context),
            Err(error) => {
                eprintln!("SKIP resident CFA export: {error}");
                return;
            }
        };
        let (cfa, metadata) = raw_fixture();
        let source = RenderSource::Cfa {
            image: &cfa,
            metadata: &metadata,
        };
        let mut settings = cfa_test::settings();
        settings.geometry.upright.mode = engine_api::recipe::settings::UprightMode::Auto;
        settings.denoise.amount = 50.;
        let adapter = Arc::new(cfa_test::Inference::default());
        let cancel = engine_api::jobs::CancellationToken::new();
        let actual = render_resident_cfa(
            &source,
            &settings,
            1,
            adapter.clone(),
            context.clone(),
            &cancel,
        )
        .unwrap()
        .expect("Bayer CFA resident capability");
        let expected = pipeline_cpu::render_linear_scaled_with_denoise(
            &settings,
            &source,
            1,
            &Default::default(),
            Some(adapter.as_ref()),
        )
        .unwrap();
        assert_eq!(
            (actual.width(), actual.height()),
            (expected.width(), expected.height())
        );
        let max = actual
            .planes()
            .iter()
            .flatten()
            .zip(expected.planes().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0_f32, f32::max);
        assert!(max < 0.01, "resident CFA export error {max}");
        cancel.cancel();
        assert!(matches!(
            render_resident_cfa(&source, &settings, 1, adapter, context, &cancel),
            Err(EngineError::Cancelled)
        ));
    }

    #[test]
    fn raw_neural_export_installs_automatic_adapter_without_noise_configuration() {
        let support = tempfile::tempdir().unwrap();
        let registry = ml_runtime::ModelRegistry::from_support(support.path()).unwrap();
        let model = registry
            .models()
            .iter()
            .find(|s| s.id == "enhance/cfa-unet-fp32")
            .unwrap();
        let mut settings = engine_api::recipe::DevelopSettings::default();
        settings.denoise.method = engine_api::recipe::settings::DenoiseMethod::Neural {
            model: engine_api::id::ModelRef {
                id: model.id.as_str().into(),
                version: model.version.clone(),
            },
            joint_demosaic: false,
        };
        settings.denoise.amount = 50.;
        let (cfa, metadata) = raw_fixture();
        let error = render(
            &pipeline_cpu::RenderSource::Cfa {
                image: &cfa,
                metadata: &metadata,
            },
            &settings,
            1,
            support.path(),
            None,
            None,
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("downloads are disabled"),
            "{error}"
        );
    }

    #[test]
    fn supplied_depth_blurs_before_geometry() {
        let support = tempfile::tempdir().unwrap();
        let values: Vec<f32> = (0..32 * 24)
            .map(|i| {
                if (i % 32 + i / 32) % 2 == 0 {
                    0.05
                } else {
                    0.8
                }
            })
            .collect();
        let image = pipeline_cpu::Image::new(32, 24, vec![values; 3]).unwrap();
        let source = pipeline_cpu::RenderSource::Rgb(&image);
        let provider = image_core::depth::DepthProvider::from_map(
            image_core::ml_depth::DepthMap::from_prediction(32, 24, vec![0.; 32 * 24]).unwrap(),
        );
        let mut settings = engine_api::recipe::DevelopSettings::default();
        settings.effects.lens_blur = Some(LensBlur::default());
        settings.geometry.crop.rect.right = 0.75;
        let (actual, warnings) =
            render(&source, &settings, 1, support.path(), None, Some(&provider)).unwrap();
        let expected = pipeline_cpu::render_linear_scaled_with_depth(
            &settings,
            &source,
            1,
            &Default::default(),
            &vec![1.; 32 * 24],
            Default::default(),
        )
        .unwrap();
        assert_eq!(actual, tone_map(expected));
        assert!(warnings.is_empty());
        // Disk depth can be reused even after model weights are removed.
        let mut pre = settings.clone();
        pre.effects = Default::default();
        let input = pipeline_cpu::render_linear_before_geometry(&pre, &source, None).unwrap();
        let model_input = image_core::depth::model_input(&input).unwrap();
        let store =
            ml_depth::DepthStore::new(support.path().join("previews/depth-cache"), 256 << 20)
                .unwrap();
        let map = ml_depth::DepthMap::from_prediction(32, 24, vec![0.; 32 * 24]).unwrap();
        map.store(
            &store,
            &ml_depth::cache_key(&model_input, ml_depth::MODEL_VERSION),
        )
        .unwrap();
        let (cached, warnings) = render(&source, &settings, 1, support.path(), None, None).unwrap();
        assert_eq!(actual, cached);
        assert!(warnings.is_empty());
    }
    #[test]
    fn ai_masks_and_depth_both_run_in_one_export() {
        use engine_api::recipe::mask::{LocalAdjustment, LocalParams, MaskComponent, MaskKind};
        struct Subject(usize);
        impl mask_ai::MaskSegmenter for Subject {
            fn segment(
                &mut self,
                image: &image::RgbImage,
                _: &mask_ai::SegmentRequest,
            ) -> anyhow::Result<Vec<f32>> {
                self.0 += 1;
                Ok(vec![1.; (image.width() * image.height()) as usize])
            }
        }
        let support = tempfile::tempdir().unwrap();
        let image = Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let source = RenderSource::Rgb(&image);
        let mut settings = DevelopSettings::default();
        settings.effects.lens_blur = Some(LensBlur::default());
        settings.locals.adjustments.push(LocalAdjustment {
            components: vec![MaskComponent::new(MaskKind::Subject { model: None })],
            params: LocalParams {
                exposure: 1.,
                ..Default::default()
            },
            ..Default::default()
        });
        let provider = DepthProvider::from_map(
            ml_depth::DepthMap::from_prediction(8, 6, vec![0.; 48]).unwrap(),
        );
        let mut segmenter = Subject(0);
        let (with_mask, warnings) = render(
            &source,
            &settings,
            1,
            support.path(),
            Some(&mut segmenter),
            Some(&provider),
        )
        .unwrap();
        settings.locals = Default::default();
        let (without_mask, _) =
            render(&source, &settings, 1, support.path(), None, Some(&provider)).unwrap();
        assert_eq!(segmenter.0, 1);
        assert!(warnings.is_empty());
        assert_ne!(with_mask, without_mask);
    }

    #[test]
    fn missing_weights_skip_only_blur_with_warning() {
        let support = tempfile::tempdir().unwrap();
        let image = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let source = pipeline_cpu::RenderSource::Rgb(&image);
        let mut settings = engine_api::recipe::DevelopSettings::default();
        settings.effects.lens_blur = Some(LensBlur::default());
        let before = settings.clone();
        let (rendered, warnings) =
            render(&source, &settings, 1, support.path(), None, None).unwrap();
        assert_eq!(rendered.dimensions(), (8, 6));
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("not cached"));
        assert_eq!(settings, before);
    }
    #[test]
    fn corrupt_depth_weights_fail_instead_of_skipping() {
        let support = tempfile::tempdir().unwrap();
        let registry = ml_runtime::ModelRegistry::from_support(support.path()).unwrap();
        let spec = registry
            .models()
            .iter()
            .find(|s| s.id == image_core::ml_depth::MODEL_ID)
            .unwrap();
        std::fs::write(
            support
                .path()
                .join("models/cache")
                .join(format!("{}.onnx", spec.sha256)),
            b"bad",
        )
        .unwrap();
        let image = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let mut settings = engine_api::recipe::DevelopSettings::default();
        settings.effects.lens_blur = Some(LensBlur::default());
        let error = render(
            &pipeline_cpu::RenderSource::Rgb(&image),
            &settings,
            1,
            support.path(),
            None,
            None,
        )
        .unwrap_err();
        assert!(error.to_string().contains("SHA-256"));
    }
    #[test]
    fn unknown_depth_provenance_is_not_silently_replaced() {
        let support = tempfile::tempdir().unwrap();
        let image = pipeline_cpu::Image::new(8, 6, vec![vec![0.18; 48]; 3]).unwrap();
        let mut settings = engine_api::recipe::DevelopSettings::default();
        settings.effects.lens_blur = Some(LensBlur {
            depth_model: Some(engine_api::id::ModelRef {
                id: "other/model".into(),
                version: "1".into(),
            }),
            ..Default::default()
        });
        assert!(
            render(
                &pipeline_cpu::RenderSource::Rgb(&image),
                &settings,
                1,
                support.path(),
                None,
                None
            )
            .unwrap_err()
            .to_string()
            .contains("provenance")
        );
    }
}
