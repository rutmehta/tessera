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
    let (w, h, metadata) = match source {
        RenderSource::Rgb(i) => (i.width(), i.height(), None),
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
    let (w, h) = if metadata
        .and_then(|m| m.catalog_orientation)
        .is_some_and(|o| o >= 5)
    {
        (h, w)
    } else {
        (w, h)
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
    let orientation = metadata.map_or(1, |m| m.orientation);
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
                let request = if c.adobe_ai.as_ref().and_then(|s| s.mask_key).is_some() {
                    None
                } else {
                    Some(mask_ai::request(&c.kind, orientation).map_err(error)?)
                };
                requests.push((c.clone(), request));
            }
        }
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
            .map(std::path::Path::to_path_buf)
            .or_else(|| std::env::var_os("TESSERA_APP_SUPPORT").map(std::path::PathBuf::from))
            .or_else(|| {
                std::env::var_os("HOME").map(|p| {
                    std::path::PathBuf::from(p).join("Library/Application Support/Tessera")
                })
            })
            .ok_or_else(|| error("set TESSERA_APP_SUPPORT to the model support directory"))
    };
    let mut loaded = None;
    let mut supplied = segmenter;
    let mut rasters = Vec::new();
    for (component, request) in requests {
        let Some(request) = request else {
            let key = component
                .adobe_ai
                .as_ref()
                .and_then(|s| s.mask_key)
                .expect("imported reference");
            let plane = mask_ai::imported_plane(&support()?, &key).map_err(error)?;
            if (plane.width, plane.height) != (w, h) {
                return Err(error("imported raster extent does not match original"));
            }
            rasters.push((component, plane));
            continue;
        };
        let segmenter: &mut dyn MaskSegmenter = match supplied.as_deref_mut() {
            Some(s) => s,
            None => {
                if loaded.is_none() {
                    loaded = Some(mask_ai::load_segmenter(&support()?).map_err(error)?);
                }
                loaded.as_mut().expect("loaded").as_mut()
            }
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
    let cache = MaskRasterCache::new(0);
    cache.set_hooks(Some(Arc::new(ReadyMasks(rasters))));
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
