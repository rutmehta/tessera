use crate::{Image, RenderSource, Rgb8Image, basic_tone, dcp::DcpProfile};
use engine_api::{
    EngineError, EngineResult,
    color::{ChromaticAdaptation, ColorMatrix3, WorkingSpace},
    recipe::{
        DevelopSettings,
        settings::{DisplayTransform, WhiteBalanceMode},
    },
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
    pipeline_cpu::validate_settings(&checked)?;
    crate::curves::validate(&settings.tone.curves)?;
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
    base.effects = Default::default();
    base.geometry = Default::default();
    if profile.is_some() {
        base.detail.sharpening.amount = 0.;
        base.detail.noise_reduction.luminance = 0.;
        base.detail.noise_reduction.color = 0.;
    }
    let mut rgb = pipeline_cpu::render_linear_scaled(&base, source, 1)?;
    if let (Some(profile), RenderSource::Cfa { metadata, .. }) = (profile, source) {
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
        let (temperature, tint) = match settings.white_balance.mode {
            WhiteBalanceMode::AsShot => {
                pipeline_cpu::as_shot_temperature_tint(camera_xyz, metadata.as_shot_wb)?
            }
            WhiteBalanceMode::Custom => (
                settings.white_balance.temperature,
                settings.white_balance.tint,
            ),
            WhiteBalanceMode::Daylight | WhiteBalanceMode::Flash => (5503., 0.),
            WhiteBalanceMode::Cloudy => (6504., 0.),
            WhiteBalanceMode::Shade => (7504., 0.),
            WhiteBalanceMode::Tungsten => (2856., 0.),
            WhiteBalanceMode::Fluorescent => (4230., 0.),
            WhiteBalanceMode::Auto => {
                return Err(EngineError::invalid(
                    "white balance",
                    "Auto is not implemented",
                ));
            }
        };
        // DCP's temperature path uses its own Bradford/daylight approximation.
        // Approximate tint with the native CAT16 residual at fixed temperature,
        // NOT the full native WB again. This is not Adobe's proprietary tint.
        let mut tinted = settings.white_balance.clone();
        tinted.mode = WhiteBalanceMode::Custom;
        tinted.temperature = temperature;
        tinted.tint = tint;
        let mut neutral = tinted.clone();
        neutral.tint = 0.;
        let tint_matrix = if tint == 0. {
            ColorMatrix3::IDENTITY
        } else {
            pipeline_cpu::white_balance_matrix(&tinted, camera_xyz, metadata.as_shot_wb)?
                * pipeline_cpu::white_balance_matrix(&neutral, camera_xyz, metadata.as_shot_wb)?
                    .inverse()?
        };
        for coord in rgb.coords() {
            let mut tile = rgb.tile(coord, 0, 1)?;
            pipeline_cpu::apply_matrix(&mut tile, undo)?;
            pipeline_cpu::map_rgb(&mut tile, |p| profile.apply_without_tone(p, temperature))?;
            pipeline_cpu::apply_matrix(&mut tile, tint_matrix)?;
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
    for coord in rgb.coords() {
        let mut tile = rgb.tile(coord, 0, 1)?;
        pipeline_cpu::map_rgb(&mut tile, |p| basic_tone(p, &settings.tone))?;
        // ProfileToneCurve is deferred until after exposure/basic tone, once.
        if let Some(profile) = profile {
            pipeline_cpu::map_rgb(&mut tile, |p| profile.apply_tone(p))?;
        }
        rgb.put(&tile)?;
    }
    let mut extra = settings.tone.clone();
    // Native guided local-contrast/dehaze and parametric curve are documented
    // approximations. Point curves must not run again on the native log axis.
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
                p.map(crate::curves::default_tone)
            } else {
                p
            };
            crate::curves::apply(p, &settings.tone.curves)
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
    pipeline_cpu::render_linear_scaled(&rest, &RenderSource::Rgb(&rgb), scale)
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
