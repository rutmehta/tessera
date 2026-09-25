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
    if p.defringe != 0.0 || p.color_overlay.is_some() {
        return Err(EngineError::invalid(
            "locals",
            "defringe and colour overlay are not implemented",
        ));
    }
    if amount == 0.0 || values.iter().all(|v| *v == 0.0) {
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
