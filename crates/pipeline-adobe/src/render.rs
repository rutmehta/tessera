use crate::{Image, RenderSource, Rgb8Image, basic_tone, dcp::DcpProfile};
use engine_api::{
    EngineError, EngineResult,
    color::{ChromaticAdaptation, ColorMatrix3, WorkingSpace},
    recipe::{DevelopSettings, settings::DisplayTransform},
};

/// Full-resolution compatibility operators followed by linear-light area reduction.
pub fn render_linear_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
) -> EngineResult<Image> {
    render_linear_scaled_with_profile(settings, source, scale, None)
}

/// Render with an explicitly resolved, parsed DCP; names never become paths.
/// DCP requires CFA camera data, not already-converted working-space RGB.
pub fn render_linear_scaled_with_profile(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    profile: Option<&DcpProfile>,
) -> EngineResult<Image> {
    render_linear_scaled_with_profile_and_locals(settings, source, scale, profile, None)
}

/// Compatibility tone/profile with host-supplied pre-geometry local masks.
pub fn render_linear_scaled_with_profile_and_locals(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    profile: Option<&DcpProfile>,
    locals: Option<&pipeline_cpu::LocalAdjustmentHook<'_>>,
) -> EngineResult<Image> {
    render_linear_scaled_with_resources(
        settings,
        source,
        scale,
        profile,
        locals,
        &Default::default(),
    )
}

/// Compatibility rendering with caller-owned retouch and pre-geometry depth.
/// Profile/exposure behavior is unchanged; resources participate only at their
/// existing operator barriers.
pub fn render_linear_scaled_with_resources(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    profile: Option<&DcpProfile>,
    locals: Option<&pipeline_cpu::LocalAdjustmentHook<'_>>,
    context: &pipeline_cpu::LensContext<'_>,
) -> EngineResult<Image> {
    render_linear_scaled_with_denoiser(settings, source, scale, profile, locals, context, None)
}

/// [`render_linear_scaled_with_resources`] with Develop's caller-owned
/// post-demosaic denoiser, applied in the native preprocessing prefix exactly
/// where the Develop renderer applies it (ENG-9: exports of RAW originals
/// with neural denoise). `None` is that function unchanged.
pub fn render_linear_scaled_with_denoiser(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    profile: Option<&DcpProfile>,
    locals: Option<&pipeline_cpu::LocalAdjustmentHook<'_>>,
    context: &pipeline_cpu::LensContext<'_>,
    denoiser: Option<&dyn pipeline_cpu::PostDemosaicDenoise>,
) -> EngineResult<Image> {
    let embedded;
    let profile = if profile.is_none() {
        embedded = match source {
            RenderSource::CameraLinear(proxy) => {
                crate::embedded_profile_fallback(proxy, settings, proxy.embedded_profile()).0
            }
            _ => None,
        };
        embedded.as_ref()
    } else {
        profile
    };
    let planned;
    let settings = if let RenderSource::CameraLinear(proxy) = source {
        planned = proxy
            .render_plan_with_resources(
                settings,
                locals.is_some(),
                context.depth_effects.is_some(),
                context.retouch.is_some(),
            )
            .0;
        &planned
    } else {
        settings
    };
    if matches!(source, RenderSource::CameraLinear(proxy) if !proxy.is_external_dng()) {
        return Err(EngineError::Unsupported {
            what: "Adobe rendering: original required; camera-linear Smart Previews use Native revision 2".into(),
        });
    }
    if profile.is_some() && matches!(source, RenderSource::Rgb(_)) {
        return Err(EngineError::invalid(
            "DCP profile",
            "requires a CFA source; RGB is already in working space",
        ));
    }
    if scale == 0 {
        return Err(EngineError::invalid("scale", "must be positive"));
    }
    let mut checked = settings.clone();
    checked.tone.display_transform = DisplayTransform::Native;
    // Profile names are identities, not paths. See ADOBE_COMPAT.md for resolution.
    checked.camera_profile.profile = Default::default();
    let mut validation = checked.clone();
    if context.depth_effects.is_some() {
        validation.effects.lens_blur = None;
    }
    pipeline_cpu::validate_settings_with_retouch(&validation, context.retouch.as_deref())?;
    let curves = settings
        .tone
        .curves_extended
        .as_ref()
        .unwrap_or(&settings.tone.curves);
    crate::curves::validate_domain(curves, settings.tone.curves_extended.is_some())?;
    if let Some(legacy) = &settings.tone.legacy_pv2010 {
        pipeline_cpu::legacy_pv2010::validate(legacy)?;
    }
    let values = [
        &settings.tone.exposure,
        &settings.tone.contrast,
        &settings.tone.highlights,
        &settings.tone.shadows,
        &settings.tone.whites,
        &settings.tone.blacks,
    ];
    if values.iter().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid("tone", "finite parameters required"));
    }
    let mut base = checked.clone();
    base.tone = Default::default();
    base.color = Default::default();
    base.locals = Default::default();
    base.locals.retouch = settings.locals.retouch.clone();
    base.effects = Default::default();
    base.geometry = Default::default();
    if profile.is_some() {
        base.detail.sharpening.amount = 0.;
        base.detail.noise_reduction.luminance = 0.;
        base.detail.noise_reduction.color = 0.;
    }
    let camera_metadata = match source {
        RenderSource::Cfa { metadata, .. } => Some(*metadata),
        RenderSource::CameraLinear(proxy) => Some(proxy.original_metadata()),
        RenderSource::Rgb(_) => None,
    };
    if let Some(metadata) = camera_metadata {
        crate::validate_baseline_exposure(metadata.baseline_exposure)?;
    }
    let mut rgb =
        pipeline_cpu::render_linear_scaled_with_denoise(&base, source, 1, context, denoiser)?;
    if let (Some(profile), Some(metadata)) = (profile, camera_metadata) {
        let camera_xyz = pipeline_cpu::camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
            metadata.cam_xyz[r].map(f64::from)
        })))?;
        let native_profile = WorkingSpace::LinearRec2020.to_xyz().inverse()? * camera_xyz;
        let native_wb = pipeline_cpu::white_balance_matrix(
            &settings.white_balance,
            camera_xyz,
            metadata.as_shot_wb,
        )?;
        // Native preprocessing has no tone/detail here. Undo its WB * profile
        // matrix to recover unbalanced camera RGB after demosaic/denoise/optics.
        // This assumes point optics are channel-neutral and no clipping occurs;
        // spatial resampling remains in native preprocessing (an approximation).
        let undo = (native_wb * native_profile).inverse()?;
        let wb =
            profile.resolve_for_camera(&settings.white_balance, camera_xyz, metadata.as_shot_wb)?;
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, 0, 1)?;
            pipeline_cpu::apply_matrix(&mut tile, undo)?;
            pipeline_cpu::map_rgb(&mut tile, |p| profile.apply_camera(p, &wb))?;
            rgb.put(&tile)?;
        }
        // Read every halo from the same pre-detail image, avoiding tile seams
        // and applying Detail exactly once, after the DCP and before basic tone.
        let mut detailed = rgb.clone();
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, pipeline_cpu::detail_halo(&settings.detail), 1)?;
            pipeline_cpu::detail(&mut tile, &settings.detail)?;
            detailed.put(&tile)?;
        }
        rgb = detailed;
    }
    let mut basic = settings.tone.clone();
    basic.exposure = 0.;
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        if let (Some(profile), Some(metadata)) = (profile, camera_metadata) {
            pipeline_cpu::map_rgb(&mut tile, |p| {
                profile.apply_exposure(
                    p,
                    metadata.baseline_exposure + settings.tone.exposure.clamp(-10., 10.),
                )
            })?;
        } else {
            let gain = (camera_metadata.map_or(0., |m| m.baseline_exposure)
                + settings.tone.exposure.clamp(-10., 10.))
            .exp2();
            pipeline_cpu::map_rgb(&mut tile, |p| p.map(|v| v * gain))?;
        }
        pipeline_cpu::map_rgb(&mut tile, |p| basic_tone(p, &basic))?;
        // ProfileToneCurve is deferred until after exposure/basic tone, once.
        if let Some(profile) = profile {
            pipeline_cpu::map_rgb(&mut tile, |p| profile.apply_tone(profile.apply_look(p)))?;
        }
        rgb.put(&tile)?;
    }
    let pre_curve = settings.color_before_curves();
    if pre_curve.monochrome.as_ref().is_some_and(|m| m.enabled) {
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, 0, 1)?;
            pipeline_cpu::color(&mut tile, &pre_curve)?;
            rgb.put(&tile)?;
        }
    }
    let mut extra = settings.tone.clone();
    // Native guided local-contrast/dehaze and parametric curve are documented
    // approximations. Point curves must not run again on the native log axis.
    extra.curves_extended = None;
    extra.curves = Default::default();
    extra.curves.parametric = settings.tone.curves.parametric.clone();
    rgb = pipeline_cpu::tone_extra_image(&rgb, &extra)?;
    let to_pro = WorkingSpace::LinearRec2020
        .conversion_to(WorkingSpace::LinearProPhoto, ChromaticAdaptation::Bradford)?;
    let from_pro = to_pro.inverse()?;
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        pipeline_cpu::apply_matrix(&mut tile, to_pro)?;
        pipeline_cpu::map_rgb(&mut tile, |p| {
            let p = if profile.is_none() {
                p.map(|x| {
                    if settings.tone.curves_extended.is_some() && !(0. ..=1.).contains(&x) {
                        x
                    } else {
                        crate::curves::default_tone(x)
                    }
                })
            } else {
                p
            };
            crate::curves::apply_domain(p, curves, settings.tone.curves_extended.is_some())
        })?;
        pipeline_cpu::apply_matrix(&mut tile, from_pro)?;
        rgb.put(&tile)?;
    }
    let mut rest = checked;
    rest.camera_profile = Default::default();
    rest.white_balance = Default::default();
    rest.lens = Default::default();
    rest.denoise = Default::default();
    rest.detail.sharpening.amount = 0.;
    rest.detail.noise_reduction.luminance = 0.;
    rest.detail.noise_reduction.color = 0.;
    rest.tone = Default::default();
    rest.color = settings.color_after_curves();
    rest.locals.retouch.clear();
    let tail = pipeline_cpu::LensContext {
        depth_effects: context.depth_effects,
        ..Default::default()
    };
    if let Some(locals) = locals {
        pipeline_cpu::render_linear_scaled_with_local_hook(
            &rest,
            &RenderSource::Rgb(&rgb),
            scale,
            &tail,
            None,
            None,
            locals,
        )
    } else {
        pipeline_cpu::render_linear_scaled_with_lens(&rest, &RenderSource::Rgb(&rgb), scale, &tail)
    }
}

/// Display sRGB after the compatibility profile/user curves (no native sigmoid).
pub fn render_scaled(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
) -> EngineResult<Rgb8Image> {
    render_scaled_with_profile(settings, source, scale, None)
}

/// Display rendering with an explicitly parsed optional camera profile.
pub fn render_scaled_with_profile(
    settings: &DevelopSettings,
    source: &RenderSource<'_>,
    scale: u32,
    profile: Option<&DcpProfile>,
) -> EngineResult<Rgb8Image> {
    let rgb = render_linear_scaled_with_profile(settings, source, scale, profile)?;
    encode(&rgb)
}

fn encode(rgb: &Image) -> EngineResult<Rgb8Image> {
    let m = WorkingSpace::LinearSrgb.to_xyz().inverse()? * WorkingSpace::LinearRec2020.to_xyz();
    let mut output = Rgb8Image::new(rgb.width(), rgb.height());
    for (i, pixel) in output.pixels_mut().enumerate() {
        let value = m.apply([
            rgb.planes()[0][i] as f64,
            rgb.planes()[1][i] as f64,
            rgb.planes()[2][i] as f64,
        ]);
        *pixel = image::Rgb(
            value.map(|v| (pipeline_cpu::srgb_oetf(v as f32).clamp(0., 1.) * 255.).round() as u8),
        );
    }
    Ok(output)
}
