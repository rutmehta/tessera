//! Per-export AI rasters. No process-global hooks, cache or test backend.
use engine_api::{
    EngineError, EngineResult,
    recipe::{
        DevelopSettings,
        mask::{LocalAdjustment, MaskComponent},
    },
    stage::{ParamHash, StageId},
};
use image_core::{MaskRasterCache, mask_cache::MaskHooks};
use mask_ai::{AlphaPlane, MaskSegmenter};
use pipeline_cpu::{Image, RenderSource};
use std::sync::Arc;

pub(crate) fn active(settings: &DevelopSettings) -> bool {
    settings.locals.adjustments.iter().any(|g| {
        g.enabled
            && g.amount != 0.0
            && g.components
                .iter()
                .flat_map(|c| c.active_leaves())
                .any(|c| c.kind.is_ai())
    })
}

struct ReadyMasks(Vec<(MaskComponent, AlphaPlane)>);
impl MaskHooks for ReadyMasks {
    fn revision(&self) -> u64 {
        0
    }
    fn rasterize(
        &self,
        input: &Image,
        group: &LocalAdjustment,
        _level: u8,
    ) -> EngineResult<Vec<f32>> {
        mask_ai::compose_with_components(input, group, |component, w, h| {
            let (_, plane) = self
                .0
                .iter()
                .find(|(c, _)| c == component)
                .ok_or_else(|| EngineError::invalid("mask", "missing export AI raster"))?;
            Ok(mask_ai::resample(plane, w, h).into())
        })
    }
}

fn error(e: impl std::fmt::Display) -> EngineError {
    EngineError::invalid("AI mask export", e.to_string())
}

/// Render the pre-local barrier with the reference pipeline, install the same
/// compositor used by FFI in a private mask cache, then finish the public CPU
/// effects/geometry operators. This never masks display-encoded pixels.
pub(crate) fn render(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    segmenter: Option<&mut dyn MaskSegmenter>,
) -> EngineResult<image::Rgb32FImage> {
    render_with_support(source, settings, segmenter, None)
}

pub(crate) fn render_with_support(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    segmenter: Option<&mut dyn MaskSegmenter>,
    support: Option<&std::path::Path>,
) -> EngineResult<image::Rgb32FImage> {
    render_with_hooks(
        source,
        settings,
        segmenter,
        None,
        None,
        &mut Vec::new(),
        support,
    )
}

pub(crate) fn render_with_hooks(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    segmenter: Option<&mut dyn MaskSegmenter>,
    denoiser: Option<&dyn pipeline_cpu::PostDemosaicDenoise>,
    depth: Option<&image_core::depth::DepthProvider>,
    warnings: &mut Vec<String>,
    mask_support: Option<&std::path::Path>,
) -> EngineResult<image::Rgb32FImage> {
    let mut pre = settings.clone();
    pre.output.proof_profile = None;
    pre.locals = Default::default();
    pre.effects = Default::default();
    pre.geometry = Default::default();
    pipeline_cpu::validate_settings(&pre)?;
    // The public reference API has no mask callback before its private lens
    // warp. Reject that combination instead of applying sensor-space masks to
    // already warped pixels. Ordinary (non-AI) exports retain the full path.
    let metadata = match source {
        RenderSource::Rgb(_) | RenderSource::StoredRgb { .. } => None,
        RenderSource::CameraLinear(proxy) if proxy.is_external_dng() => {
            Some(proxy.original_metadata())
        }
        RenderSource::CameraLinear(_) => return Err(crate::original_required()),
        RenderSource::Cfa { metadata, .. } => Some(*metadata),
    };
    let input = pipeline_cpu::render_linear_before_geometry(&pre, source, denoiser)?;
    let lens = pipeline_cpu::resolve_lens(&input, &pre.lens, metadata, &Default::default())?;
    if pre.lens.manual_distortion != 0.0
        || lens.sample().is_some()
        || lens.source() == pipeline_cpu::CorrectionSource::Embedded
    {
        return Err(error(
            "AI masks with lens warps require a hook-aware lens renderer",
        ));
    }
    let cache = ready_masks(source, settings, segmenter, warnings, mask_support)?;
    let mut planes = input.planes().to_vec();
    for group in settings
        .locals
        .adjustments
        .iter()
        .filter(|g| g.enabled && g.amount != 0.0 && !g.components.is_empty())
    {
        let mask = cache.rasterize(
            &input,
            group,
            0,
            ParamHash::of(StageId::Color, &0u8),
            Default::default(),
        )?;
        let adjusted = pipeline_cpu::adjust_local(&input, &group.params, group.amount)?;
        let blended = pipeline_cpu::blend_local(&input, &adjusted, &mask)?;
        for ((out, original), local) in planes.iter_mut().zip(input.planes()).zip(blended.planes())
        {
            for ((v, b), a) in out.iter_mut().zip(original).zip(local) {
                *v += a - b;
            }
        }
    }
    let mut rgb = Image::new(input.width(), input.height(), planes)?;
    if let Some(blur) = &settings.effects.lens_blur {
        let provider = depth.ok_or_else(|| error("Lens Blur depth provider is missing"))?;
        if let Some(plane) = crate::depth::estimate(provider, &rgb, warnings)? {
            rgb = pipeline_cpu::lens_blur(&rgb, &plane, blur, Default::default())?;
        }
    }
    let mut point_effects = settings.effects.clone();
    point_effects.lens_blur = None;
    let extent = engine_api::tile::Extent::new(rgb.width(), rgb.height());
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        pipeline_cpu::effects_in_crop(&mut tile, &point_effects, extent, &settings.geometry.crop)?;
        rgb.put(&tile)?;
    }
    let rgb = pipeline_cpu::geometry(&rgb, &settings.geometry)?;
    Ok(image::Rgb32FImage::from_fn(
        rgb.width(),
        rgb.height(),
        |x, y| {
            let i = y as usize * rgb.width() as usize + x as usize;
            let v: [f32; 3] = std::array::from_fn(|c| rgb.planes()[c][i]);
            let y = 0.2627 * v[0] + 0.6780 * v[1] + 0.0593 * v[2];
            image::Rgb(if y <= 0.0 {
                [0.0; 3]
            } else {
                v.map(|c| c * pipeline_cpu::sigmoid(y, Default::default()) / y)
            })
        },
    ))
}

/// Materialize every requested external component before rendering. A missing
/// imported plane can regenerate, but a missing model or invalid raster is an
/// export error. The cache and inference context belong only to this export.
fn ready_masks(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    segmenter: Option<&mut dyn MaskSegmenter>,
    warnings: &mut Vec<String>,
    mask_support: Option<&std::path::Path>,
) -> EngineResult<MaskRasterCache> {
    let cache = MaskRasterCache::new(0);
    if let Some(hooks) = ready_hooks(source, settings, segmenter, warnings, mask_support)? {
        cache.set_hooks(Some(hooks));
    }
    Ok(cache)
}

/// The materialized external rasters as mask hooks; `None` when the recipe
/// requests no external component (procedural masks need no hooks).
fn ready_hooks(
    source: &RenderSource<'_>,
    settings: &DevelopSettings,
    segmenter: Option<&mut dyn MaskSegmenter>,
    warnings: &mut Vec<String>,
    mask_support: Option<&std::path::Path>,
) -> EngineResult<Option<Arc<dyn MaskHooks>>> {
    let (w, h, metadata) = match source {
        RenderSource::Rgb(i) | RenderSource::StoredRgb { image: i, .. } => {
            (i.width(), i.height(), None)
        }
        RenderSource::CameraLinear(proxy) if proxy.is_external_dng() => (
            proxy.pixels().width(),
            proxy.pixels().height(),
            Some(proxy.original_metadata()),
        ),
        RenderSource::CameraLinear(_) => return Err(crate::original_required()),
        RenderSource::Cfa { metadata, .. } => (
            metadata.default_crop[2],
            metadata.default_crop[3],
            Some(*metadata),
        ),
    };
    // Rasters live in the stored (sensor) frame for RAW, Smart Preview and
    // catalog-oriented RGB sources alike (LR-8m, LR-8n); segmentation sees
    // the displayed orientation.
    let orientation = match source {
        RenderSource::StoredRgb { orientation, .. } => *orientation,
        _ => metadata.map_or(1, |m| m.orientation),
    };
    // Validate all requests before loading (or downloading) any weights.
    let mut requests = Vec::new();
    for group in settings
        .locals
        .adjustments
        .iter()
        .filter(|g| g.enabled && g.amount != 0.0)
    {
        for c in group
            .components
            .iter()
            .flat_map(|c| c.active_leaves())
            .filter(|c| c.kind.is_ai())
        {
            if !requests.iter().any(|(component, _)| component == c) {
                let request = if matches!(c.kind, engine_api::recipe::MaskKind::Depth { .. })
                    || c.adobe_ai.as_ref().and_then(|s| s.mask_key).is_some()
                {
                    None
                } else {
                    Some(mask_ai::request(&c.kind, orientation).map_err(error)?)
                };
                requests.push((c.clone(), request));
            }
        }
    }
    if requests.is_empty() {
        return Ok(None);
    }
    // Stable as-shot segmentation input; it is independent of local/global edits.
    let scale = w.max(h).div_ceil(2048).max(1);
    let rgb = pipeline_cpu::render_scaled(&DevelopSettings::default(), source, scale)?;
    let (sw, sh) = rgb.dimensions();
    let (dw, dh) = if orientation >= 5 { (sh, sw) } else { (sw, sh) };
    let pixels: Vec<[u8; 3]> = rgb.pixels().map(|p| p.0).collect();
    let shown = mask_ai::reorient(&pixels, sw, sh, dw, dh, |p| mask_ai::orient(p, orientation));
    let shown = image::RgbImage::from_raw(dw, dh, shown.into_iter().flatten().collect())
        .ok_or_else(|| error("segmentation input"))?;
    let support = || -> EngineResult<std::path::PathBuf> {
        mask_support
            .map(|root| Ok(root.to_path_buf()))
            .unwrap_or_else(crate::depth::support)
    };
    let mut loaded = None;
    let mut supplied = segmenter;
    let mut rasters = Vec::new();
    let mut ready_depth = None;
    for (component, request) in requests {
        if let engine_api::recipe::MaskKind::Depth { model, .. } = &component.kind {
            if model.as_ref().is_some_and(|model| {
                model.id.as_str() != image_core::ml_depth::MODEL_ID
                    || model.version != image_core::ml_depth::MODEL_VERSION
            }) {
                return Err(error("unsupported depth-mask model provenance"));
            }
            if ready_depth.is_none() {
                let input = pipeline_cpu::render_linear_before_geometry(
                    &DevelopSettings::default(),
                    source,
                    None,
                )?;
                let shown = image_core::depth::model_input(&input)?;
                let root = support()?;
                let store = image_core::ml_depth::DepthStore::new(
                    root.join("previews/depth-cache"),
                    256 << 20,
                )
                .map_err(error)?;
                let key =
                    image_core::ml_depth::cache_key(&shown, image_core::ml_depth::MODEL_VERSION);
                let depth = match image_core::ml_depth::DepthMap::cached(&store, &key)
                    .filter(|d| (d.width(), d.height()) == (input.width(), input.height()))
                {
                    Some(depth) => depth,
                    None => {
                        image_core::depth::DepthProvider::from_support(&root)?.estimate(&input)?
                    }
                };
                ready_depth = Some((input, depth.near_to_far()));
            }
            let (input, plane) = ready_depth.as_ref().expect("resolved above");
            let group = LocalAdjustment {
                components: vec![MaskComponent::new(component.kind.clone())],
                ..Default::default()
            };
            let alpha = pipeline_cpu::masks::rasterize(
                input,
                &group,
                pipeline_cpu::masks::MaskOptions {
                    depth: Some(plane),
                    ..Default::default()
                },
            )?;
            rasters.push((
                component,
                AlphaPlane {
                    width: input.width(),
                    height: input.height(),
                    data: alpha,
                },
            ));
            continue;
        }
        let request = match request {
            Some(request) => request,
            None => {
                let key = component
                    .adobe_ai
                    .as_ref()
                    .and_then(|s| s.mask_key)
                    .expect("imported reference");
                match mask_ai::imported_plane(&support()?, &key) {
                    Ok(plane) if (plane.width, plane.height) == (w, h) => {
                        rasters.push((component, plane));
                        continue;
                    }
                    _ => warnings.push(
                        "regenerating AI mask: stored raster missing, corrupt or wrong extent"
                            .into(),
                    ),
                }
                mask_ai::request(&component.kind, orientation).map_err(error)?
            }
        };
        // Export never renders a different image than the one the user sees
        // once the mask exists: a model that cannot be loaded, a backend that
        // fails and an invalid raster are all errors, and nothing is published.
        // The model is loaded only when a component actually needs inference.
        if supplied.is_none() && loaded.is_none() {
            loaded = Some(mask_ai::load_segmenter(&support()?).map_err(error)?);
        }
        let segmenter = match (supplied.as_deref_mut(), loaded.as_mut()) {
            (Some(segmenter), _) => segmenter,
            (None, Some(segmenter)) => segmenter.as_mut(),
            (None, None) => unreachable!("loaded above"),
        };
        let alpha = segmenter.segment(&shown, &request).map_err(error)?;
        if alpha.len() != dw as usize * dh as usize
            || alpha.iter().any(|v| !(0.0..=1.0).contains(v))
        {
            return Err(error("invalid segmentation raster"));
        }
        let data = mask_ai::reorient(&alpha, dw, dh, sw, sh, |p| {
            mask_ai::unorient(p, orientation)
        });
        rasters.push((
            component,
            AlphaPlane {
                width: sw,
                height: sh,
                data,
            },
        ));
    }
    Ok(Some(Arc::new(ReadyMasks(rasters))))
}

/// Develop's renderer for every Adobe-process recipe (RAW and RGB originals
/// and Smart Previews, ENG-9) and for external proxies that need Develop's
/// resources: the same local-adjustment barrier, Lens Blur depth, retouch,
/// post-demosaic denoise and process family as Develop. Adobe pixels never
/// receive a Native sigmoid. Returns display-referred linear Rec.2020 floats
/// before the managed output transform.
///
/// Adobe-process recipes are drawn by Develop's renderer itself at the level
/// of `scale` ([`crate::adobe_render`], ENG-10); `scale` must then be 1, 2,
/// 4 or 8.
#[allow(clippy::too_many_arguments)]
pub(crate) fn render_develop(
    source: &RenderSource<'_>,
    recipe: &engine_api::recipe::Recipe,
    scale: u32,
    segmenter: Option<&mut dyn MaskSegmenter>,
    warnings: &mut Vec<String>,
    support: Option<&std::path::Path>,
    retouch: Option<Arc<dyn pipeline_cpu::RetouchRenderer>>,
    cancel: &engine_api::jobs::CancellationToken,
) -> EngineResult<image::Rgb32FImage> {
    let mut settings = recipe.settings.clone();
    settings.output.proof_profile = None;
    let adobe = recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe;
    if adobe {
        // Fail before materializing any resource.
        crate::adobe_render::level(scale)?;
    }
    let hooks = ready_hooks(source, &settings, segmenter, warnings, support)?;
    // Originals resolve the model root like every other export (proxies keep
    // their render plan, which drops Lens Blur without a depth resource).
    let proxy = matches!(source, RenderSource::CameraLinear(_));
    let depth_root = match support {
        Some(root) => Some(root.to_path_buf()),
        None if !proxy && settings.effects.lens_blur.is_some() => Some(crate::depth::support()?),
        None => None,
    };
    let depth_provider = if settings.effects.lens_blur.is_some() {
        depth_root
            .as_deref()
            .map(|root| image_core::depth::DepthProvider::from_support(root).map(Arc::new))
            .transpose()?
    } else {
        None
    };
    let denoiser = match support {
        Some(root) => crate::depth::denoiser(source, &settings, root)?,
        None if matches!(source, RenderSource::Cfa { .. })
            && pipeline_cpu::denoise_active(&settings.denoise) =>
        {
            crate::depth::denoiser(source, &settings, &crate::depth::support()?)?
        }
        None => None,
    };
    if adobe {
        // ENG-10: Develop's own renderer, at the level of the render scale.
        return crate::adobe_render::render(
            source,
            recipe.process_version,
            &settings,
            scale,
            crate::adobe_render::Resources {
                masks: hooks,
                retouch,
                depth: depth_provider,
                denoiser,
            },
            cancel,
        );
    }
    let cache = MaskRasterCache::new(0);
    cache.set_hooks(hooks);
    let locals = |input: &Image, groups: &[LocalAdjustment]| {
        let mut planes = input.planes().to_vec();
        for group in groups
            .iter()
            .filter(|g| g.enabled && g.amount != 0. && !g.components.is_empty())
        {
            let mask = cache.rasterize(
                input,
                group,
                0,
                ParamHash::of(StageId::Color, &0u8),
                Default::default(),
            )?;
            let adjusted = pipeline_cpu::adjust_local(input, &group.params, group.amount)?;
            let blended = pipeline_cpu::blend_local(input, &adjusted, &mask)?;
            for ((out, original), changed) in
                planes.iter_mut().zip(input.planes()).zip(blended.planes())
            {
                for ((out, original), changed) in out.iter_mut().zip(original).zip(changed) {
                    *out += changed - original;
                }
            }
        }
        Image::new(input.width(), input.height(), planes)
    };
    let depth_renderer = depth_provider
        .map(|provider| image_core::Renderer::new(Default::default()).with_depth(provider));
    let depth_effects = |input: &Image| {
        depth_renderer
            .as_ref()
            .expect("installed hook")
            .apply_depth_effects(input, &settings)
    };
    let context = pipeline_cpu::LensContext {
        retouch,
        depth_effects: depth_renderer
            .as_ref()
            .map(|_| &depth_effects as &pipeline_cpu::DepthEffectHook<'_>),
        ..Default::default()
    };
    let denoiser = denoiser
        .as_ref()
        .map(|d| d as &dyn pipeline_cpu::PostDemosaicDenoise);
    let rgb = pipeline_cpu::render_linear_scaled_with_local_hook(
        &settings, source, scale, &context, None, denoiser, &locals,
    )?;
    Ok(crate::depth::tone_map(rgb))
}

#[cfg(test)]
mod lr4_tests {
    use super::*;
    use engine_api::recipe::{MaskComponent, MaskKind};
    #[test]
    fn lr4_nested_ai_activates_raster_export_but_disabled_does_not() {
        let mut c = MaskComponent::new(MaskKind::Brush { strokes: vec![] });
        c.group = Some(vec![MaskComponent::new(MaskKind::Subject { model: None })]);
        let mut s = DevelopSettings::default();
        s.locals.adjustments.push(LocalAdjustment {
            components: vec![c],
            ..Default::default()
        });
        assert!(active(&s));
        s.locals.adjustments[0].components[0].enabled = false;
        assert!(!active(&s));
    }
}

#[cfg(test)]
mod lr5b_tests {
    use super::*;
    use engine_api::recipe::{MaskComponent, MaskKind};
    #[test]
    fn lr5b_missing_stored_raster_regenerates_with_diagnostic() {
        struct Segmenter;
        impl MaskSegmenter for Segmenter {
            fn segment(
                &mut self,
                image: &image::RgbImage,
                _: &mask_ai::SegmentRequest,
            ) -> anyhow::Result<Vec<f32>> {
                Ok(vec![1.; (image.width() * image.height()) as usize])
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let input = Image::new(4, 2, vec![vec![0.18; 8]; 3]).unwrap();
        let source = RenderSource::Rgb(&input);
        let mut c = MaskComponent::new(MaskKind::Subject { model: None });
        c.adobe_ai = Some(engine_api::recipe::mask::AdobeAiMask {
            resource_id: None,
            category: "Subject".into(),
            mask_key: Some([47; 32]),
            regenerate: false,
        });
        let mut settings = DevelopSettings::default();
        let mut group = LocalAdjustment {
            components: vec![c],
            ..Default::default()
        };
        group.params.exposure = 1.;
        settings.locals.adjustments.push(group);
        let mut warnings = vec![];
        let out = render_with_hooks(
            &source,
            &settings,
            Some(&mut Segmenter),
            None,
            None,
            &mut warnings,
            Some(dir.path()),
        )
        .unwrap();
        assert!(
            warnings
                .iter()
                .any(|w| w.contains("regenerat") && w.contains("missing"))
        );
        let base =
            render_with_support(&source, &DevelopSettings::default(), None, Some(dir.path()))
                .unwrap();
        assert!(out.get_pixel(0, 0)[0] > base.get_pixel(0, 0)[0]);
    }
    #[test]
    fn lr5b_file_export_surfaces_missing_raster_regeneration_notice() {
        struct Subject;
        impl MaskSegmenter for Subject {
            fn segment(
                &mut self,
                image: &image::RgbImage,
                _: &mask_ai::SegmentRequest,
            ) -> anyhow::Result<Vec<f32>> {
                Ok(vec![1.; (image.width() * image.height()) as usize])
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let pixels = Image::new(4, 2, vec![vec![0.18; 8]; 3]).unwrap();
        let input = crate::ExportImage {
            source: RenderSource::Rgb(&pixels),
            name: "synthetic",
            sequence: 1,
            date: "",
            metadata: None,
        };
        let mut c = MaskComponent::new(MaskKind::Subject { model: None });
        c.adobe_ai = Some(engine_api::recipe::mask::AdobeAiMask {
            resource_id: None,
            category: "Subject".into(),
            mask_key: Some([46; 32]),
            regenerate: false,
        });
        let mut recipe = engine_api::recipe::Recipe::default();
        recipe.settings.locals.adjustments.push(LocalAdjustment {
            components: vec![c],
            ..Default::default()
        });
        recipe.history.base = recipe.settings.clone();
        for format in [crate::Format::Jpeg { quality: 90 }, crate::Format::Dng] {
            let options = crate::ExportSettings {
                format,
                mask_support: Some(dir.path().to_path_buf()),
                output_dir: dir.path().join("out"),
                ..Default::default()
            };
            let rendered = crate::render_one_cancellable(
                &input,
                &recipe,
                &options,
                &Default::default(),
                None,
                Some(&mut Subject),
            )
            .unwrap();
            assert!(
                rendered
                    .warnings()
                    .iter()
                    .any(|w| w.contains("regenerat") && w.contains("missing"))
            );
        }
    }
    /// LR-5c ruling 1: export never silently differs from what the user sees
    /// once the model arrives. No model is an error; nothing is rendered.
    #[test]
    fn lr5c_export_without_model_is_an_error_for_inverted_and_subtract_adjustments() {
        let dir = tempfile::tempdir().unwrap();
        // The model cannot be loaded from here: deterministic, no network.
        std::fs::create_dir_all(dir.path().join("models/models.toml")).unwrap();
        let input = Image::new(4, 2, vec![vec![0.18; 8]; 3]).unwrap();
        let source = RenderSource::Rgb(&input);
        for subtract in [false, true] {
            let mut ai = MaskComponent::new(MaskKind::Subject { model: None });
            ai.invert = !subtract;
            ai.combine = if subtract {
                engine_api::recipe::mask::MaskCombine::Subtract
            } else {
                engine_api::recipe::mask::MaskCombine::Add
            };
            let mut group = LocalAdjustment {
                components: vec![ai],
                invert: true,
                ..Default::default()
            };
            if subtract {
                group.components.insert(
                    0,
                    MaskComponent::new(MaskKind::Linear {
                        start: [0., 0.],
                        end: [1., 0.],
                    }),
                );
            }
            group.params.exposure = 1.;
            let mut settings = DevelopSettings::default();
            settings.locals.adjustments.push(group);
            let mut warnings = vec![];
            let error = render_with_hooks(
                &source,
                &settings,
                None,
                None,
                None,
                &mut warnings,
                Some(dir.path()),
            )
            .unwrap_err();
            assert!(error.to_string().contains("AI mask"), "{error}");
        }
    }
    /// A missing stored raster with no model to regenerate it is an error too.
    #[test]
    fn lr5c_missing_stored_raster_without_model_is_an_export_error() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("models/models.toml")).unwrap();
        let input = Image::new(4, 2, vec![vec![0.18; 8]; 3]).unwrap();
        let mut c = MaskComponent::new(MaskKind::Subject { model: None });
        c.adobe_ai = Some(engine_api::recipe::mask::AdobeAiMask {
            resource_id: None,
            category: "Subject".into(),
            mask_key: Some([45; 32]),
            regenerate: false,
        });
        let mut group = LocalAdjustment {
            components: vec![c],
            ..Default::default()
        };
        group.params.exposure = 1.;
        let mut settings = DevelopSettings::default();
        settings.locals.adjustments.push(group);
        assert!(
            render_with_hooks(
                &RenderSource::Rgb(&input),
                &settings,
                None,
                None,
                None,
                &mut vec![],
                Some(dir.path()),
            )
            .is_err()
        );
    }
}
