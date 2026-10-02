//! The single shared channel for per-key Lightroom translation diagnostics.
//!
//! A develop key whose matrix status is `approximate` (translated into recipe
//! fields while its Adobe semantics are unverified) keeps its exact source in
//! `recipe.unknown["lrcat_develop_source"]` and records one info-level entry
//! here. Storage is `recipe.unknown[KEY]`: a JSON object keyed by Adobe key,
//! each value an array of [`Entry`] objects. Lanes write only through
//! [`push_approximate`] or [`push_ignored`]; readers use [`entries`]. A recipe
//! without diagnostics has no such member, so its serialization is unchanged.
use std::collections::BTreeMap;

use engine_api::recipe::Recipe;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

/// `recipe.unknown` member holding the diagnostics object.
pub const KEY: &str = "lrcat_translation_diagnostics";

/// One diagnostic for one Adobe key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Always `info`; approximate and ignored translations never warn.
    pub level: String,
    /// Translation status, `approximate` or `ignored`.
    pub status: String,
    /// Owning lane, e.g. `LR-2`.
    pub lane: String,
    /// JSON pointer of the populated recipe field; absent for ignored keys.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Why the translation is approximate or ignored.
    pub reason: String,
}

/// A proven no-op must not appear as an approximate or ignored report entry.
/// Keep foreign shapes and every source-retention member untouched.
pub(crate) fn remove_noop(recipe: &mut Recipe, adobe_key: &str) {
    if let Some(Value::Object(object)) = recipe.unknown.get_mut(KEY) {
        if object.get(adobe_key).is_some_and(|v| {
            v.as_array().is_some_and(|values| {
                values.iter().all(|v| {
                    serde_json::from_value::<Entry>(v.clone()).is_ok_and(|entry| {
                        entry.level == "info"
                            && matches!(entry.status.as_str(), "approximate" | "ignored")
                    })
                })
            })
        }) {
            object.remove(adobe_key);
            if object.is_empty() {
                recipe.unknown.remove(KEY);
            }
        }
    }
}

/// Record that `adobe_key` was translated approximately into `field`.
/// Appends to the key's list (creating the object or list if absent), skips an
/// identical existing entry, and never touches other keys' lists. A value of
/// the wrong shape under [`KEY`] or the key (a foreign writer's) is left
/// untouched and nothing is written, so the matrix guard fails closed; debug
/// builds assert.
pub fn push_approximate(
    recipe: &mut Recipe,
    adobe_key: &str,
    field: &str,
    lane: &str,
    reason: &str,
) {
    let entry = serde_json::to_value(Entry {
        level: "info".into(),
        status: "approximate".into(),
        lane: lane.into(),
        field: Some(field.into()),
        reason: reason.into(),
    })
    .expect("entry serializes");
    push(recipe, adobe_key, entry);
}

/// Record an intentionally ignored source key without claiming a populated field.
/// Uses the same append, dedupe and no-clobber rules as [`push_approximate`].
pub fn push_ignored(recipe: &mut Recipe, adobe_key: &str, lane: &str, reason: &str) {
    let entry = serde_json::to_value(Entry {
        level: "info".into(),
        status: "ignored".into(),
        lane: lane.into(),
        field: None,
        reason: reason.into(),
    })
    .expect("entry serializes");
    push(recipe, adobe_key, entry);
}

fn push(recipe: &mut Recipe, adobe_key: &str, entry: Value) {
    let Value::Object(object) = recipe
        .unknown
        .entry(KEY.into())
        .or_insert_with(|| Value::Object(Map::new()))
    else {
        debug_assert!(false, "{KEY} is not an object; left untouched");
        return;
    };
    let Value::Array(list) = object
        .entry(adobe_key)
        .or_insert_with(|| Value::Array(vec![]))
    else {
        debug_assert!(false, "{KEY}[{adobe_key}] is not an array; left untouched");
        return;
    };
    if !list.contains(&entry) {
        list.push(entry);
    }
}

/// Every well-formed diagnostics entry, by Adobe key. Malformed entries
/// (e.g. from a foreign writer) are skipped; keys with none are omitted.
pub fn entries(recipe: &Recipe) -> BTreeMap<String, Vec<Entry>> {
    let Some(Value::Object(object)) = recipe.unknown.get(KEY) else {
        return BTreeMap::new();
    };
    object
        .iter()
        .filter_map(|(key, list)| {
            let list: Vec<Entry> = list
                .as_array()?
                .iter()
                .filter_map(|e| Entry::deserialize(e).ok())
                .collect();
            (!list.is_empty()).then(|| (key.clone(), list))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_entries_have_no_field_append_dedupe_and_round_trip() {
        let mut recipe = Recipe::default();
        push_ignored(&mut recipe, "CA", "LR-7", "stale");
        push_ignored(&mut recipe, "CA", "LR-7", "stale");
        push_ignored(&mut recipe, "CA", "LR-7", "another reason");
        push_approximate(&mut recipe, "Other", "/a", "LR-2", "existing");
        let other = recipe.unknown[KEY]["Other"].clone();
        push_ignored(&mut recipe, "Other", "LR-7", "stale");
        assert_eq!(recipe.unknown[KEY]["Other"][0], other[0]);
        assert_eq!(recipe.unknown[KEY]["Other"].as_array().unwrap().len(), 2);
        assert_eq!(
            recipe.unknown[KEY]["CA"],
            serde_json::json!([
                {"level":"info", "status":"ignored", "lane":"LR-7", "reason":"stale"},
                {"level":"info", "status":"ignored", "lane":"LR-7", "reason":"another reason"}
            ])
        );
        let back: Recipe = serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap();
        assert_eq!(entries(&back), entries(&recipe));
        assert_eq!(entries(&back)["CA"].len(), 2);
        assert_eq!(entries(&back)["CA"][0].status, "ignored");
    }

    #[test]
    fn ignored_never_clobbers_foreign_shapes() {
        for foreign in [serde_json::json!([1]), serde_json::json!({"CA":"foreign"})] {
            let mut recipe = Recipe::default();
            recipe.unknown.insert(KEY.into(), foreign.clone());
            let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                push_ignored(&mut recipe, "CA", "LR-7", "stale");
            }))
            .is_err();
            assert_eq!(panicked, cfg!(debug_assertions));
            assert_eq!(recipe.unknown[KEY], foreign);
        }
    }

    fn entry(lane: &str, field: &str, reason: &str) -> Entry {
        Entry {
            level: "info".into(),
            status: "approximate".into(),
            lane: lane.into(),
            field: Some(field.into()),
            reason: reason.into(),
        }
    }

    #[test]
    fn absent_object_is_created_with_one_info_entry() {
        let mut recipe = Recipe::default();
        assert!(entries(&recipe).is_empty());
        push_approximate(
            &mut recipe,
            "Exposure2012",
            "/settings/tone/exposure",
            "LR-2",
            "r",
        );
        assert_eq!(
            recipe.unknown[KEY],
            serde_json::json!({"Exposure2012": [{
                "level": "info", "status": "approximate", "lane": "LR-2",
                "field": "/settings/tone/exposure", "reason": "r"
            }]})
        );
        assert_eq!(
            entries(&recipe)["Exposure2012"],
            vec![entry("LR-2", "/settings/tone/exposure", "r")]
        );
    }

    #[test]
    fn appends_and_dedupes_identical_entries() {
        let mut recipe = Recipe::default();
        push_approximate(&mut recipe, "K", "/a", "LR-1", "one");
        push_approximate(&mut recipe, "K", "/a", "LR-1", "one");
        push_approximate(&mut recipe, "K", "/a", "LR-1", "two");
        push_approximate(&mut recipe, "K", "/b", "LR-1", "one");
        assert_eq!(
            entries(&recipe)["K"],
            vec![
                entry("LR-1", "/a", "one"),
                entry("LR-1", "/a", "two"),
                entry("LR-1", "/b", "one"),
            ]
        );
    }

    #[test]
    fn keys_are_isolated_and_never_replaced() {
        let mut recipe = Recipe::default();
        push_approximate(&mut recipe, "A", "/a", "LR-1", "a");
        let before = recipe.unknown[KEY]["A"].clone();
        push_approximate(&mut recipe, "B", "/b", "LR-7", "b");
        assert_eq!(recipe.unknown[KEY]["A"], before);
        let all = entries(&recipe);
        assert_eq!(all.len(), 2);
        assert_eq!(all["A"], vec![entry("LR-1", "/a", "a")]);
        assert_eq!(all["B"], vec![entry("LR-7", "/b", "b")]);
        // Other unknown members are untouched.
        recipe.unknown.insert("other".into(), Value::from(1));
        push_approximate(&mut recipe, "C", "/c", "LR-2", "c");
        assert_eq!(recipe.unknown["other"], 1);
    }

    #[test]
    fn no_diagnostics_serializes_exactly_as_before() {
        let recipe = Recipe::default();
        let bytes = serde_json::to_vec(&recipe).unwrap();
        assert!(!String::from_utf8(bytes.clone()).unwrap().contains(KEY));
        let back: Recipe = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(serde_json::to_vec(&back).unwrap(), bytes);
    }

    #[test]
    fn diagnostics_round_trip_through_serialization() {
        let mut recipe = Recipe::default();
        push_approximate(&mut recipe, "K", "/a", "LR-1", "why");
        let back: Recipe = serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap();
        assert_eq!(entries(&back), entries(&recipe));
    }

    /// Pushes into a wrong-shaped value and returns whether it panicked
    /// (debug builds assert); the recipe must be unchanged either way.
    fn push_into_foreign(recipe: &mut Recipe) -> bool {
        let before = recipe.clone();
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            push_approximate(
                recipe,
                "LensBlur",
                "/settings/effects/lens_blur",
                "LR-6",
                "r",
            )
        }))
        .is_err();
        assert_eq!(recipe.unknown, before.unknown);
        panicked
    }

    #[test]
    fn foreign_top_level_array_survives_untouched() {
        let mut recipe = Recipe::default();
        let foreign = serde_json::json!([{"key": "LensBlur", "note": "LR-6 ad hoc"}]);
        recipe.unknown.insert(KEY.into(), foreign.clone());
        assert_eq!(push_into_foreign(&mut recipe), cfg!(debug_assertions));
        assert_eq!(recipe.unknown[KEY], foreign);
        assert!(entries(&recipe).is_empty());
    }

    #[test]
    fn foreign_per_key_value_survives_untouched() {
        let mut recipe = Recipe::default();
        let foreign = serde_json::json!({"LensBlur": "ad hoc", "Other": [1]});
        recipe.unknown.insert(KEY.into(), foreign.clone());
        assert_eq!(push_into_foreign(&mut recipe), cfg!(debug_assertions));
        assert_eq!(recipe.unknown[KEY], foreign);
        assert!(entries(&recipe).is_empty());
    }

    /// Only this module may spell the storage key; lanes go through the API.
    /// Scans every crate's `src` and the macOS app sources.
    #[test]
    fn no_other_source_writes_the_key_directly() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let root = root.canonicalize().unwrap();
        let this = root.join("crates/import-lrcat/src/diagnostics.rs");
        let mut stack = vec![root.join("apps/mac/Sources")];
        for krate in std::fs::read_dir(root.join("crates")).unwrap() {
            let src = krate.unwrap().path().join("src");
            if src.is_dir() {
                stack.push(src);
            }
        }
        assert!(stack.len() > 10, "workspace layout changed: {stack:?}");
        let mut offenders = vec![];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs" || e == "swift")
                    && path != this
                    && std::fs::read_to_string(&path)
                        .unwrap()
                        .contains(concat!("lrcat_translation", "_diagnostics"))
                {
                    offenders.push(path);
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "write via push_approximate: {offenders:?}"
        );
    }
}
