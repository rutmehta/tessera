//! Catalog-only Adobe geometry extension. Consume only successfully translated
//! values; solver metadata, inactive solutions and malformed inputs stay exact.
use engine_api::{
    EngineResult,
    recipe::{
        Recipe,
        history::EditMeta,
        settings::{GuideLine, UprightMode},
    },
};
use std::collections::{BTreeMap, BTreeSet};

fn numbers<const N: usize>(raw: &str) -> Option<[f64; N]> {
    let values = raw
        .split(',')
        .map(|v| v.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let values: [f64; N] = values.try_into().ok()?;
    values.iter().all(|v| v.is_finite()).then_some(values)
}

pub(crate) fn apply<'a>(
    recipe: &mut Recipe,
    warnings: &mut Vec<String>,
    properties: impl Iterator<Item = (&'a str, &'a str)>,
) -> EngineResult<()> {
    let properties: BTreeMap<_, _> = properties
        .filter(|(key, _)| {
            key.starts_with("UprightTransform_")
                || key.starts_with("UprightFourSegments")
                || matches!(
                    *key,
                    "EnableDistractionRemoval" | "GenerativeRemove" | "GenerativeFill"
                )
        })
        .collect();
    if properties.is_empty() {
        return Ok(());
    }
    let mut upright = recipe.settings.geometry.upright.clone();
    let mut consumed = BTreeSet::new();
    let mut notes = Vec::new();
    let index = match upright.mode {
        UprightMode::Off => 0,
        UprightMode::Auto => 1,
        UprightMode::Full => 2,
        UprightMode::Level => 3,
        UprightMode::Vertical => 4,
        UprightMode::Guided => 5,
    };
    let selected = format!("UprightTransform_{index}");
    if index != 0
        && let Some(raw) = properties.get(selected.as_str())
    {
        let matrix =
            numbers::<9>(raw).map(|n| [[n[0], n[1], n[2]], [n[3], n[4], n[5]], [n[6], n[7], n[8]]]);
        if let Some(matrix) = matrix.filter(engine_api::recipe::settings::Upright::valid_homography)
        {
            upright.homography = Some(matrix);
            consumed.insert(selected.clone());
        } else {
            notes.push(format!("{selected}: invalid or singular Upright matrix; Tessera cannot reproduce Adobe's saved correction; source preserved"));
        }
    }
    if upright.mode == UprightMode::Guided
        && let Some(raw) = properties.get("UprightFourSegmentsCount")
    {
        let count = raw.parse::<usize>().ok().filter(|n| (2..=4).contains(n));
        let guides = count.and_then(|count| {
            (0..count)
                .map(|i| {
                    let key = format!("UprightFourSegments_{i}");
                    let v = numbers::<4>(properties.get(key.as_str())?)?;
                    if v.iter().any(|v| !(0.0..=1.0).contains(v))
                        || (v[0] - v[2]).hypot(v[1] - v[3]) < 1e-6
                    {
                        return None;
                    }
                    Some(GuideLine {
                        start: [v[0] as f32, v[1] as f32],
                        end: [v[2] as f32, v[3] as f32],
                    })
                })
                .collect::<Option<Vec<_>>>()
        });
        if let Some(guides) = guides {
            consumed.insert("UprightFourSegmentsCount".to_string());
            for i in 0..guides.len() {
                consumed.insert(format!("UprightFourSegments_{i}"));
            }
            upright.guides = guides;
            if upright.homography.is_none() {
                notes.push("PerspectiveUpright: Guided correction will be recomputed from imported guides; Adobe solver parity is not guaranteed".into());
            }
        } else {
            notes.push("UprightFourSegmentsCount: incomplete or invalid guides; Tessera cannot reproduce Adobe's guided correction; source preserved".into());
        }
    }
    for (key, raw) in &properties {
        if matches!(
            *key,
            "EnableDistractionRemoval" | "GenerativeRemove" | "GenerativeFill"
        ) && !matches!(raw.trim(), "false" | "False" | "0" | "")
        {
            notes.push(format!("{key}: requires Adobe cloud; not translatable. Tessera cannot render this feature without Adobe's rendered pixels. Export a rendered TIFF from Lightroom to preserve its appearance; source preserved"));
        }
    }
    // Do not create a history edit for rows outside this translator's scope.
    if upright != recipe.settings.geometry.upright {
        recipe.edit(EditMeta::user("Import Adobe Upright geometry", 0), |s| {
            s.geometry.upright = upright
        })?;
    }
    for key in &consumed {
        recipe.unknown.remove(&format!("crs:{key}"));
        warnings.retain(|w| {
            !w.starts_with(&format!("crs:{key}:")) && !w.starts_with(&format!("{key}:"))
        });
    }
    for bucket in ["lrcat_develop_source", "lrcat_develop_lua"] {
        if let Some(value) = recipe.unknown.get_mut(bucket) {
            let map = if bucket == "lrcat_develop_source" {
                value.get_mut("properties").and_then(|p| p.as_object_mut())
            } else {
                value.as_object_mut()
            };
            if let Some(map) = map {
                map.retain(|key, _| !consumed.contains(key));
                if map.is_empty() {
                    recipe.unknown.remove(bucket);
                }
            }
        }
    }
    if let Some(entries) = recipe
        .unknown
        .get_mut("lrcat_develop_lua_entries")
        .and_then(|v| v.as_array_mut())
    {
        entries.retain(|e| {
            !e["key"]["string"]
                .as_str()
                .is_some_and(|k| consumed.contains(k))
        });
        if entries.is_empty() {
            recipe.unknown.remove("lrcat_develop_lua_entries");
        }
    }
    for note in notes {
        if !warnings.contains(&note) {
            warnings.push(note);
        }
    }
    Ok(())
}
