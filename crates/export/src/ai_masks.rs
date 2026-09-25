//! Per-export AI rasters. No process-global hooks, cache or test backend.
use engine_api::{
    EngineError, EngineResult,
    recipe::{
        DevelopSettings,
        mask::{LocalAdjustment, MaskKind},
    },
    stage::{ParamHash, StageId},
};
use image_core::{MaskRasterCache, mask_cache::MaskHooks};
use mask_ai::{AlphaPlane, MaskSegmenter};
use pipeline_cpu::{Image, RenderSource};
use std::sync::Arc;

pub(crate) fn active(settings: &DevelopSettings) -> bool {
    settings
        .locals
        .adjustments
        .iter()
        .any(|g| g.enabled && g.amount != 0.0 && g.components.iter().any(|c| c.kind.is_ai()))
}

struct ReadyMasks(Vec<(MaskKind, AlphaPlane)>);
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
        mask_ai::compose(input, group, |kind, w, h| {
            let (_, plane) = self
                .0
                .iter()
                .find(|(k, _)| k == kind)
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
    let mut pre = settings.clone();
    pre.output.proof_profile = None;
    pipeline_cpu::validate_settings(&pre)?;
    pre.locals = Default::default();
    pre.effects = Default::default();
    pre.geometry = Default::default();
    // The public reference API has no mask callback before its private lens
    // warp. Reject that combination instead of applying sensor-space masks to
    // already warped pixels. Ordinary (non-AI) exports retain the full path.
    let (w, h, metadata) = match source {
        RenderSource::Rgb(i) => (i.width(), i.height(), None),
        RenderSource::Cfa { metadata, .. } => (
            metadata.default_crop[2],
            metadata.default_crop[3],
            Some(*metadata),
        ),
    };
    let input = pipeline_cpu::render_linear_scaled(&pre, source, 1)?;
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
        for c in group.components.iter().filter(|c| c.kind.is_ai()) {
            if !requests.iter().any(|(kind, _)| kind == &c.kind) {
                requests.push((
                    c.kind.clone(),
                    mask_ai::request(&c.kind, orientation).map_err(error)?,
                ));
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
    let mut loaded;
    let segmenter = match segmenter {
        Some(s) => s,
        None => {
            let support = std::env::var_os("TESSERA_APP_SUPPORT")
                .map(std::path::PathBuf::from)
                .or_else(|| {
                    std::env::var_os("HOME").map(|p| {
                        std::path::PathBuf::from(p).join("Library/Application Support/Tessera")
                    })
                })
                .ok_or_else(|| error("set TESSERA_APP_SUPPORT to the model support directory"))?;
            loaded = mask_ai::load_segmenter(&support).map_err(error)?;
            loaded.as_mut()
        }
    };
    let mut rasters = Vec::new();
    for (kind, request) in requests {
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
            kind,
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
    let extent = engine_api::tile::Extent::new(rgb.width(), rgb.height());
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        pipeline_cpu::effects_in_crop(
            &mut tile,
            &settings.effects,
            extent,
            &settings.geometry.crop,
        )?;
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
