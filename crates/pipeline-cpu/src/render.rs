use crate::{DemosaicAlgorithm, Image, SigmoidSettings};
use engine_api::{
    EngineError, EngineResult,
    color::{ColorMatrix3, WorkingSpace},
    recipe::{
        DevelopSettings,
        settings::{DemosaicMethod, DisplayTransform},
    },
    tile::{Pyramid, TILE_SIZE},
};
use raw_decode::{CfaImage, CfaLayout, RawMetadata};
pub type Rgb8Image = image::RgbImage;

/// CFA needs its metadata because the engine-api pyramid intentionally holds
/// only samples. RGB input is already scene-linear Rec.2020 with D65 white.
pub enum RenderSource<'a> {
    Cfa {
        image: &'a CfaImage,
        metadata: &'a RawMetadata,
    },
    Rgb(&'a Image),
}

pub fn render(settings: &DevelopSettings, source: &RenderSource<'_>) -> EngineResult<Rgb8Image> {
    render_scaled(settings, source, 1)
}

/// Full-resolution operators followed by an area average in linear light.
/// Never decimate the CFA: doing so aliases Bayer/X-Trans colour phases.
pub fn render_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
) -> EngineResult<Rgb8Image> {
    let rgb = render_linear_scaled(settings, source, scale)?;
    let mut out = Rgb8Image::new(rgb.width(), rgb.height());
    for coord in rgb.coords() {
        let tile = crate::display(
            &rgb.tile(coord, 0, 1)?,
            SigmoidSettings::default(),
            settings.output.gamut_mapping,
        )?;
        let l = tile.layout();
        let n = l.plane_len();
        let data = tile.samples::<u8>()?;
        let (ox, oy) = coord.pixel_origin(TILE_SIZE);
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let i = (y * l.extent.width + x) as usize;
                out.put_pixel(
                    ox + x,
                    oy + y,
                    image::Rgb([data[i], data[n + i], data[2 * n + i]]),
                );
            }
        }
    }
    Ok(out)
}

/// Scene-linear Rec.2020 through Geometry, then linear-light box downsampling.
/// Output (display) is not applied. Default detail is active in native revision 2.
pub fn render_linear_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
) -> EngineResult<Image> {
    render_linear_scaled_with_lens(settings, source, scale, &crate::LensContext::default())
}

/// Render with an explicit near-to-far depth plane and caller-owned lens context.
///
/// Depth must contain one finite 0..=1 sample per full-resolution active-area
/// pixel: RGB input dimensions, or RAW metadata.default_crop dimensions. It is
/// aligned before recipe Geometry (crop/rotation/lens warp), never to the output
/// preview. Blur runs after Locals, before vignette/grain, Geometry and downsample.
/// The depth plane is validated even when lens blur is absent; options are used
/// only when `effects.lens_blur` is present. Depth also feeds local depth masks.
/// No model inference or recipe-schema changes are performed here.
pub fn render_linear_scaled_with_depth(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &crate::LensContext<'_>,
    depth: &[f32],
    options: crate::LensBlurOptions,
) -> EngineResult<Image> {
    render_linear_impl(
        settings,
        source,
        scale,
        context,
        Some((depth, options)),
        None,
        None,
    )
}

/// Render with caller-owned database or user profile, without changing recipe schema.
pub fn render_linear_scaled_with_lens(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &crate::LensContext<'_>,
) -> EngineResult<Image> {
    render_linear_impl(settings, source, scale, context, None, None, None)
}

/// The reference render with an already-resolved lens correction (for
/// example from [`crate::resolve_lens_sensor`]) instead of re-analysing the
/// demosaiced frame. Used to gate resident backends on forced calibrations.
pub fn render_linear_scaled_resolved(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    resolved: &crate::ResolvedLens,
) -> EngineResult<Image> {
    render_linear_impl(
        settings,
        source,
        scale,
        &crate::LensContext::default(),
        None,
        None,
        Some(resolved),
    )
}

/// Full reference renderer with caller-owned post-demosaic inference.
pub fn render_linear_scaled_with_denoise(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &crate::LensContext<'_>,
    denoiser: Option<&dyn crate::PostDemosaicDenoise>,
) -> EngineResult<Image> {
    render_linear_impl(settings, source, scale, context, None, denoiser, None)
}

fn render_linear_impl(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    context: &crate::LensContext<'_>,
    depth: Option<(&[f32], crate::LensBlurOptions)>,
    denoiser: Option<&dyn crate::PostDemosaicDenoise>,
    resolved: Option<&crate::ResolvedLens>,
) -> EngineResult<Image> {
    if depth.is_some() {
        let mut without_blur = settings.clone();
        without_blur.effects.lens_blur = None;
        validate_settings(&without_blur)?;
    } else {
        validate_settings(settings)?;
    }
    if scale == 0 {
        return Err(EngineError::invalid("scale", "must be positive"));
    }
    let (mut rgb, mut crop, correction) = match source {
        RenderSource::Rgb(image) => {
            if image.planes().len() != 3 {
                return Err(EngineError::invalid("RGB", "three planes required"));
            }
            if crate::denoise_active(&settings.denoise) {
                return Err(EngineError::invalid(
                    "denoise",
                    "post-demosaic denoise requires a CFA source",
                ));
            }
            let crop = [0, 0, image.width(), image.height()];
            let correction = match resolved {
                Some(r) => r.clone(),
                None => crate::resolve_lens(image, &settings.lens, None, context)?,
            };
            let mut out =
                crate::optics::lateral_ca(image, None, crop, &settings.lens, &correction)?;
            let matrix = crate::white_balance_matrix(
                &settings.white_balance,
                WorkingSpace::LinearRec2020.to_xyz(),
                [1.0; 4],
            )?;
            for coord in out.coords() {
                let mut t = out.tile(coord, 0, 1)?;
                crate::apply_matrix(&mut t, matrix)?;
                out.put(&t)?;
            }
            (out, crop, correction)
        }
        RenderSource::Cfa { image, metadata } => {
            if image.pyramid().extent().width != metadata.width
                || image.pyramid().extent().height != metadata.height
            {
                return Err(EngineError::invalid(
                    "metadata",
                    "dimensions do not match CFA",
                ));
            }
            let cfa = metadata.cfa_layout;
            crate::mosaic::validate_cfa(cfa)?;
            let period = if matches!(cfa, CfaLayout::XTrans(_)) {
                6
            } else {
                2
            };
            if metadata.width < period || metadata.height < period {
                return Err(EngineError::invalid(
                    "CFA",
                    "image must contain a complete CFA period",
                ));
            }
            let camera_xyz = crate::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
                metadata.cam_xyz[r].map(f64::from)
            })))?;
            let profile = WorkingSpace::LinearRec2020.to_xyz().inverse()? * camera_xyz;
            let wb = crate::white_balance_matrix(
                &settings.white_balance,
                camera_xyz,
                metadata.as_shot_wb,
            )?;
            let algorithm = match settings.demosaic.method {
                DemosaicMethod::Auto => DemosaicAlgorithm::MalvarHeCutler,
                DemosaicMethod::Bilinear => DemosaicAlgorithm::Bilinear,
                _ => {
                    return Err(EngineError::invalid(
                        "demosaic",
                        "only Auto (MHC) and Bilinear implemented",
                    ));
                }
            };
            let raw = Image::from_pyramid(image.pyramid())?;
            let mut recovered = Image::blank(raw.width(), raw.height(), 1);
            for coord in raw.coords() {
                let t = raw.tile(coord, 4, period)?;
                recovered.put(&crate::reconstruct_highlights(
                    &t,
                    cfa,
                    settings.linearize.highlight_reconstruction,
                )?)?;
            }
            drop(raw);
            let demosaic_image = |raw: &Image| -> EngineResult<Image> {
                let mut out = Image::blank(raw.width(), raw.height(), 3);
                for coord in raw.coords() {
                    out.put(&crate::demosaic(
                        &raw.tile(coord, 3, period)?,
                        cfa,
                        algorithm,
                    )?)?;
                }
                Ok(out)
            };
            // Resolve/estimate in original camera RGB, never mixed working primaries.
            let mut out = demosaic_image(&recovered)?;
            let correction = match resolved {
                Some(r) => r.clone(),
                None => {
                    let analysis = out.downsample_crop(metadata.default_crop, 1)?;
                    crate::resolve_lens(&analysis, &settings.lens, Some(metadata), context)?
                }
            };
            if correction.ca_active(&settings.lens) {
                if correction.source() == crate::CorrectionSource::Database
                    && matches!(cfa, CfaLayout::Bayer(_))
                {
                    let corrected = crate::optics::lateral_ca(
                        &recovered,
                        Some(cfa),
                        metadata.default_crop,
                        &settings.lens,
                        &correction,
                    )?;
                    out = demosaic_image(&corrected)?;
                } else {
                    out = crate::optics::lateral_ca(
                        &out,
                        None,
                        metadata.default_crop,
                        &settings.lens,
                        &correction,
                    )?;
                }
            }
            out = crate::post_demosaic_denoise(out, camera_xyz, &settings.denoise, denoiser)?;
            for coord in out.coords() {
                let mut t = out.tile(coord, 0, 1)?;
                // Contract ordering is CameraProfile THEN WhiteBalance.
                crate::apply_matrix(&mut t, profile)?;
                crate::apply_matrix(&mut t, wb)?;
                out.put(&t)?;
            }
            (out, metadata.default_crop, correction)
        }
    };
    // All channel alignment is complete before matrices/detail/tone.
    let analysis = rgb.downsample_crop(crop, 1)?;
    if let Some((plane, _)) = depth
        && (plane.len() != analysis.width() as usize * analysis.height() as usize
            || plane
                .iter()
                .any(|d| !d.is_finite() || !(0. ..=1.).contains(d)))
    {
        return Err(EngineError::invalid(
            "depth",
            "finite near-to-far active-area plane required",
        ));
    }
    let needs_m2 = has_m2_settings(settings)
        || correction.sample().is_some()
        || correction.source() == crate::CorrectionSource::Embedded;
    if needs_m2 {
        rgb = analysis;
        crop = [0, 0, rgb.width(), rgb.height()];
    }
    rgb = crate::optics::profile_vignette(&rgb, &settings.lens, &correction)?;
    rgb = crate::optics::point_corrections(&rgb, &settings.lens)?;
    if crate::detail_halo(&settings.detail) > 0 || settings.detail != Default::default() {
        let workers = std::thread::available_parallelism().map_or(1, usize::from);
        rgb = detail_image(&rgb, &settings.detail, workers)?;
    }
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        crate::tone(&mut tile, &settings.tone)?;
        rgb.put(&tile)?;
    }
    if needs_m2 {
        // Remove masked sensor margins before estimating global airlight.
        rgb = rgb.downsample_crop(crop, 1)?;
        rgb = crate::tone_extra_image(&rgb, &settings.tone)?;
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, 0, 1)?;
            crate::color(&mut tile, &settings.color)?;
            rgb.put(&tile)?;
        }
        rgb = crate::locals_image(
            &rgb,
            &settings.locals.adjustments,
            crate::masks::MaskOptions {
                depth: depth.map(|(plane, _)| plane),
                ..Default::default()
            },
        )?;
        if let Some(blur) = &settings.effects.lens_blur {
            let (plane, options) =
                depth.ok_or_else(|| EngineError::invalid("depth", "lens blur requires depth"))?;
            rgb = crate::lens_blur(&rgb, plane, blur, options)?;
        }
        let mut point_effects = settings.effects.clone();
        point_effects.lens_blur = None;
        let extent = engine_api::tile::Extent::new(rgb.width(), rgb.height());
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, 0, 1)?;
            crate::effects_in_crop(&mut tile, &point_effects, extent, &settings.geometry.crop)?;
            rgb.put(&tile)?;
        }
        let mut common = settings.lens.clone();
        common.remove_chromatic_aberration = false;
        rgb = crate::geometry_effects::geometry_mapped(
            &rgb,
            &settings.geometry,
            correction.geometry_active(&common),
            |p, _| Some(correction.map(p, 1, &common)),
        )?;
        rgb.downsample_crop([0, 0, rgb.width(), rgb.height()], scale)
    } else {
        rgb.downsample_crop(crop, scale)
    }
}

// Independent scalar tiles retain their arithmetic and immutable real-neighbour
// halos. Bound scratch storage to eight workers rather than retaining all tiles.
fn detail_image(
    input: &Image,
    settings: &engine_api::recipe::settings::DetailSettings,
    workers: usize,
) -> EngineResult<Image> {
    let coords: Vec<_> = input.coords().collect();
    let workers = workers.clamp(1, 8).min(coords.len());
    let output = std::sync::Mutex::new(Image::blank(input.width(), input.height(), 3));
    std::thread::scope(|scope| -> EngineResult<()> {
        let mut handles = Vec::new();
        for worker in 0..workers {
            let coords = &coords;
            let output = &output;
            handles.push(scope.spawn(move || -> EngineResult<()> {
                for &coord in coords.iter().skip(worker).step_by(workers) {
                    let mut tile = input.tile(coord, crate::detail_halo(settings), 1)?;
                    crate::detail(&mut tile, settings)?;
                    output
                        .lock()
                        .expect("detail output lock poisoned")
                        .put(&tile)?;
                }
                Ok(())
            }));
        }
        for handle in handles {
            handle.join().expect("detail worker panicked")?;
        }
        Ok(())
    })?;
    Ok(output.into_inner().expect("detail output lock poisoned"))
}

/// Whether a recipe needs M2 neighbourhood, colour, effect or geometry passes.
pub fn has_m2_settings(s: &DevelopSettings) -> bool {
    !s.locals.adjustments.is_empty()
        || crate::detail_halo(&s.detail) > 0
        || s.detail != Default::default()
        || s.color != Default::default()
        || s.effects != Default::default()
        || s.geometry != Default::default()
        || s.lens.manual_distortion != 0.
        || s.lens.manual_vignetting != 0.
        || s.lens.defringe_purple.amount != 0.
        || s.lens.defringe_green.amount != 0.
        || s.tone.texture != 0.0
        || s.tone.clarity != 0.0
        || s.tone.dehaze != 0.0
        || s.tone.curves != Default::default()
}

/// Reject changed out-of-scope controls instead of silently ignoring them.
/// Public so tiled renderers built on these operators apply the same scope.
pub fn validate_settings(s: &DevelopSettings) -> EngineResult<()> {
    use engine_api::recipe::settings::HighlightReconstruction;
    if !matches!(
        s.demosaic.method,
        DemosaicMethod::Auto | DemosaicMethod::Bilinear
    ) || !matches!(
        s.linearize.highlight_reconstruction,
        HighlightReconstruction::Clip | HighlightReconstruction::ReconstructColor
    ) {
        return Err(EngineError::invalid(
            "settings",
            "unsupported M1 reconstruction/demosaic method",
        ));
    }
    let default = DevelopSettings::default();
    let mut supported = default.clone();
    crate::validate_denoise(&s.denoise)?;
    supported.denoise = s.denoise.clone();
    supported.linearize = s.linearize.clone();
    supported.demosaic.method = s.demosaic.method;
    supported.white_balance = s.white_balance.clone();
    supported.tone = s.tone.clone();
    supported.detail = s.detail.clone();
    crate::optics::validate(&s.lens)?;
    supported.lens = s.lens.clone();
    supported.locals.adjustments = s.locals.adjustments.clone();
    supported.color.vibrance = s.color.vibrance;
    supported.color.saturation = s.color.saturation;
    supported.color.hsl = s.color.hsl.clone();
    supported.color.grading = s.color.grading.clone();
    supported.effects.vignette = s.effects.vignette.clone();
    supported.effects.grain = s.effects.grain.clone();
    supported.geometry = s.geometry.clone();
    supported.output.gamut_mapping = s.output.gamut_mapping;
    if s != &supported
        || !matches!(
            s.tone.display_transform,
            DisplayTransform::Native | DisplayTransform::Sigmoid
        )
    {
        return Err(EngineError::invalid(
            "settings",
            "non-default operator not implemented by CPU reference renderer",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parallel_detail_is_bit_exact_across_tiles_and_edges() {
        let input = Image::new(
            519,
            263,
            (0..3)
                .map(|c| {
                    (0..519 * 263)
                        .map(|i| ((i * 17 + c * 13) % 257) as f32 / 256.0)
                        .collect()
                })
                .collect(),
        )
        .unwrap();
        let settings = Default::default();
        let serial = detail_image(&input, &settings, 1).unwrap();
        let parallel = detail_image(&input, &settings, 4).unwrap();
        assert_eq!(serial.planes(), parallel.planes());
    }
}
