//! The host-facing filter catalogue (WP B5-05): every menu filter with its
//! group, display name and parameter schema, and the mapping from the
//! schema's user-facing values (Photoshop units: pixels, degrees, percent,
//! 0–255 levels) to an [`Effect`] with [`FilterParams`].
//!
//! Hosts build their Filter menu and dialogs from [`list`]; nothing about a
//! filter's controls is hard-coded in the app. Values arrive as
//! [`ParamValue`]s keyed by [`ParamSpec::key`]; missing keys take the
//! default. [`build`] takes a `scale`: previews render on a pyramid level
//! (`scale = 2^level`), where every *spatial* value (pixel radii, distances,
//! wavelengths, offsets) is divided by the scale so the preview matches the
//! full-resolution result.
//!
//! This crate has no serde; [`FilterInfo::schema_json`] writes the schema by
//! hand (the format is documented there).

use crate::{Effect, FilterParams, distort::Distortion};
use engine_api::{EngineError, EngineResult};
use std::collections::BTreeMap;

/// Menu groups, in menu order.
pub const GROUPS: [&str; 7] = [
    "Blur", "Sharpen", "Noise", "Distort", "Stylize", "Render", "Other",
];

/// What control a parameter gets.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamKind {
    /// A number slider.
    Slider {
        min: f64,
        max: f64,
        default: f64,
        step: f64,
        /// Display unit: `px`, `%`, `°`, `levels` or empty.
        unit: &'static str,
        /// Divided by the preview scale (pixel distances).
        spatial: bool,
        /// Whole numbers only.
        integer: bool,
    },
    /// An angle dial, degrees.
    Angle { min: f64, max: f64, default: f64 },
    /// One of several named options: `(value, label)`.
    Choice {
        options: &'static [(&'static str, &'static str)],
        default: &'static str,
    },
    /// A point in normalized canvas coordinates (0…1 each).
    Point { default: [f64; 2] },
    /// A checkbox.
    Toggle { default: bool },
}

/// One control of a filter dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct ParamSpec {
    pub key: &'static str,
    pub label: &'static str,
    pub kind: ParamKind,
}

/// A value for one parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum ParamValue {
    Number(f64),
    Bool(bool),
    Text(String),
    Point([f64; 2]),
}

/// One menu filter.
#[derive(Clone, Debug, PartialEq)]
pub struct FilterInfo {
    /// Stable snake_case id (`gaussian_blur`).
    pub id: &'static str,
    /// One of [`GROUPS`].
    pub group: &'static str,
    /// Menu title (`Gaussian Blur`).
    pub name: &'static str,
    pub params: Vec<ParamSpec>,
}

#[allow(clippy::too_many_arguments)]
const fn slider(
    key: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    default: f64,
    step: f64,
    unit: &'static str,
    spatial: bool,
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        kind: ParamKind::Slider {
            min,
            max,
            default,
            step,
            unit,
            spatial,
            integer: false,
        },
    }
}

const fn integer(
    key: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    default: f64,
    unit: &'static str,
    spatial: bool,
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        kind: ParamKind::Slider {
            min,
            max,
            default,
            step: 1.0,
            unit,
            spatial,
            integer: true,
        },
    }
}

const fn angle(
    key: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
    default: f64,
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        kind: ParamKind::Angle { min, max, default },
    }
}

const fn choice(
    key: &'static str,
    label: &'static str,
    options: &'static [(&'static str, &'static str)],
    default: &'static str,
) -> ParamSpec {
    ParamSpec {
        key,
        label,
        kind: ParamKind::Choice { options, default },
    }
}

const fn toggle(key: &'static str, label: &'static str, default: bool) -> ParamSpec {
    ParamSpec {
        key,
        label,
        kind: ParamKind::Toggle { default },
    }
}

const fn point(key: &'static str, label: &'static str) -> ParamSpec {
    ParamSpec {
        key,
        label,
        kind: ParamKind::Point {
            default: [0.5, 0.5],
        },
    }
}

const RADIAL_METHODS: &[(&str, &str)] = &[("spin", "Spin"), ("zoom", "Zoom")];
const QUALITY: &[(&str, &str)] = &[("draft", "Draft"), ("good", "Good"), ("best", "Best")];
const DISTRIBUTION: &[(&str, &str)] = &[("uniform", "Uniform"), ("gaussian", "Gaussian")];
const RIPPLE_SIZE: &[(&str, &str)] =
    &[("small", "Small"), ("medium", "Medium"), ("large", "Large")];
const POLAR_MODES: &[(&str, &str)] = &[
    ("rect_to_polar", "Rectangular to Polar"),
    ("polar_to_rect", "Polar to Rectangular"),
];

/// Every menu filter, grouped in [`GROUPS`] order. Lens Blur (needs a depth
/// map), Oil Paint and Lens Flare (explicit placeholders in this crate) and
/// Camera Raw (needs a host processor) are not listed.
pub fn list() -> Vec<FilterInfo> {
    let f = |id, group, name, params| FilterInfo {
        id,
        group,
        name,
        params,
    };
    vec![
        // Blur
        f(
            "gaussian_blur",
            "Blur",
            "Gaussian Blur",
            vec![slider("radius", "Radius", 0.1, 250.0, 2.0, 0.1, "px", true)],
        ),
        f(
            "box_blur",
            "Blur",
            "Box Blur",
            vec![slider("radius", "Radius", 1.0, 250.0, 5.0, 1.0, "px", true)],
        ),
        f(
            "motion_blur",
            "Blur",
            "Motion Blur",
            vec![
                angle("angle", "Angle", -90.0, 90.0, 0.0),
                slider("distance", "Distance", 1.0, 500.0, 10.0, 1.0, "px", true),
            ],
        ),
        f(
            "radial_blur",
            "Blur",
            "Radial Blur",
            vec![
                integer("amount", "Amount", 1.0, 100.0, 10.0, "", false),
                choice("method", "Blur method", RADIAL_METHODS, "spin"),
                choice("quality", "Quality", QUALITY, "good"),
            ],
        ),
        f(
            "surface_blur",
            "Blur",
            "Surface Blur",
            vec![
                slider("radius", "Radius", 1.0, 100.0, 5.0, 1.0, "px", true),
                integer("threshold", "Threshold", 2.0, 255.0, 15.0, "levels", false),
            ],
        ),
        // Sharpen
        f(
            "unsharp_mask",
            "Sharpen",
            "Unsharp Mask",
            vec![
                integer("amount", "Amount", 1.0, 500.0, 100.0, "%", false),
                slider("radius", "Radius", 0.1, 250.0, 1.0, 0.1, "px", true),
                integer("threshold", "Threshold", 0.0, 255.0, 0.0, "levels", false),
            ],
        ),
        f(
            "smart_sharpen",
            "Sharpen",
            "Smart Sharpen",
            vec![
                integer("amount", "Amount", 1.0, 150.0, 100.0, "%", false),
                slider("radius", "Radius", 0.5, 3.0, 1.0, 0.1, "px", true),
            ],
        ),
        // Noise
        f(
            "add_noise",
            "Noise",
            "Add Noise",
            vec![
                slider("amount", "Amount", 0.1, 100.0, 10.0, 0.1, "%", false),
                choice("distribution", "Distribution", DISTRIBUTION, "uniform"),
                toggle("monochromatic", "Monochromatic", false),
                integer("seed", "Seed", 0.0, 9999.0, 1.0, "", false),
            ],
        ),
        f(
            "reduce_noise",
            "Noise",
            "Reduce Noise",
            vec![integer("strength", "Strength", 0.0, 10.0, 6.0, "", false)],
        ),
        f(
            "median",
            "Noise",
            "Median",
            vec![integer("radius", "Radius", 1.0, 100.0, 1.0, "px", true)],
        ),
        f(
            "dust_and_scratches",
            "Noise",
            "Dust & Scratches",
            vec![
                integer("radius", "Radius", 1.0, 100.0, 1.0, "px", true),
                integer("threshold", "Threshold", 0.0, 255.0, 0.0, "levels", false),
            ],
        ),
        // Distort
        f(
            "pinch",
            "Distort",
            "Pinch",
            vec![
                integer("amount", "Amount", -100.0, 100.0, 50.0, "%", false),
                point("center", "Center"),
            ],
        ),
        f(
            "spherize",
            "Distort",
            "Spherize",
            vec![
                integer("amount", "Amount", -100.0, 100.0, 100.0, "%", false),
                point("center", "Center"),
            ],
        ),
        f(
            "twirl",
            "Distort",
            "Twirl",
            vec![
                angle("angle", "Angle", -999.0, 999.0, 50.0),
                point("center", "Center"),
            ],
        ),
        f(
            "wave",
            "Distort",
            "Wave",
            vec![
                slider("amplitude", "Amplitude", 1.0, 999.0, 10.0, 1.0, "px", true),
                slider(
                    "wavelength",
                    "Wavelength",
                    2.0,
                    999.0,
                    120.0,
                    1.0,
                    "px",
                    true,
                ),
                angle("phase", "Phase", 0.0, 360.0, 0.0),
            ],
        ),
        f(
            "ripple",
            "Distort",
            "Ripple",
            vec![
                integer("amount", "Amount", -100.0, 100.0, 50.0, "%", false),
                choice("size", "Size", RIPPLE_SIZE, "medium"),
            ],
        ),
        f(
            "polar_coordinates",
            "Distort",
            "Polar Coordinates",
            vec![choice("mode", "Mode", POLAR_MODES, "rect_to_polar")],
        ),
        // Stylize
        f(
            "emboss",
            "Stylize",
            "Emboss",
            vec![integer("amount", "Amount", 1.0, 500.0, 100.0, "%", false)],
        ),
        f("find_edges", "Stylize", "Find Edges", vec![]),
        f(
            "solarize",
            "Stylize",
            "Solarize",
            vec![integer(
                "threshold",
                "Threshold",
                0.0,
                255.0,
                128.0,
                "levels",
                false,
            )],
        ),
        // Render
        f(
            "clouds",
            "Render",
            "Clouds",
            vec![
                slider("scale", "Scale", 8.0, 250.0, 128.0, 1.0, "px", true),
                integer("seed", "Seed", 0.0, 9999.0, 1.0, "", false),
            ],
        ),
        f(
            "difference_clouds",
            "Render",
            "Difference Clouds",
            vec![
                slider("scale", "Scale", 8.0, 250.0, 128.0, 1.0, "px", true),
                integer("seed", "Seed", 0.0, 9999.0, 1.0, "", false),
            ],
        ),
        // Other
        f(
            "high_pass",
            "Other",
            "High Pass",
            vec![slider(
                "radius", "Radius", 0.1, 250.0, 10.0, 0.1, "px", true,
            )],
        ),
        f(
            "offset",
            "Other",
            "Offset",
            vec![
                integer(
                    "horizontal",
                    "Horizontal",
                    -30000.0,
                    30000.0,
                    0.0,
                    "px",
                    true,
                ),
                integer("vertical", "Vertical", -30000.0, 30000.0, 0.0, "px", true),
            ],
        ),
    ]
}

/// The catalogue entry for `id`.
pub fn find(id: &str) -> Option<FilterInfo> {
    list().into_iter().find(|f| f.id == id)
}

fn json_str(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 2);
    o.push('"');
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            c if (c as u32) < 0x20 => o.push_str(&format!("\\u{:04x}", c as u32)),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

fn json_num(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

impl FilterInfo {
    /// The parameter schema as JSON:
    ///
    /// ```json
    /// {"params":[
    ///   {"key":"radius","label":"Radius","kind":"slider","min":0.1,"max":250,
    ///    "default":2,"step":0.1,"unit":"px","spatial":true,"integer":false},
    ///   {"key":"angle","label":"Angle","kind":"angle","min":-90,"max":90,"default":0},
    ///   {"key":"method","label":"Blur method","kind":"choice",
    ///    "options":[{"value":"spin","label":"Spin"}],"default":"spin"},
    ///   {"key":"center","label":"Center","kind":"point","default":[0.5,0.5]},
    ///   {"key":"monochromatic","label":"Monochromatic","kind":"toggle","default":false}]}
    /// ```
    pub fn schema_json(&self) -> String {
        let params: Vec<String> = self
            .params
            .iter()
            .map(|p| {
                let head = format!(
                    "\"key\":{},\"label\":{}",
                    json_str(p.key),
                    json_str(p.label)
                );
                let body = match &p.kind {
                    ParamKind::Slider {
                        min,
                        max,
                        default,
                        step,
                        unit,
                        spatial,
                        integer,
                    } => format!(
                        "\"kind\":\"slider\",\"min\":{},\"max\":{},\"default\":{},\"step\":{},\"unit\":{},\"spatial\":{spatial},\"integer\":{integer}",
                        json_num(*min),
                        json_num(*max),
                        json_num(*default),
                        json_num(*step),
                        json_str(unit)
                    ),
                    ParamKind::Angle { min, max, default } => format!(
                        "\"kind\":\"angle\",\"min\":{},\"max\":{},\"default\":{}",
                        json_num(*min),
                        json_num(*max),
                        json_num(*default)
                    ),
                    ParamKind::Choice { options, default } => format!(
                        "\"kind\":\"choice\",\"options\":[{}],\"default\":{}",
                        options
                            .iter()
                            .map(|(v, l)| format!(
                                "{{\"value\":{},\"label\":{}}}",
                                json_str(v),
                                json_str(l)
                            ))
                            .collect::<Vec<_>>()
                            .join(","),
                        json_str(default)
                    ),
                    ParamKind::Point { default } => format!(
                        "\"kind\":\"point\",\"default\":[{},{}]",
                        json_num(default[0]),
                        json_num(default[1])
                    ),
                    ParamKind::Toggle { default } => {
                        format!("\"kind\":\"toggle\",\"default\":{default}")
                    }
                };
                format!("{{{head},{body}}}")
            })
            .collect();
        format!("{{\"params\":[{}]}}", params.join(","))
    }
}

/// Resolved, validated values of one filter (defaults filled in).
struct Values<'a> {
    info: &'a FilterInfo,
    given: &'a BTreeMap<String, ParamValue>,
    scale: f64,
}

impl Values<'_> {
    fn spec(&self, key: &str) -> EngineResult<&ParamSpec> {
        self.info
            .params
            .iter()
            .find(|p| p.key == key)
            .ok_or_else(|| EngineError::internal(format!("{}: no parameter {key}", self.info.id)))
    }

    /// A slider or angle value in its schema range, divided by the scale
    /// when spatial.
    fn num(&self, key: &str) -> EngineResult<f64> {
        let spec = self.spec(key)?;
        let (min, max, default, spatial, int) = match spec.kind {
            ParamKind::Slider {
                min,
                max,
                default,
                spatial,
                integer,
                ..
            } => (min, max, default, spatial, integer),
            ParamKind::Angle { min, max, default } => (min, max, default, false, false),
            _ => return Err(EngineError::internal(format!("{key} is not a number"))),
        };
        let v = match self.given.get(key) {
            None => default,
            Some(ParamValue::Number(v)) => *v,
            Some(other) => {
                return Err(EngineError::invalid(
                    key,
                    format!("expected a number, got {other:?}"),
                ));
            }
        };
        if !v.is_finite() || v < min - 1e-9 || v > max + 1e-9 {
            return Err(EngineError::invalid(
                key,
                format!("{v} is outside {min}…{max}"),
            ));
        }
        let v = if int { v.round() } else { v };
        Ok(if spatial { v / self.scale } else { v })
    }

    fn text(&self, key: &str) -> EngineResult<&str> {
        let ParamKind::Choice { options, default } = self.spec(key)?.kind else {
            return Err(EngineError::internal(format!("{key} is not a choice")));
        };
        let v = match self.given.get(key) {
            None => default,
            Some(ParamValue::Text(s)) => s.as_str(),
            Some(other) => {
                return Err(EngineError::invalid(
                    key,
                    format!("expected an option, got {other:?}"),
                ));
            }
        };
        options
            .iter()
            .find(|(o, _)| *o == v)
            .map(|(o, _)| *o)
            .ok_or_else(|| EngineError::invalid(key, format!("unknown option {v:?}")))
    }

    fn flag(&self, key: &str) -> EngineResult<bool> {
        let ParamKind::Toggle { default } = self.spec(key)?.kind else {
            return Err(EngineError::internal(format!("{key} is not a toggle")));
        };
        match self.given.get(key) {
            None => Ok(default),
            Some(ParamValue::Bool(b)) => Ok(*b),
            Some(other) => Err(EngineError::invalid(
                key,
                format!("expected true/false, got {other:?}"),
            )),
        }
    }

    fn point(&self, key: &str) -> EngineResult<[f32; 2]> {
        let ParamKind::Point { default } = self.spec(key)?.kind else {
            return Err(EngineError::internal(format!("{key} is not a point")));
        };
        let p = match self.given.get(key) {
            None => default,
            Some(ParamValue::Point(p)) => *p,
            Some(other) => {
                return Err(EngineError::invalid(
                    key,
                    format!("expected [x, y], got {other:?}"),
                ));
            }
        };
        if p.iter().any(|v| !v.is_finite() || !(0.0..=1.0).contains(v)) {
            return Err(EngineError::invalid(key, "point must be in 0…1"));
        }
        Ok([p[0] as f32, p[1] as f32])
    }
}

/// The effect and parameters of filter `id` with `values` (missing keys take
/// their defaults), for rendering at `scale` (1 = full resolution, `2^level`
/// on pyramid level `level`). Values outside their schema range, unknown
/// options and wrong types are errors; unknown keys are ignored.
pub fn build(
    id: &str,
    values: &BTreeMap<String, ParamValue>,
    scale: f32,
) -> EngineResult<(Effect, FilterParams)> {
    let info =
        find(id).ok_or_else(|| EngineError::invalid("filter", format!("unknown filter {id:?}")))?;
    if !scale.is_finite() || scale < 1.0 {
        return Err(EngineError::invalid("scale", "must be at least 1"));
    }
    let v = Values {
        info: &info,
        given: values,
        scale: f64::from(scale),
    };
    let mut p = FilterParams {
        amount: 1.0,
        ..Default::default()
    };
    let deg = |d: f64| d.to_radians() as f32;
    let effect = match id {
        "gaussian_blur" => {
            p.radius = v.num("radius")? as f32;
            Effect::Gaussian
        }
        "box_blur" => {
            p.radius = v.num("radius")? as f32;
            Effect::Box
        }
        "motion_blur" => {
            // The kernel spans ±radius along the direction; Photoshop's
            // distance is the whole length. Angles are counter-clockwise on
            // screen (image y points down).
            p.radius = (v.num("distance")? / 2.0).min(250.0) as f32;
            p.angle = -deg(v.num("angle")?);
            Effect::Motion
        }
        "radial_blur" => {
            let amount = v.num("amount")?;
            p.radius = match v.text("quality")? {
                "draft" => 4.0,
                "best" => 32.0,
                _ => 16.0,
            };
            if v.text("method")? == "zoom" {
                p.angle = (amount / 100.0 * 0.5) as f32;
                Effect::RadialZoom
            } else {
                p.angle = deg(amount * 0.3);
                Effect::RadialSpin
            }
        }
        "surface_blur" => {
            p.radius = v.num("radius")? as f32;
            p.threshold = (v.num("threshold")? / 255.0) as f32;
            Effect::SurfaceBlur
        }
        "unsharp_mask" => {
            p.strength = (v.num("amount")? / 100.0) as f32;
            p.radius = v.num("radius")? as f32;
            p.threshold = (v.num("threshold")? / 255.0) as f32;
            Effect::UnsharpMask
        }
        "smart_sharpen" => {
            p.strength = (v.num("amount")? / 100.0) as f32;
            p.radius = v.num("radius")?.clamp(0.5, 3.0) as f32;
            Effect::SmartSharpen
        }
        "add_noise" => {
            p.strength = (v.num("amount")? / 100.0 * 0.5) as f32;
            p.gaussian_noise = v.text("distribution")? == "gaussian";
            p.monochrome = v.flag("monochromatic")?;
            p.seed = v.num("seed")? as u32;
            Effect::AddNoise
        }
        "reduce_noise" => {
            p.strength = (v.num("strength")? / 10.0) as f32;
            p.radius = 1.0;
            Effect::ReduceNoise
        }
        "median" => {
            p.radius = v.num("radius")?.max(0.5) as f32;
            Effect::Median
        }
        "dust_and_scratches" => {
            p.radius = v.num("radius")?.max(0.5) as f32;
            p.threshold = (v.num("threshold")? / 255.0) as f32;
            Effect::DustScratches
        }
        "pinch" | "spherize" => {
            p.distort.amount = (v.num("amount")? / 100.0) as f32;
            p.distort.center = v.point("center")?;
            Effect::Distort(if id == "pinch" {
                Distortion::Pinch
            } else {
                Distortion::Spherize
            })
        }
        "twirl" => {
            p.distort.amount = deg(v.num("angle")?);
            p.distort.center = v.point("center")?;
            Effect::Distort(Distortion::Twirl)
        }
        "wave" => {
            p.distort.amount = v.num("amplitude")? as f32;
            p.distort.wavelength = v.num("wavelength")? as f32;
            p.distort.phase = deg(v.num("phase")?);
            Effect::Distort(Distortion::Wave)
        }
        "ripple" => {
            let wavelength = match v.text("size")? {
                "small" => 30.0,
                "large" => 120.0,
                _ => 60.0,
            } / f64::from(scale);
            // Kept inside the invertibility bound |a| < λ / 4π.
            let bound = wavelength / (4.0 * std::f64::consts::PI) * 0.95;
            p.distort.wavelength = wavelength as f32;
            p.distort.amount = (v.num("amount")? / 100.0 * bound) as f32;
            Effect::Distort(Distortion::Ripple)
        }
        "polar_coordinates" => Effect::Distort(if v.text("mode")? == "polar_to_rect" {
            Distortion::PolarToRectangular
        } else {
            Distortion::RectangularToPolar
        }),
        "emboss" => {
            p.strength = (v.num("amount")? / 100.0) as f32;
            Effect::Emboss
        }
        "find_edges" => Effect::FindEdges,
        "solarize" => {
            p.threshold = (v.num("threshold")? / 255.0) as f32;
            Effect::Solarize
        }
        "clouds" | "difference_clouds" => {
            p.radius = v.num("scale")?.max(1.0) as f32;
            p.seed = v.num("seed")? as u32;
            if id == "clouds" {
                Effect::Clouds
            } else {
                Effect::DifferenceClouds
            }
        }
        "high_pass" => {
            p.radius = v.num("radius")? as f32;
            Effect::HighPass
        }
        "offset" => {
            p.distort.offset = [v.num("horizontal")? as f32, v.num("vertical")? as f32];
            Effect::Distort(Distortion::Offset)
        }
        other => {
            return Err(EngineError::internal(format!(
                "catalogue filter {other} has no mapping"
            )));
        }
    };
    crate::validate(&p)?;
    Ok((effect, p))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_filter_builds_with_defaults_and_groups_are_known() {
        let mut ids = std::collections::BTreeSet::new();
        for f in list() {
            assert!(GROUPS.contains(&f.group), "{}", f.id);
            assert!(ids.insert(f.id), "duplicate {}", f.id);
            build(f.id, &BTreeMap::new(), 1.0).unwrap_or_else(|e| panic!("{}: {e}", f.id));
            build(f.id, &BTreeMap::new(), 4.0).unwrap_or_else(|e| panic!("{} @4: {e}", f.id));
            let schema = f.schema_json();
            assert!(schema.starts_with("{\"params\":["), "{schema}");
        }
        assert!(ids.len() >= 20);
    }

    #[test]
    fn spatial_values_scale_and_ranges_are_enforced() {
        let mut v = BTreeMap::new();
        v.insert("radius".to_owned(), ParamValue::Number(8.0));
        let (e, p) = build("gaussian_blur", &v, 4.0).unwrap();
        assert_eq!(e, Effect::Gaussian);
        assert_eq!(p.radius, 2.0);
        assert_eq!(p.amount, 1.0);
        v.insert("radius".to_owned(), ParamValue::Number(300.0));
        assert!(build("gaussian_blur", &v, 1.0).is_err());
        v.insert("radius".to_owned(), ParamValue::Text("x".into()));
        assert!(build("gaussian_blur", &v, 1.0).is_err());
        assert!(build("no_such_filter", &BTreeMap::new(), 1.0).is_err());
        let mut m = BTreeMap::new();
        m.insert("method".to_owned(), ParamValue::Text("zoom".into()));
        assert_eq!(build("radial_blur", &m, 1.0).unwrap().0, Effect::RadialZoom);
        m.insert("method".to_owned(), ParamValue::Text("warp".into()));
        assert!(build("radial_blur", &m, 1.0).is_err());
    }
}
