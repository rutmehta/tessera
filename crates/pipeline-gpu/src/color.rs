use engine_api::{EngineError, EngineResult, recipe::settings::ColorSettings};

pub(crate) fn parameters(s: &ColorSettings, p: &mut Vec<f32>) -> EngineResult<()> {
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
    Ok(())
}
