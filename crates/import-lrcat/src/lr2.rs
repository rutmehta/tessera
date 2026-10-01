//! Additive catalog translations. Unsupported/approximate source stays lossless.
use crate::lua_develop::{LuaKey, LuaTable, LuaValue};
use engine_api::{
    EngineResult,
    recipe::{
        Author, EditMeta, Recipe,
        settings::{Curve, CurvePoint},
    },
};
use std::collections::{BTreeMap, BTreeSet};

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
                x: number(&p[0], 0., 255.)? / 255.,
                y: number(&p[1], 0., 255.)? / 255.,
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
    let mut translated = BTreeSet::<String>::new();
    let mut failed_curve = false;
    for (key, target) in CURVES.into_iter().zip([
        &mut settings.tone.curves.rgb,
        &mut settings.tone.curves.red,
        &mut settings.tone.curves.green,
        &mut settings.tone.curves.blue,
    ]) {
        if let Some(v) = values.get(key) {
            if let Some(c) = curve(v) {
                *target = c;
                translated.insert(key.into());
            } else {
                failed_curve = true;
            }
        }
    }
    if CURVES.iter().any(|k| values.contains_key(*k)) && !failed_curve {
        warnings.retain(|s| s != crate::lua_develop::EXTENDED_TONE_CURVE_NOTE);
    }
    let mut gray = settings.color.monochrome.clone().unwrap_or_default();
    let mut has_gray = false;
    if let Some(v) = values.get("ConvertToGrayscale") {
        let enabled = match v {
            LuaValue::Bool(b) => Some(*b),
            _ => number(v, 0., 1.)
                .filter(|n| *n == 0. || *n == 1.)
                .map(|n| n == 1.),
        };
        if let Some(b) = enabled {
            gray.enabled = b;
            has_gray = true;
            translated.insert("ConvertToGrayscale".into());
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
            has_gray = true;
            translated.insert(key);
        }
    }
    if has_gray {
        if gray.enabled && !warnings.iter().any(|w| w.starts_with("B&W approximation:")) {
            warnings.push("B&W approximation: hue mixer rendered with linear Rec.2020 luminance; Adobe profile-dependent B&W response is not reproduced exactly".into());
        }
        settings.color.monochrome = Some(gray);
    }
    // PV2010 controls are image-adaptive in Adobe. These are deliberately
    // documented heuristics, not Adobe's unpublished conversion algorithm.
    if recipe.process_version.revision <= 2 {
        let n = |k: &str, min, max| values.get(k).and_then(|v| number(v, min, max));
        let mut approximate = false;
        if !values.contains_key("Exposure2012") {
            let exposure = n("Exposure", -5., 5.);
            let brightness = n("Brightness", -150., 150.);
            if exposure.is_some() || brightness.is_some() {
                settings.tone.exposure = (exposure.unwrap_or(0.)
                    + (brightness.unwrap_or(50.) - 50.) / 50.)
                    .clamp(-10., 10.);
                approximate = true;
            }
        }
        for (legacy, modern, min, max, offset, scale, target) in [
            (
                "Contrast",
                "Contrast2012",
                -50.,
                100.,
                25.,
                1.,
                &mut settings.tone.contrast,
            ),
            (
                "FillLight",
                "Shadows2012",
                0.,
                100.,
                0.,
                1.,
                &mut settings.tone.shadows,
            ),
            (
                "HighlightRecovery",
                "Highlights2012",
                0.,
                100.,
                0.,
                -1.,
                &mut settings.tone.highlights,
            ),
            (
                "Shadows",
                "Blacks2012",
                0.,
                100.,
                0.,
                -1.,
                &mut settings.tone.blacks,
            ),
        ] {
            let alias = match legacy {
                "HighlightRecovery" => "Recovery",
                "Shadows" => "Blacks",
                _ => legacy,
            };
            if !values.contains_key(modern)
                && let Some(v) = n(legacy, min, max).or_else(|| n(alias, min, max))
            {
                *target = ((v - offset) * scale).clamp(-100., 100.);
                approximate = true;
            }
        }
        if approximate
            && !warnings
                .iter()
                .any(|w| w.starts_with("PV2010 approximation:"))
        {
            warnings.push("PV2010 approximation: legacy sliders mapped heuristically; Adobe image-adaptive brightness, recovery and black clipping cannot be reproduced exactly; source preserved".into());
        }
    }
    for key in values
        .keys()
        .filter(|k| k.starts_with("AutoToneDigest") || k.as_str() == "DepthMapInfo")
    {
        warnings.retain(|w| {
            !w.starts_with(&format!("crs:{key}:")) && !w.starts_with(&format!("{key}:"))
        });
        warnings.push(format!("{key}: retained metadata, not a pixel adjustment; digest is not an Auto Tone recipe and depth metadata is not a depth raster; source preserved"));
    }
    if !translated.is_empty() {
        warnings.retain(|w| {
            !translated
                .iter()
                .any(|k| w.starts_with(&format!("crs:{k}:")) || w.starts_with(&format!("{k}:")))
        });
        for name in ["lrcat_develop_source", "lrcat_develop_lua"] {
            if let Some(value) = recipe.unknown.get_mut(name) {
                let map = if name == "lrcat_develop_source" {
                    value.get_mut("properties").and_then(|v| v.as_object_mut())
                } else {
                    value.as_object_mut()
                };
                if let Some(map) = map {
                    for key in &translated {
                        map.remove(key);
                    }
                    if map.is_empty() {
                        recipe.unknown.remove(name);
                    }
                }
            }
        }
        if let Some(entries) = recipe
            .unknown
            .get_mut("lrcat_develop_lua_entries")
            .and_then(|v| v.as_array_mut())
        {
            entries.retain(|v| {
                !v["key"]["string"]
                    .as_str()
                    .is_some_and(|k| translated.contains(k))
            });
            if entries.is_empty() {
                recipe.unknown.remove("lrcat_develop_lua_entries");
            }
        }
        for key in &translated {
            recipe.unknown.remove(&format!("crs:{key}"));
        }
    }
    if settings != recipe.settings {
        recipe.edit(
            EditMeta {
                label: "Import LR-2 develop settings".into(),
                author: Author::Import {
                    source: "lrcat".into(),
                },
                ..Default::default()
            },
            |s| *s = settings,
        )?;
    }
    Ok(())
}
