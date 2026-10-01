//! Synthetic decoder checks for the coordination matrix; no catalog is opened.
use std::collections::BTreeSet;

use engine_api::recipe::CrsKey;
use import_lrcat::lua_develop::{self, EXTENDED_TONE_CURVE_KEYS, KEY_MAP};

fn check_matrix(matrix: &str) -> Result<usize, String> {
    let mut keys = BTreeSet::new();
    let mut translated = 0;
    for line in matrix.lines().filter(|line| line.starts_with("| `")) {
        let cells: Vec<_> = line.split('|').map(str::trim).collect();
        if cells.len() != 7 {
            return Err(format!("expected five columns: {line}"));
        }
        let key = cells[1].trim_matches('`');
        if !keys.insert(key) {
            return Err(format!("duplicate key: {key}"));
        }
        if !matches!(
            cells[3],
            "LR-1" | "LR-2" | "LR-3" | "LR-4" | "LR-5" | "LR-6" | "LR-7"
        ) {
            return Err(format!("invalid lane: {line}"));
        }
        match cells[4] {
            "retained" | "unsupported-diagnostic" => continue,
            "translated" => (),
            _ => return Err(format!("invalid status: {line}")),
        }
        if key.contains('*') || key.contains('/') || cells[5] == "—" {
            return Err(format!(
                "translated row needs a concrete key and synthetic Lua value: {key}"
            ));
        }
        let value = cells[5].trim_matches('`');
        let (recipe, warnings) = lua_develop::parse(&format!("s = {{ {key} = {value} }}"), "15.4")
            .map_err(|e| format!("{key}: {e}"))?;
        for container in ["lrcat_develop_source", "lrcat_develop_lua"] {
            let source = &recipe.unknown.get(container);
            if source.is_some_and(|s| s.get(key).is_some() || s["properties"].get(key).is_some()) {
                return Err(format!(
                    "{key}: translated key is still retained in {container}"
                ));
            }
        }
        if recipe.unknown.contains_key(&format!("crs:{key}")) || !warnings.is_empty() {
            return Err(format!(
                "{key}: translated key has retained diagnostics: {warnings:?}"
            ));
        }
        let path = cells[2].trim_matches('`');
        let json = serde_json::to_value(recipe).unwrap();
        if !path.starts_with('/') || json.pointer(path).is_none() {
            return Err(format!("{key}: missing recipe path {path}"));
        }
        translated += 1;
    }
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
    Ok(translated)
}

#[test]
fn translation_matrix_matches_synthetic_import() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/coordination/LR-TRANSLATION-MATRIX.md"
    );
    let matrix = std::fs::read_to_string(path).expect("translation matrix must exist");
    let translated = check_matrix(&matrix).unwrap();
    eprintln!("matrix guard checked {translated} translated synthetic imports");
}

#[test]
fn matrix_guard_rejects_a_retained_key_claimed_as_translated() {
    let matrix = "| `PointColors` | `/settings/color/point_colors` | LR-1 | translated | `{}` |";
    let error = check_matrix(matrix).unwrap_err();
    assert!(error.contains("still retained"), "{error}");
}
