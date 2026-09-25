use engine_api::{EngineError, EngineResult, recipe::settings::ColorSettings};

thread_local! {
    static CONSTANTS: std::cell::RefCell<crate::fused::ConstantsCache<ColorSettings>> = std::cell::RefCell::new(Default::default());
}

pub(crate) fn parameters(s: &ColorSettings, p: &mut Vec<f32>) -> EngineResult<()> {
    let prepared = CONSTANTS.with(|cache| {
        cache.borrow_mut().get_or_try_insert(s, || {
            let mut prepared = vec![0.; 33];
            build_parameters(s, &mut prepared)?;
            Ok(prepared)
        })
    })?;
    p[0] = prepared[0];
    p.truncate(9);
    p.extend_from_slice(&prepared[9..]);
    Ok(())
}
fn build_parameters(s: &ColorSettings, p: &mut Vec<f32>) -> EngineResult<()> {
    if !s.point_colors.is_empty() || s.lut.is_some() {
        return Err(EngineError::invalid(
            "color",
            "Point Color and LUT are not implemented in M2",
        ));
    }
    p[0] = 6.0;
    p[9] = if s == &ColorSettings::default() {
        0.0
    } else {
        1.0
    };
    p.extend([
        s.vibrance,
        s.saturation,
        s.grading.balance,
        s.grading.blending,
    ]);
    for b in [&s.hsl.hue, &s.hsl.saturation, &s.hsl.luminance] {
        p.extend([
            b.red, b.orange, b.yellow, b.green, b.aqua, b.blue, b.purple, b.magenta,
        ]);
    }
    for w in [
        &s.grading.shadows,
        &s.grading.midtones,
        &s.grading.highlights,
        &s.grading.global,
    ] {
        p.extend([w.hue, w.saturation, w.luminance]);
    }
    if p[33..].iter().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid("color", "finite parameters required"));
    }
    // Wheel directions are image constants, not per-pixel trigonometry.
    for w in [
        &s.grading.shadows,
        &s.grading.midtones,
        &s.grading.highlights,
        &s.grading.global,
    ] {
        let angle = (w.hue - (w.hue / 360.).floor() * 360.).to_radians();
        let (sin, cos) = angle.sin_cos();
        p.extend([cos, sin]);
    }
    Ok(())
}
