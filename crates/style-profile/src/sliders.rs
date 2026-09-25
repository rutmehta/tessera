//! Explicit base-edit allowlist. Curves, LUTs, masks, geometry and detail are never touched.
use engine_api::{
    recipe::{settings::WhiteBalanceMode, DevelopSettings},
    EngineError, EngineResult,
};

#[derive(Clone)]
pub(crate) struct Slider {
    pub path: String,
    pub min: f64,
    pub max: f64,
    pub default: f64,
}
impl Slider {
    pub fn range(&self) -> f64 {
        self.max - self.min
    }
}
pub(crate) fn sliders() -> Vec<Slider> {
    let defaults = serde_json::to_value(DevelopSettings::default()).expect("settings serialize");
    let mut specs = vec![
        ("/white_balance/temperature".into(), 2000., 50000.),
        ("/white_balance/tint".into(), -150., 150.),
    ];
    for name in [
        "exposure",
        "contrast",
        "highlights",
        "shadows",
        "whites",
        "blacks",
        "texture",
        "clarity",
        "dehaze",
    ] {
        let limit = if name == "exposure" { 10. } else { 100. };
        specs.push((format!("/tone/{name}"), -limit, limit));
    }
    for name in ["saturation", "vibrance"] {
        specs.push((format!("/color/{name}"), -100., 100.));
    }
    for kind in ["hue", "saturation", "luminance"] {
        for band in [
            "red", "orange", "yellow", "green", "aqua", "blue", "purple", "magenta",
        ] {
            specs.push((format!("/color/hsl/{kind}/{band}"), -100., 100.));
        }
    }
    for wheel in ["shadows", "midtones", "highlights", "global"] {
        for (field, min, max) in [
            ("hue", 0., 360.),
            ("saturation", 0., 100.),
            ("luminance", -100., 100.),
        ] {
            specs.push((format!("/color/grading/{wheel}/{field}"), min, max));
        }
    }
    specs.push(("/color/grading/blending".into(), 0., 100.));
    specs.push(("/color/grading/balance".into(), -100., 100.));
    specs
        .into_iter()
        .map(|(path, min, max)| Slider {
            default: defaults
                .pointer(&path)
                .and_then(|v| v.as_f64())
                .expect("allowlisted numeric field"),
            path,
            min,
            max,
        })
        .collect()
}
pub(crate) fn values(s: &DevelopSettings) -> EngineResult<Vec<f64>> {
    let v = serde_json::to_value(s)?;
    sliders()
        .iter()
        .map(|p| {
            let n = v
                .pointer(&p.path)
                .and_then(|v| v.as_f64())
                .ok_or_else(|| EngineError::invalid(&p.path, "nonfinite slider"))?;
            if !(p.min..=p.max).contains(&n) {
                return Err(EngineError::invalid(&p.path, "slider outside range"));
            }
            Ok((n - p.default) / p.range())
        })
        .collect()
}
pub(crate) fn overlay(base: &DevelopSettings, deltas: &[f64]) -> EngineResult<DevelopSettings> {
    let specs = sliders();
    if deltas.len() != specs.len() || deltas.iter().any(|v| !v.is_finite()) {
        return Err(EngineError::invalid(
            "sliders",
            "invalid dimensions or nonfinite",
        ));
    }
    let mut v = serde_json::to_value(base)?;
    for (s, d) in specs.iter().zip(deltas) {
        *v.pointer_mut(&s.path).expect("allowlist") =
            serde_json::json!((s.default + d * s.range()).clamp(s.min, s.max));
    }
    let mut out: DevelopSettings = serde_json::from_value(v)?;
    // Explicit base predictions must affect rendering even at default numeric WB;
    // retaining AsShot/Daylight would silently ignore the learned slider values.
    out.white_balance.mode = WhiteBalanceMode::Custom;
    Ok(out)
}
