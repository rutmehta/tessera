//! Synthetic decoder checks for the coordination matrix; no catalog is opened.
#[path = "lr6_fields/mod.rs"]
mod lr6_fields;
use lr6_fields::check_lr6_fields;
use std::collections::BTreeSet;

use engine_api::recipe::{CrsKey, Recipe};
use import_lrcat::lua_develop::{self, EXTENDED_TONE_CURVE_KEYS, KEY_MAP};
use import_lrcat::{LR2_APPROXIMATE_FIELDS, diagnostics};

type Import = dyn Fn(&str, &str) -> Result<(Recipe, Vec<String>), String>;

/// The production synthetic import: one Lua row through `lua_develop::parse`.
fn lua_import(key: &str, value: &str) -> Result<(Recipe, Vec<String>), String> {
    let context = if key == "DepthMapInfo" {
        "LensBlur = { Active = true },".into()
    } else if key.starts_with("UprightTransform_") {
        format!(
            "PerspectiveUpright = {},",
            key.trim_start_matches("UprightTransform_")
        )
    } else if key.starts_with("UprightFourSegments") {
        "PerspectiveUpright = 5, UprightFourSegmentsCount = 4, UprightFourSegments_0 = '0.1,0.1,0.2,0.9', UprightFourSegments_1 = '0.9,0.1,0.8,0.9', UprightFourSegments_2 = '0.1,0.1,0.9,0.2', UprightFourSegments_3 = '0.1,0.9,0.9,0.8',".into()
    } else if key.starts_with("UprightCenter") || key.starts_with("UprightFocal") {
        "PerspectiveUpright = 1, UprightTransform_1 = '0,-1,0,1,0,0,0,0,1',".into()
    } else {
        String::new()
    };
    // Replace the context value instead of creating duplicate Adobe properties.
    let version = if key.starts_with("ChromaticAberration")
        || matches!(
            key,
            "Exposure"
                | "Brightness"
                | "Contrast"
                | "FillLight"
                | "HighlightRecovery"
                | "Recovery"
                | "Shadows"
                | "Blacks"
        ) {
        "5.7"
    } else {
        "15.4"
    };
    let context = if key.starts_with("UprightFourSegments") {
        let mut fields = vec![
            ("PerspectiveUpright", "5"),
            ("UprightFourSegmentsCount", "4"),
            ("UprightFourSegments_0", "'0.1,0.1,0.2,0.9'"),
            ("UprightFourSegments_1", "'0.9,0.1,0.8,0.9'"),
            ("UprightFourSegments_2", "'0.1,0.1,0.9,0.2'"),
            ("UprightFourSegments_3", "'0.1,0.9,0.9,0.8'"),
        ];
        fields.retain(|(k, _)| *k != key);
        fields
            .into_iter()
            .map(|(k, v)| format!("{k} = {v},"))
            .collect::<String>()
    } else {
        context
    };
    let hdr = if EXTENDED_TONE_CURVE_KEYS.contains(&key) {
        "HDREditMode=1,"
    } else {
        ""
    };
    let root = key.split('/').next().unwrap();
    lua_develop::parse(
        &format!("s = {{ {context} {hdr} {root} = {value} }}"),
        version,
    )
    .map_err(|e| format!("{key}: {e}"))
}

/// A synthetic import of an empty develop row: every field at its default.
fn baseline() -> Result<(Recipe, Vec<String>), String> {
    lua_develop::parse("s = {}", "15.4").map_err(|e| format!("baseline: {e}"))
}

#[derive(Debug, Default, PartialEq)]
struct Counts {
    translated: usize,
    approximate: usize,
}

/// Whether the exact source of `key` is in a retained-source container.
fn retained_in(recipe: &Recipe, container: &str, key: &str) -> bool {
    let key = key.split('/').next().unwrap();
    recipe
        .unknown
        .get(container)
        .is_some_and(|s| s.get(key).is_some() || s["properties"].get(key).is_some())
}

/// Row checks only. `import` builds the synthetic recipe for one key/value.
fn check_rows(matrix: &str, import: &Import) -> Result<(Counts, BTreeSet<String>), String> {
    let mut keys = BTreeSet::new();
    let mut counts = Counts::default();
    let (baseline, _) = baseline()?;
    let baseline = serde_json::to_value(baseline).unwrap();
    for line in matrix.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<_> = line.split('|').map(str::trim).collect();
        if cells.len() != 7 {
            return Err(format!("expected five columns: {line}"));
        }
        let key = cells[1].trim_matches('`');
        if !keys.insert(key.to_owned()) {
            return Err(format!("duplicate key: {key}"));
        }
        if !matches!(
            cells[3],
            "LR-1" | "LR-2" | "LR-3" | "LR-4" | "LR-5" | "LR-6" | "LR-7"
        ) {
            return Err(format!("invalid lane: {line}"));
        }
        let approximate = match cells[4] {
            "retained" | "unsupported-diagnostic" => continue,
            "translated" => false,
            "approximate" => true,
            _ => return Err(format!("invalid status: {line}")),
        };
        if key.contains('*')
            || (key.contains('/') && !key.starts_with("MaskGroupBasedCorrections/"))
            || cells[5] == "—"
        {
            return Err(format!(
                "{} row needs a concrete key and synthetic Lua value: {key}",
                cells[4]
            ));
        }
        let value = cells[5].trim_matches('`');
        let (recipe, warnings) = import(key, value)?;
        let path = cells[2].trim_matches('`');
        let json = serde_json::to_value(&recipe).unwrap();
        if !path.starts_with('/') || json.pointer(path).is_none_or(serde_json::Value::is_null) {
            return Err(format!("{key}: missing recipe path {path}"));
        }
        let notes = diagnostics::entries(&recipe)
            .remove(key)
            .unwrap_or_default()
            .into_iter()
            .filter(|note| note.status == "approximate")
            .collect::<Vec<_>>();
        if approximate {
            if !warnings.is_empty() {
                return Err(format!("{key}: approximate key has warnings: {warnings:?}"));
            }
            if json.pointer(path) == baseline.pointer(path)
                || json.pointer(path).is_none_or(serde_json::Value::is_null)
            {
                return Err(format!("{key}: approximate key left {path} unpopulated"));
            }
            if !retained_in(&recipe, "lrcat_develop_source", key) {
                return Err(format!(
                    "{key}: approximate key source is not retained in lrcat_develop_source"
                ));
            }
            if !notes
                .iter()
                .any(|n| n.level == "info" && n.status == "approximate")
            {
                return Err(format!(
                    "{key}: approximate key has no info diagnostics entry"
                ));
            }
            // Lane is free-form; the field must name the row's recipe path.
            if !notes.iter().any(|n| {
                n.level == "info" && n.status == "approximate" && n.field.as_deref() == Some(path)
            }) {
                return Err(format!(
                    "{key}: approximate diagnostics name no entry for field {path}: {notes:?}"
                ));
            }
            if matches!(key, "LensBlur" | "DepthMapInfo") {
                check_lr6_fields(&recipe, key, path)?;
            }
            counts.approximate += 1;
            continue;
        }
        for container in ["lrcat_develop_source", "lrcat_develop_lua"] {
            if retained_in(&recipe, container, key) {
                return Err(format!(
                    "{key}: translated key is still retained in {container}"
                ));
            }
        }
        if recipe.unknown.contains_key(&format!("crs:{key}"))
            || !warnings.is_empty()
            || !notes.is_empty()
        {
            return Err(format!(
                "{key}: translated key has retained diagnostics: {warnings:?} {notes:?}"
            ));
        }
        counts.translated += 1;
    }
    Ok((counts, keys))
}

fn check_matrix(matrix: &str) -> Result<Counts, String> {
    let (counts, keys) = check_rows(matrix, &lua_import)?;
    for key in KEY_MAP
        .iter()
        .map(|(key, _)| *key)
        .filter(|key| {
            CrsKey::from_xmp_name(key).is_none()
                || key.starts_with("Upright")
                || matches!(
                    *key,
                    "PointColors"
                        | "LensBlur"
                        | "RetouchAreas"
                        | "RetouchInfo"
                        | "MaskGroupBasedCorrections"
                )
        })
        .chain(EXTENDED_TONE_CURVE_KEYS.iter().copied())
    {
        if !keys.contains(key) {
            return Err(format!("matrix is missing retained key {key}"));
        }
    }
    Ok(counts)
}

#[test]
fn translation_matrix_matches_synthetic_import() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/coordination/LR-TRANSLATION-MATRIX.md"
    );
    let matrix = std::fs::read_to_string(path).expect("translation matrix must exist");
    let counts = check_matrix(&matrix).unwrap();
    let lr2_rows = matrix
        .lines()
        .filter(|line| line.contains("| LR-2 | approximate |"))
        .collect::<Vec<_>>()
        .join("\n");
    let (lr2_counts, keys) = check_rows(&lr2_rows, &lua_import).unwrap();
    assert_eq!(lr2_counts.approximate, 21);
    assert_eq!(LR2_APPROXIMATE_FIELDS.len(), 21);
    for &(key, path) in LR2_APPROXIMATE_FIELDS {
        assert!(keys.contains(key), "missing LR-2 row: {key}");
        assert!(
            lr2_rows
                .lines()
                .any(|line| line.starts_with(&format!("| `{key}` | `{path}` |"))),
            "matrix path differs from shared LR-2 table: {key}"
        );
    }
    eprintln!("matrix guard checked {counts:?} synthetic imports");
}

#[test]
fn matrix_guard_rejects_a_retained_key_claimed_as_translated() {
    let matrix = "| `PointColors` | `/settings/color/point_colors` | LR-1 | translated | `{}` |";
    let error = check_matrix(matrix).unwrap_err();
    assert_eq!(
        error,
        "PointColors: translated key is still retained in lrcat_develop_source"
    );
}

// --- `approximate` status: test-only fixture rows and a synthetic lane. ---

const APPROXIMATE_ROW: &str =
    "| `Exposure2012` | `/settings/tone/exposure` | LR-2 | approximate | `0.35` |";

/// What a converted lane does for an approximate key; each flag drops one of
/// the guard's conditions.
#[derive(Clone, Copy)]
struct Lane {
    translate: bool,
    retain: bool,
    diagnose: bool,
    warn: bool,
    /// Field named by the diagnostics entry.
    field: &'static str,
}
const FULL: Lane = Lane {
    translate: true,
    retain: true,
    diagnose: true,
    warn: false,
    field: "/settings/tone/exposure",
};

fn synthetic_lane(lane: Lane) -> Box<Import> {
    Box::new(move |key, value| {
        let (mut recipe, mut warnings) = if lane.translate {
            lua_import(key, value)?
        } else {
            baseline()?
        };
        if lane.retain {
            let source = recipe
                .unknown
                .entry("lrcat_develop_source".into())
                .or_insert_with(|| serde_json::json!({"shape": "lua-values", "properties": {}}));
            source["properties"][key] = value.into();
        }
        if lane.diagnose {
            diagnostics::push_approximate(
                &mut recipe,
                key,
                lane.field,
                "LR-2",
                "matrix fixture: Adobe semantics unverified",
            );
        }
        if lane.warn {
            warnings.push(format!("{key}: fixture warning"));
        }
        Ok((recipe, warnings))
    })
}

#[test]
fn matrix_guard_accepts_a_complete_approximate_row() {
    let (counts, _) = check_rows(APPROXIMATE_ROW, &*synthetic_lane(FULL)).unwrap();
    assert_eq!(
        counts,
        Counts {
            translated: 0,
            approximate: 1
        }
    );
}

#[test]
fn matrix_guard_rejects_approximate_without_the_recipe_field() {
    let lane = synthetic_lane(Lane {
        translate: false,
        ..FULL
    });
    let error = check_rows(APPROXIMATE_ROW, &*lane).unwrap_err();
    assert!(error.contains("unpopulated"), "{error}");
    let bad_path = APPROXIMATE_ROW.replace("/settings/tone/exposure", "/settings/tone/nope");
    let error = check_rows(&bad_path, &*synthetic_lane(FULL)).unwrap_err();
    assert!(error.contains("missing recipe path"), "{error}");
}

#[test]
fn matrix_guard_rejects_approximate_without_retained_source() {
    let lane = synthetic_lane(Lane {
        retain: false,
        ..FULL
    });
    let error = check_rows(APPROXIMATE_ROW, &*lane).unwrap_err();
    assert!(error.contains("not retained"), "{error}");
}

#[test]
fn matrix_guard_rejects_approximate_without_a_diagnostic() {
    let lane = synthetic_lane(Lane {
        diagnose: false,
        ..FULL
    });
    let error = check_rows(APPROXIMATE_ROW, &*lane).unwrap_err();
    assert!(error.contains("no info diagnostics entry"), "{error}");
}

#[test]
fn matrix_guard_rejects_approximate_diagnostic_naming_another_field() {
    let lane = synthetic_lane(Lane {
        field: "/settings/tone/contrast",
        ..FULL
    });
    let error = check_rows(APPROXIMATE_ROW, &*lane).unwrap_err();
    assert!(
        error.contains("no entry for field /settings/tone/exposure"),
        "{error}"
    );
}

#[test]
fn matrix_guard_rejects_approximate_with_warnings() {
    let lane = synthetic_lane(Lane { warn: true, ..FULL });
    let error = check_rows(APPROXIMATE_ROW, &*lane).unwrap_err();
    assert!(error.contains("has warnings"), "{error}");
}

#[test]
fn matrix_guard_rejects_an_approximate_row_without_a_synthetic_value() {
    let row = "| `Exposure2012` | `/settings/tone/exposure` | LR-2 | approximate | — |";
    let error = check_rows(row, &*synthetic_lane(FULL)).unwrap_err();
    assert!(error.contains("approximate row needs"), "{error}");
}

#[test]
fn matrix_guard_rejects_a_translated_row_carrying_an_approximate_diagnostic() {
    let row = APPROXIMATE_ROW.replace("approximate", "translated");
    let lane = synthetic_lane(Lane {
        retain: false,
        ..FULL
    });
    let error = check_rows(&row, &*lane).unwrap_err();
    assert!(error.contains("retained diagnostics"), "{error}");
}

/// The production lane does not yet emit approximate diagnostics, so the
/// fixture row fails against it: the guard is not satisfied by the parser alone.
#[test]
fn matrix_guard_rejects_the_fixture_row_against_the_unconverted_parser() {
    let error = check_rows(APPROXIMATE_ROW, &lua_import).unwrap_err();
    assert!(error.contains("not retained"), "{error}");
}

#[test]
fn lr7e_ignored_notes_do_not_count_as_approximate() {
    let lane = |key: &str, value: &str| {
        let (mut recipe, warnings) = synthetic_lane(FULL)(key, value)?;
        recipe.unknown.get_mut(diagnostics::KEY).unwrap()[key][0]["status"] = "ignored".into();
        Ok((recipe, warnings))
    };
    let error = check_rows(APPROXIMATE_ROW, &lane).unwrap_err();
    assert!(error.contains("no info diagnostics entry"), "{error}");
}

#[test]
fn lr7e_translated_row_can_carry_an_ignored_note() {
    let lane = |key: &str, value: &str| {
        let (mut recipe, warnings) = synthetic_lane(Lane {
            retain: false,
            ..FULL
        })(key, value)?;
        recipe.unknown.get_mut(diagnostics::KEY).unwrap()[key][0]["status"] = "ignored".into();
        Ok((recipe, warnings))
    };
    let row = APPROXIMATE_ROW.replace("approximate", "translated");
    let (counts, _) = check_rows(&row, &lane).unwrap();
    assert_eq!(counts.translated, 1);
    assert_eq!(counts.approximate, 0);
}

#[test]
fn lr6d_field_guard_checks_falloff_and_each_reason() {
    let (r, _) = lua_import(
        "LensBlur",
        "{ Active = true, BlurAmount = 37, FocalRange = '10 20 60 80' }",
    )
    .unwrap();
    let path = "/settings/effects/lens_blur";
    check_lr6_fields(&r, "LensBlur", path).unwrap();
    let mut missing = r.clone();
    missing
        .settings
        .effects
        .lens_blur
        .as_mut()
        .unwrap()
        .focus_falloff = None;
    assert!(
        check_lr6_fields(&missing, "LensBlur", path)
            .unwrap_err()
            .contains("FocalRange")
    );
    let mut missing = r.clone();
    missing.unknown.get_mut(diagnostics::KEY).unwrap()["LensBlur"]
        .as_array_mut()
        .unwrap()
        .retain(|d| !d["reason"].as_str().unwrap().contains("BlurAmount"));
    assert!(
        check_lr6_fields(&missing, "LensBlur", path)
            .unwrap_err()
            .contains("BlurAmount")
    );
}

#[test]
fn lr6e_field_guard_rejects_duplicate_field_reasons() {
    let (mut r, _) = lua_import("LensBlur", "{ Active = true, BlurAmount = 37 }").unwrap();
    let entries = r.unknown.get_mut(diagnostics::KEY).unwrap()["LensBlur"]
        .as_array_mut()
        .unwrap();
    let reason = entries
        .iter()
        .find(|d| {
            d["reason"]
                .as_str()
                .unwrap()
                .starts_with("approximate: BlurAmount:")
        })
        .unwrap()
        .clone();
    entries.push(reason);
    assert!(
        check_lr6_fields(&r, "LensBlur", "/settings/effects/lens_blur")
            .unwrap_err()
            .contains("requires one field info reason")
    );
}
