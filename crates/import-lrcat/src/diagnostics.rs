//! The single shared channel for per-key Lightroom translation diagnostics.
//!
//! A develop key whose matrix status is `approximate` (translated into recipe
//! fields while its Adobe semantics are unverified) keeps its exact source in
//! `recipe.unknown["lrcat_develop_source"]` and records one info-level entry
//! here. Storage is `recipe.unknown[KEY]`: a JSON object keyed by Adobe key,
//! each value an array of [`Entry`] objects. Lanes write only through
//! [`push_approximate`]; readers use [`entries`]. A recipe without diagnostics
//! has no such member, so its serialization is unchanged.
use std::collections::BTreeMap;

use engine_api::recipe::Recipe;
use serde::{Deserialize, Serialize};
#[allow(unused_imports)]
use serde_json::{Map, Value};

/// `recipe.unknown` member holding the diagnostics object.
pub const KEY: &str = "lrcat_translation_diagnostics";

/// One diagnostic for one Adobe key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Always `info` for `approximate`; approximate translations never warn.
    pub level: String,
    /// Matrix status, `approximate`.
    pub status: String,
    /// Owning lane, e.g. `LR-2`.
    pub lane: String,
    /// JSON pointer of the populated recipe field, e.g. `/settings/tone/exposure`.
    pub field: String,
    /// Why the translation is approximate.
    pub reason: String,
}

/// Record that `adobe_key` was translated approximately into `field`.
/// Appends to the key's list (creating the object or list if absent), skips an
/// identical existing entry, and never touches other keys' lists. A non-object
/// value under [`KEY`] (which no writer produces) is replaced by an object.
pub fn push_approximate(
    recipe: &mut Recipe,
    adobe_key: &str,
    field: &str,
    lane: &str,
    reason: &str,
) {
    let _ = (recipe, adobe_key, field, lane, reason);
    todo_red()
}

fn todo_red() {}

/// Every well-formed diagnostics entry, by Adobe key. Malformed entries
/// (e.g. from a foreign writer) are skipped; keys with none are omitted.
pub fn entries(recipe: &Recipe) -> BTreeMap<String, Vec<Entry>> {
    let _ = recipe;
    BTreeMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(lane: &str, field: &str, reason: &str) -> Entry {
        Entry {
            level: "info".into(),
            status: "approximate".into(),
            lane: lane.into(),
            field: field.into(),
            reason: reason.into(),
        }
    }

    #[test]
    fn absent_object_is_created_with_one_info_entry() {
        let mut recipe = Recipe::default();
        assert!(entries(&recipe).is_empty());
        push_approximate(&mut recipe, "Exposure2012", "/settings/tone/exposure", "LR-2", "r");
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
        let back: Recipe =
            serde_json::from_slice(&serde_json::to_vec(&recipe).unwrap()).unwrap();
        assert_eq!(entries(&back), entries(&recipe));
    }

    /// Only this module may spell the storage key; lanes go through the API.
    #[test]
    fn no_other_crate_source_writes_the_key_directly() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut stack = vec![src];
        let mut offenders = vec![];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "rs")
                    && path.file_name().is_some_and(|n| n != "diagnostics.rs")
                    && std::fs::read_to_string(&path)
                        .unwrap()
                        .contains(concat!("lrcat_translation", "_diagnostics"))
                {
                    offenders.push(path);
                }
            }
        }
        assert!(offenders.is_empty(), "write via push_approximate: {offenders:?}");
    }
}
