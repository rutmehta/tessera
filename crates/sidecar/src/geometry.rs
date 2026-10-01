//! Approximate Adobe geometry and legacy lens decoding shared by catalog and sidecar.
//! Callers retain exact source; successful approximations emit info, never warnings.
use engine_api::{
    EngineResult,
    recipe::{
        Recipe,
        history::{Author, EditMeta},
        settings::{GuideLine, UprightMode},
    },
};
use std::collections::{BTreeMap, BTreeSet};

const FRAME_KEYS: [&str; 5] = [
    "UprightCenterMode",
    "UprightCenterNormX",
    "UprightCenterNormY",
    "UprightFocalMode",
    "UprightFocalLength35mm",
];

fn numbers<const N: usize>(raw: &str) -> Option<[f64; N]> {
    let values = raw
        .split(',')
        .map(|v| v.trim().parse::<f64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let values: [f64; N] = values.try_into().ok()?;
    values.iter().all(|v| v.is_finite()).then_some(values)
}

pub fn apply<'a>(
    recipe: &mut Recipe,
    warnings: &mut Vec<String>,
    properties: impl Iterator<Item = (&'a str, &'a str)>,
) -> EngineResult<()> {
    let properties: BTreeMap<_, _> = properties
        .filter(|(key, _)| {
            key.starts_with("Upright")
                || key.starts_with("UprightFourSegments")
                || matches!(
                    *key,
                    "EnableDistractionRemoval"
                        | "GenerativeRemove"
                        | "GenerativeFill"
                        | "ChromaticAberrationR"
                        | "ChromaticAberrationB"
                )
        })
        .collect();
    if properties.is_empty() {
        return Ok(());
    }
    let mut upright = recipe.settings.geometry.upright.clone();
    let mut consumed = BTreeSet::new();
    let mut notes = Vec::new();
    let mut lens = recipe.settings.lens.clone();
    for (key, target) in [
        ("ChromaticAberrationR", &mut lens.legacy_ca_red),
        ("ChromaticAberrationB", &mut lens.legacy_ca_blue),
    ] {
        if let Some(raw) = properties.get(key)
            && let Ok(value) = raw.trim().parse::<f32>()
            && value.is_finite()
            && (-100.0..=100.0).contains(&value)
        {
            if recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe
                && recipe.process_version.revision <= 2
                && value != 0.
            {
                *target = Some(value);
            } else if recipe.process_version.family == engine_api::recipe::ProcessFamily::Adobe {
                *target = None;
            }
            consumed.insert(key.to_string());
        }
    }
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
        if let Some(matrix) = matrix {
            // Assumption: row-major source-to-output; explicit center/focal metadata
            // defines a normalized focal frame. Focal length is scaled by 35mm
            // on both unit-image axes (Adobe aspect convention is unverified).
            let number = |key: &str, default: f64| {
                properties
                    .get(key)
                    .and_then(|s| s.trim().parse::<f64>().ok())
                    .filter(|v| v.is_finite())
                    .unwrap_or(default)
            };
            let valid_frame_values = [
                "UprightCenterNormX",
                "UprightCenterNormY",
                "UprightFocalLength35mm",
            ]
            .iter()
            .all(|key| {
                properties
                    .get(key)
                    .is_none_or(|raw| raw.trim().parse::<f64>().is_ok_and(|v| v.is_finite()))
            });
            let cx = number("UprightCenterNormX", 0.5);
            let cy = number("UprightCenterNormY", 0.5);
            let focal = number("UprightFocalLength35mm", 35.) / 35.;
            let framed = FRAME_KEYS.iter().any(|key| properties.contains_key(key));
            let matrix = if framed && focal > 0. {
                fn mul(a: [[f64; 3]; 3], b: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
                    std::array::from_fn(|i| {
                        std::array::from_fn(|j| (0..3).map(|k| a[i][k] * b[k][j]).sum())
                    })
                }
                mul(
                    [[focal, 0., cx], [0., focal, cy], [0., 0., 1.]],
                    mul(
                        matrix,
                        [
                            [1. / focal, 0., -cx / focal],
                            [0., 1. / focal, -cy / focal],
                            [0., 0., 1.],
                        ],
                    ),
                )
            } else {
                matrix
            };
            // Poles must be checked after conjugation: the raw saved matrix
            // may be finite over its focal frame but have a pole in [0,1].
            if framed
                && (!valid_frame_values
                    || focal <= 0.
                    || !(0.0..=1.0).contains(&cx)
                    || !(0.0..=1.0).contains(&cy))
            {
                notes.push(format!(
                    "{selected}: invalid center/focal frame; source preserved"
                ));
            } else if !engine_api::recipe::settings::Upright::valid_homography(&matrix) {
                notes.push(format!("{selected}: invalid or singular Upright matrix; Tessera cannot reproduce Adobe's saved correction; source preserved"));
            } else {
                upright.homography = Some(matrix);
                upright.homography_mode = Some(upright.mode);
                consumed.insert(selected.clone());
                for key in properties.keys().filter(|k| FRAME_KEYS.contains(k)) {
                    consumed.insert((*key).to_string());
                }
            }
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
    if upright != recipe.settings.geometry.upright || lens != recipe.settings.lens {
        recipe.settings.geometry.upright = upright;
        recipe.settings.lens = lens;
        // This codec is only invoked during import. Fold all decoded settings into
        // the existing import transaction, preserving the original history base.
        let base = recipe.history.base.clone();
        recipe.history.entries.clear();
        recipe.history.head = None;
        recipe.history.record(
            &base,
            &recipe.settings,
            EditMeta {
                label: "Import Adobe develop".into(),
                author: Author::Import {
                    source: "lightroom".into(),
                },
                ..EditMeta::default()
            },
        )?;
    }
    for key in &consumed {
        recipe.unknown.remove(&format!("crs:{key}"));
        warnings.retain(|w| {
            !w.starts_with(&format!("crs:{key}:")) && !w.starts_with(&format!("{key}:"))
        });
    }
    for key in &consumed {
        let reason = if key.starts_with("ChromaticAberration") {
            "legacy CA sign and radial units are unverified"
        } else {
            "Adobe row-major source-to-output matrix and normalized center/focal frame or guide convention are unverified"
        };
        let is_applied = match key.as_str() {
            "ChromaticAberrationR" => recipe.settings.lens.legacy_ca_red.is_some(),
            "ChromaticAberrationB" => recipe.settings.lens.legacy_ca_blue.is_some(),
            _ => true,
        };
        if !is_applied {
            if let Some(diagnostics) = recipe
                .unknown
                .get_mut("translation_diagnostics")
                .and_then(|v| v.as_object_mut())
            {
                diagnostics.remove(key);
            }
            continue;
        }
        recipe
            .unknown
            .entry("translation_diagnostics".into())
            .or_insert_with(|| serde_json::json!({}))[key] =
            serde_json::json!({"level":"info","message":format!("approximate: {reason}")});
    }
    if recipe
        .unknown
        .get("translation_diagnostics")
        .and_then(|v| v.as_object())
        .is_some_and(|v| v.is_empty())
    {
        recipe.unknown.remove("translation_diagnostics");
    }
    if !consumed.is_empty() {
        warnings.retain(|w| !w.starts_with("legacy Adobe PV1/2:"));
    }
    for note in notes {
        if !warnings.contains(&note) {
            warnings.push(note);
        }
    }
    Ok(())
}
