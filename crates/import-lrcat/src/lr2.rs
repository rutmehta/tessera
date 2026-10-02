//! Additive catalog translations. Unsupported/approximate source stays lossless.
use crate::lua_develop::{LuaKey, LuaTable, LuaValue};
use engine_api::{
    EngineResult,
    recipe::{
        Recipe,
        settings::{Curve, CurvePoint},
    },
};
use std::collections::BTreeMap;

type Values = BTreeMap<String, LuaValue>;
const BANDS: [&str; 8] = [
    "Red", "Orange", "Yellow", "Green", "Aqua", "Blue", "Purple", "Magenta",
];
const CURVES: [&str; 4] = [
    "ExtendedToneCurvePV2012",
    "ExtendedToneCurvePV2012Red",
    "ExtendedToneCurvePV2012Green",
    "ExtendedToneCurvePV2012Blue",
];

fn relevant(key: &str) -> bool {
    CURVES.contains(&key)
        || key.starts_with("GrayMixer")
        || key.starts_with("AutoToneDigest")
        || matches!(
            key,
            "ConvertToGrayscale"
                | "DepthMapInfo"
                | "Exposure"
                | "Brightness"
                | "Contrast"
                | "FillLight"
                | "HighlightRecovery"
                | "Recovery"
                | "Shadows"
                | "Blacks"
                | "Exposure2012"
                | "Contrast2012"
                | "Highlights2012"
                | "Shadows2012"
                | "Blacks2012"
        )
}

pub(crate) fn lua(
    table: &LuaTable,
    recipe: &mut Recipe,
    warnings: &mut Vec<String>,
) -> EngineResult<()> {
    let values = table
        .fields
        .iter()
        .filter_map(|(k, v)| match k {
            LuaKey::Str(k) if relevant(k) => Some((k.clone(), v.clone())),
            _ => None,
        })
        .collect();
    apply(&values, recipe, warnings)
}

pub(crate) fn xmp(
    doc: &roxmltree::Document<'_>,
    recipe: &mut Recipe,
    warnings: &mut Vec<String>,
) -> EngineResult<()> {
    // This pass never edits the original fragments or normalization machinery.
    let mut values = Values::new();
    let crs = engine_api::recipe::crs::CRS_NAMESPACE;
    let rdf = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    for desc in doc.descendants().filter(|n| {
        n.has_tag_name((rdf, "Description"))
            && n.parent().is_some_and(|p| p.has_tag_name((rdf, "RDF")))
    }) {
        for a in desc
            .attributes()
            .filter(|a| a.namespace() == Some(crs) && relevant(a.name()))
        {
            values.insert(a.name().into(), scalar(a.value()));
        }
        for n in desc.children().filter(|n| {
            n.is_element() && n.tag_name().namespace() == Some(crs) && relevant(n.tag_name().name())
        }) {
            let value = if CURVES.contains(&n.tag_name().name()) {
                let mut items = Vec::new();
                for li in n.descendants().filter(|n| n.has_tag_name((rdf, "li"))) {
                    items.extend(li.text().unwrap_or("").split(',').map(|s| scalar(s.trim())));
                }
                LuaValue::Table(LuaTable {
                    items,
                    fields: Vec::new(),
                })
            } else {
                scalar(n.text().unwrap_or(""))
            };
            values.insert(n.tag_name().name().into(), value);
        }
    }
    apply(&values, recipe, warnings)
}

fn scalar(s: &str) -> LuaValue {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" => LuaValue::Bool(true),
        "false" => LuaValue::Bool(false),
        _ if s.trim().parse::<f64>().is_ok() => LuaValue::Number(s.trim().into()),
        _ => LuaValue::String(s.into()),
    }
}
fn number(v: &LuaValue, min: f32, max: f32) -> Option<f32> {
    let LuaValue::Number(s) = v else { return None };
    s.parse::<f32>()
        .ok()
        .filter(|n| n.is_finite() && (min..=max).contains(n))
}
fn curve(v: &LuaValue) -> Option<Curve> {
    let LuaValue::Table(t) = v else { return None };
    if !t.fields.is_empty() || t.items.len() < 4 || !t.items.len().is_multiple_of(2) {
        return None;
    }
    let points: Option<Vec<_>> = t
        .items
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            Some(CurvePoint {
                x: number(&p[0], -f32::MAX, f32::MAX)? / 255.,
                y: number(&p[1], -f32::MAX, f32::MAX)? / 255.,
            })
        })
        .collect();
    let points = points?;
    if points
        .windows(2)
        .any(|p| p[0].x >= p[1].x || p[0].y > p[1].y)
    {
        return None;
    }
    Some(Curve(points))
}
fn apply(values: &Values, recipe: &mut Recipe, warnings: &mut Vec<String>) -> EngineResult<()> {
    // Avoid cloning settings (including masks) on the existing fast path.
    if values.keys().all(|k| {
        matches!(
            k.as_str(),
            "Exposure2012" | "Contrast2012" | "Highlights2012" | "Shadows2012" | "Blacks2012"
        )
    }) {
        return Ok(());
    }
    let mut settings = recipe.settings.clone();
    let mut failed_curve = false;
    let mut approximate = BTreeMap::<String, &str>::new();
    let mut curves = engine_api::recipe::settings::ToneCurves::default();
    let mut has_extended = false;
    for (key, target) in CURVES.into_iter().zip([
        &mut curves.rgb,
        &mut curves.red,
        &mut curves.green,
        &mut curves.blue,
    ]) {
        if let Some(v) = values.get(key) {
            if let Some(c) = curve(v) {
                // Identity source is provenance only; never replace an ordinary curve.
                if settings.output.hdr && c.0.iter().any(|p| p.x != p.y) {
                    *target = c;
                    has_extended = true;
                    approximate.insert(
                        key.into(),
                        "HDR axis calibration is not verified against Adobe-rendered charts",
                    );
                }
            } else {
                failed_curve = true;
            }
        }
    }
    if has_extended {
        settings.tone.curves_extended = Some(curves);
    }
    if CURVES.iter().any(|k| values.contains_key(*k)) && !failed_curve {
        warnings.retain(|s| s != crate::lua_develop::EXTENDED_TONE_CURVE_NOTE);
    }
    let mut gray = settings.color.monochrome.clone().unwrap_or_default();
    let mut gray_keys = Vec::new();
    if let Some(v) = values.get("ConvertToGrayscale") {
        let enabled = match v {
            LuaValue::Bool(b) => Some(*b),
            _ => number(v, 0., 1.)
                .filter(|n| *n == 0. || *n == 1.)
                .map(|n| n == 1.),
        };
        if let Some(b) = enabled {
            gray.enabled = b;
            gray_keys.push("ConvertToGrayscale".to_string());
        }
    }
    for (band, target) in BANDS.into_iter().zip([
        &mut gray.mixer.red,
        &mut gray.mixer.orange,
        &mut gray.mixer.yellow,
        &mut gray.mixer.green,
        &mut gray.mixer.aqua,
        &mut gray.mixer.blue,
        &mut gray.mixer.purple,
        &mut gray.mixer.magenta,
    ]) {
        let key = format!("GrayMixer{band}");
        if let Some(n) = values.get(&key).and_then(|v| number(v, -100., 100.)) {
            *target = n;
            gray_keys.push(key);
        }
    }
    if !gray_keys.is_empty() && (gray.enabled || gray.mixer != Default::default()) {
        settings.color.monochrome = Some(gray);
        for key in gray_keys {
            approximate.insert(
                key.clone(),
                "profile-dependent B&W response and ordering lack Adobe-rendered calibration",
            );
        }
    }
    if recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe
        && recipe.process_version.revision <= 2
    {
        use engine_api::recipe::settings::LegacyPv2010;
        let n = |k: &str, min, max| values.get(k).and_then(|v| number(v, min, max));
        let legacy = LegacyPv2010 {
            exposure: n("Exposure", -5., 5.),
            brightness: n("Brightness", -150., 150.),
            contrast: n("Contrast", -50., 100.),
            fill_light: n("FillLight", 0., 100.),
            recovery: n("HighlightRecovery", 0., 100.).or_else(|| n("Recovery", 0., 100.)),
            blacks: n("Shadows", 0., 100.).or_else(|| n("Blacks", 0., 100.)),
        };
        for (key, present) in [
            ("Exposure", legacy.exposure.is_some()),
            ("Brightness", legacy.brightness.is_some()),
            ("Contrast", legacy.contrast.is_some()),
            ("FillLight", legacy.fill_light.is_some()),
            (
                "HighlightRecovery",
                n("HighlightRecovery", 0., 100.).is_some(),
            ),
            (
                "Recovery",
                n("HighlightRecovery", 0., 100.).is_none() && n("Recovery", 0., 100.).is_some(),
            ),
            ("Shadows", n("Shadows", 0., 100.).is_some()),
            (
                "Blacks",
                n("Shadows", 0., 100.).is_none() && n("Blacks", 0., 100.).is_some(),
            ),
        ] {
            if present && values.contains_key(key) {
                let reason = match key {
                    "Exposure" => {
                        "EV multiplier is implemented; Adobe PV2010 camera/process calibration lacks rendered evidence"
                    }
                    "Brightness" => {
                        "Adobe midtone transfer and slider-to-gain calibration are unpublished"
                    }
                    "Contrast" => {
                        "Adobe PV2010 pivot, transfer and slider-to-slope calibration are unpublished"
                    }
                    "FillLight" => {
                        "scalar reference does not reproduce Adobe's unpublished spatial shadow adaptation"
                    }
                    "HighlightRecovery" | "Recovery" => {
                        "working RGB lacks Adobe's original camera-channel clipping and RAW reconstruction data"
                    }
                    _ => {
                        "public DNG black ramp assumes camera shadow scale and stage gain of one; PV2010 parity is unverified"
                    }
                };
                approximate.insert(key.into(), reason);
            }
        }
        if legacy != LegacyPv2010::default() {
            // PV2010 is authoritative: stale PV2012 controls must not render twice.
            settings.tone.exposure = 0.;
            settings.tone.contrast = 0.;
            settings.tone.shadows = 0.;
            settings.tone.highlights = 0.;
            settings.tone.whites = 0.;
            settings.tone.blacks = 0.;
            settings.tone.legacy_pv2010 = Some(legacy);
        }
    }
    for (key, reason) in &approximate {
        warnings.retain(|w| {
            !w.starts_with(&format!("crs:{key}:")) && !w.starts_with(&format!("{key}:"))
        });
        let field = match key.as_str() {
            "ConvertToGrayscale" => "/settings/color/monochrome/enabled".to_string(),
            k if k.starts_with("GrayMixer") => format!(
                "/settings/color/monochrome/mixer/{}",
                k.trim_start_matches("GrayMixer").to_ascii_lowercase()
            ),
            k if k.starts_with("ExtendedToneCurvePV2012") => {
                let channel = k
                    .trim_start_matches("ExtendedToneCurvePV2012")
                    .to_ascii_lowercase();
                format!(
                    "/settings/tone/curves_extended/{}",
                    if channel.is_empty() { "rgb" } else { &channel }
                )
            }
            k => {
                let member = match k {
                    "Exposure" => "exposure",
                    "Brightness" => "brightness",
                    "Contrast" => "contrast",
                    "FillLight" => "fill_light",
                    "HighlightRecovery" | "Recovery" => "recovery",
                    "Shadows" | "Blacks" => "blacks",
                    _ => unreachable!("only translated LR-2 keys have approximation reasons"),
                };
                format!("/settings/tone/legacy_pv2010/{member}")
            }
        };
        crate::diagnostics::push_approximate(recipe, key, &field, "LR-2", reason);
    }
    for key in values
        .keys()
        .filter(|k| k.starts_with("AutoToneDigest") || k.as_str() == "DepthMapInfo")
    {
        warnings.retain(|w| {
            !w.starts_with(&format!("crs:{key}:")) && !w.starts_with(&format!("{key}:"))
        });
        if key.starts_with("AutoToneDigest") {
            continue;
        }
        warnings.push(format!("{key}: retained metadata, not a pixel adjustment; digest is not an Auto Tone recipe and depth metadata is not a depth raster; source preserved"));
    }
    if settings != recipe.settings {
        // Import has not escaped to callers: rebuild its single initial edit.
        let meta = recipe
            .history
            .entries
            .first()
            .map(|e| e.meta.clone())
            .unwrap_or_else(|| engine_api::recipe::EditMeta {
                label: "Import XMP".into(),
                author: engine_api::recipe::Author::Import {
                    source: "xmp".into(),
                },
                ..Default::default()
            });
        recipe.history = Default::default();
        recipe.settings = Default::default();
        recipe.edit(meta, |s| *s = settings)?;
    }
    Ok(())
}

#[cfg(test)]
#[test]
fn lr2b_native_revision_two_is_not_adobe_pv2010() {
    let mut recipe = Recipe::default();
    let mut values = Values::new();
    values.insert("Exposure".into(), LuaValue::Number("1".into()));
    apply(&values, &mut recipe, &mut Vec::new()).unwrap();
    assert!(recipe.settings.tone.legacy_pv2010.is_none());
}

#[cfg(test)]
#[test]
fn lr2e_apply_leaves_history_for_shared_finish() {
    let mut recipe = Recipe::default();
    let base = recipe.history.base.clone();
    let mut values = Values::new();
    values.insert("ConvertToGrayscale".into(), LuaValue::Bool(true));
    apply(&values, &mut recipe, &mut Vec::new()).unwrap();
    assert!(recipe.settings.color.monochrome.as_ref().unwrap().enabled);
    assert_eq!(recipe.history.base, base);
    assert!(recipe.history.entries.is_empty());
}
