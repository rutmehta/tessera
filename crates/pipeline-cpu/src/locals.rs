//! Masked adjustment layers in scene-linear Rec.2020.
use crate::{
    Image,
    masks::{MaskOptions, rasterize},
};
use engine_api::{
    EngineError, EngineResult,
    recipe::mask::{LocalAdjustment, LocalParams},
};

/// Apply each group to the immutable pre-local image and sum masked deltas.
/// Amount scales slider parameters, not opacity (the recipe contract).
pub fn locals_image(
    input: &Image,
    groups: &[LocalAdjustment],
    options: MaskOptions<'_>,
) -> EngineResult<Image> {
    let mut planes = input.planes().to_vec();
    for group in groups
        .iter()
        .filter(|g| g.enabled && g.amount != 0.0 && !g.components.is_empty())
    {
        let mask = rasterize(input, group, options)?;
        let adjusted = adjust_local(input, &group.params, group.amount)?;
        let blended = blend_local(input, &adjusted, &mask)?;
        for (c, plane) in planes.iter_mut().enumerate() {
            for (i, v) in plane.iter_mut().enumerate() {
                if mask[i] != 0.0 {
                    *v += blended.planes()[c][i] - input.planes()[c][i];
                }
            }
        }
    }
    Image::new(input.width(), input.height(), planes)
}

/// Compute one unmasked local adjustment from the pre-local working image.
pub fn adjust_local(input: &Image, p: &LocalParams, amount: f32) -> EngineResult<Image> {
    use engine_api::{
        color::{ChromaticAdaptation, WorkingSpace},
        recipe::settings::{ColorSettings, DetailSettings, ToneSettings},
    };
    let values = [
        p.exposure,
        p.contrast,
        p.highlights,
        p.shadows,
        p.whites,
        p.blacks,
        p.temperature,
        p.tint,
        p.texture,
        p.clarity,
        p.dehaze,
        p.saturation,
        p.sharpness,
        p.noise,
        p.moire,
        p.hue,
        p.defringe,
    ];
    if !amount.is_finite()
        || !(0.0..=200.0).contains(&amount)
        || values.iter().any(|v| !v.is_finite())
        || input.planes().len() != 3
    {
        return Err(EngineError::invalid(
            "locals",
            "invalid amount, parameters or RGB image",
        ));
    }
    if !(0.0..=100.0).contains(&p.defringe)
        || p.color_overlay.is_some_and(|v| {
            !v[0].is_finite() || !(0.0..=360.0).contains(&v[0]) || !(0.0..=100.0).contains(&v[1])
        })
    {
        return Err(EngineError::invalid(
            "locals",
            "invalid defringe or colour overlay",
        ));
    }
    for point in p.point_colors.iter().flatten() {
        point.validate()?;
    }
    if amount == 0.0
        || (values.iter().all(|v| *v == 0.0)
            && p.curves.is_none()
            && p.curves_extended.is_none()
            && p.point_colors.is_none()
            && p.color_overlay.is_none())
    {
        return Ok(input.clone());
    }
    let scale = amount / 100.0;
    let slider = |v: f32| (v * scale).clamp(-100.0, 100.0);
    let tone = ToneSettings {
        exposure: (p.exposure * scale).clamp(-10.0, 10.0),
        contrast: slider(p.contrast),
        highlights: slider(p.highlights),
        shadows: slider(p.shadows),
        whites: slider(p.whites),
        blacks: slider(p.blacks),
        texture: slider(p.texture),
        clarity: slider(p.clarity),
        dehaze: slider(p.dehaze),
        ..Default::default()
    };
    let mut result = input.clone();
    let wb = if p.temperature != 0.0 || p.tint != 0.0 {
        let work = WorkingSpace::LinearRec2020.to_xyz();
        let source = crate::temperature_white(6504.0, 0.0)?;
        let target = crate::temperature_white(
            6504.0 * (-slider(p.temperature) / 100.0).exp2(),
            -slider(p.tint),
        )?;
        Some(work.inverse()? * ChromaticAdaptation::Cat16.matrix(source, target)? * work)
    } else {
        None
    };
    for coord in input.coords() {
        let mut tile = input.tile(coord, 0, 1)?;
        if let Some(m) = wb {
            crate::apply_matrix(&mut tile, m)?;
        }
        crate::tone(&mut tile, &tone)?;
        result.put(&tile)?;
    }
    if tone.texture != 0.0 || tone.clarity != 0.0 || tone.dehaze != 0.0 {
        result = crate::tone_extra_image(&result, &tone)?;
    }
    if p.saturation != 0.0 || p.hue != 0.0 {
        for coord in result.coords() {
            let mut tile = result.tile(coord, 0, 1)?;
            crate::color(
                &mut tile,
                &ColorSettings {
                    saturation: slider(p.saturation),
                    ..Default::default()
                },
            )?;
            if p.hue != 0.0 {
                let angle = (f64::from(p.hue) * f64::from(scale)).rem_euclid(360.0) as f32;
                let (sin, cos) = angle.to_radians().sin_cos();
                crate::map_rgb(&mut tile, |rgb| {
                    let [l, a, b] = crate::color_detail::to_lab(rgb);
                    crate::color_detail::from_lab([l, a * cos - b * sin, a * sin + b * cos])
                })?;
            }
            result.put(&tile)?;
        }
    }
    // Local controls are signed. Positive sharpness uses global capture detail;
    // negative sharpness attenuates detail through NR. Negative noise adds back
    // the removed residual instead of inventing stochastic noise.
    for (sharpness, noise, reverse) in [
        (
            slider(p.sharpness).max(0.0),
            (-slider(p.sharpness)).max(0.0),
            false,
        ),
        (0.0, slider(p.noise).abs(), p.noise < 0.0),
    ] {
        if sharpness == 0.0 && noise == 0.0 {
            continue;
        }
        let mut detail = DetailSettings::default();
        detail.sharpening.amount = sharpness;
        detail.noise_reduction.color = 0.0;
        detail.noise_reduction.luminance = noise;
        let mut filtered = result.clone();
        for coord in result.coords() {
            let mut tile = result.tile(coord, crate::detail_halo(&detail), 1)?;
            crate::detail(&mut tile, &detail)?;
            filtered.put(&tile)?;
        }
        if reverse {
            let planes = result
                .planes()
                .iter()
                .zip(filtered.planes())
                .map(|(a, b)| a.iter().zip(b).map(|(a, b)| 2.0 * a - b).collect())
                .collect();
            result = Image::new(result.width(), result.height(), planes)?;
        } else {
            result = filtered;
        }
    }
    if let Some(points) = &p.point_colors {
        let mut points = points.clone();
        for point in &mut points {
            point.hue_shift = (point.hue_shift * scale).clamp(-360., 360.);
            point.saturation_shift = slider(point.saturation_shift);
            point.luminance_shift = slider(point.luminance_shift);
        }
        for coord in result.coords() {
            let mut tile = result.tile(coord, 0, 1)?;
            crate::color(
                &mut tile,
                &ColorSettings {
                    point_colors: points.clone(),
                    ..Default::default()
                },
            )?;
            result.put(&tile)?;
        }
    }
    if p.curves.is_some() || p.curves_extended.is_some() {
        let curved = crate::tone_extra_image(
            &result,
            &ToneSettings {
                curves: p.curves.clone().unwrap_or_default(),
                curves_extended: p.curves_extended.clone(),
                ..Default::default()
            },
        )?;
        // Curve amount interpolates/extrapolates the rendered curve delta in
        // scene-linear light, avoiding invalid/non-monotone scaled knots.
        result = Image::new(
            result.width(),
            result.height(),
            result
                .planes()
                .iter()
                .zip(curved.planes())
                .map(|(a, b)| a.iter().zip(b).map(|(a, b)| a + scale * (b - a)).collect())
                .collect(),
        )?;
    }
    if let Some([hue, saturation]) = p.color_overlay {
        // A unit-value hue, scaled to the original luminance, then blended.
        // This is Tessera's documented approximation of Adobe local toning.
        let h = hue.rem_euclid(360.) / 60.;
        let x = 1. - (h % 2. - 1.).abs();
        let tint = match h as u32 {
            0 => [1., x, 0.],
            1 => [x, 1., 0.],
            2 => [0., 1., x],
            3 => [0., x, 1.],
            4 => [x, 0., 1.],
            _ => [1., 0., x],
        };
        let tint_luma = crate::luminance(tint);
        let weight = (saturation * scale / 100.).clamp(0., 1.);
        for coord in result.coords() {
            let mut tile = result.tile(coord, 0, 1)?;
            crate::map_rgb(&mut tile, |rgb| {
                let y = crate::luminance(rgb);
                std::array::from_fn(|c| rgb[c] + weight * (tint[c] * y / tint_luma - rgb[c]))
            })?;
            result.put(&tile)?;
        }
    }
    if p.defringe != 0. {
        let mut lens = engine_api::recipe::settings::LensSettings::default();
        lens.defringe_purple.amount = (p.defringe * scale / 5.).clamp(0., 20.);
        lens.defringe_green.amount = lens.defringe_purple.amount;
        result = crate::optics::defringe(&result, &lens)?;
    }
    // Moiré intentionally remains a validated no-op pending frequency analysis.
    Ok(result)
}

/// Blend a locally adjusted image using a finite alpha plane in [0,1].
pub fn blend_local(base: &Image, adjusted: &Image, mask: &[f32]) -> EngineResult<Image> {
    if base.width() != adjusted.width()
        || base.height() != adjusted.height()
        || base.planes().len() != 3
        || adjusted.planes().len() != 3
        || mask.len() != base.planes()[0].len()
        || mask
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
    {
        return Err(EngineError::invalid(
            "locals",
            "RGB dimensions and finite alpha plane must match",
        ));
    }
    Image::new(
        base.width(),
        base.height(),
        base.planes()
            .iter()
            .zip(adjusted.planes())
            .map(|(a, b)| {
                a.iter()
                    .zip(b)
                    .zip(mask)
                    .map(|((&a, &b), &w)| {
                        if w == 0.0 {
                            a
                        } else if w == 1.0 {
                            b
                        } else {
                            a + (b - a) * w
                        }
                    })
                    .collect()
            })
            .collect(),
    )
}

/// Split local Point Color into its own pass.
///
/// Local Point Color is ONE stage in every render path, whether B&W is on or
/// off: after basic Tone and before monochrome conversion and the global point
/// curves (`tone_extra`). It selects on scene colour, so it has to precede the
/// B&W mix, and keeping it there when B&W is off means the global curves act on
/// its result in both modes. Every other local control keeps its position after
/// global colour. Returns `(point-colour groups, remaining groups)`; a group
/// with other controls appears in both, with Point Color only in the first.
/// The Tone stage hash covers the first set (`DevelopSettings::stage_hashes`).
pub fn split_local_point_colors(
    groups: &[LocalAdjustment],
) -> (
    Vec<LocalAdjustment>,
    std::borrow::Cow<'_, [LocalAdjustment]>,
) {
    if !groups.iter().any(|g| g.params.point_colors.is_some()) {
        return (Vec::new(), std::borrow::Cow::Borrowed(groups));
    }
    let mut before = Vec::new();
    let mut after = groups.to_vec();
    for group in &mut after {
        if let Some(points) = group.params.point_colors.take() {
            let mut point_group = group.clone();
            point_group.params = LocalParams {
                point_colors: Some(points),
                ..Default::default()
            };
            before.push(point_group);
        }
    }
    (before, std::borrow::Cow::Owned(after))
}
