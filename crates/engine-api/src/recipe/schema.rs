//! Conditional recipe schema version 4 (LR-SCHEMA).
//!
//! A recipe that *uses* a field an older (schema 3) build would misrender or
//! drop on re-save is written as schema 4; every other recipe stays schema 3
//! and byte-identical. Which fields count is decided by exactly one list,
//! [`V4_FEATURE_PREDICATES`]. Nothing else may decide the written version.
//!
//! Adding a feature (one line, plus a test in `v4_feature_predicates`):
//!
//! ```text
//! ("point_colors", |r| r.settings.color.point_colors.is_some()),
//! ```
//!
//! and a test that calls `assert_bumped_only_when_present("point_colors", ..)`.
//! The first entry also lifts [`max_writable_schema_version`] to 4, so this
//! build can re-save the schema 4 documents it writes.

use super::{Recipe, RECIPE_SCHEMA_VERSION};

/// Schema version carried by recipes that use a schema 4 feature.
pub const RECIPE_SCHEMA_VERSION_V4: u32 = 4;

/// A named schema 4 feature test.
pub type FeaturePredicate = (&'static str, fn(&Recipe) -> bool);

/// Every schema 4 feature, by diagnostic name. Empty until a lane lands one.
const V4_FEATURE_PREDICATES: &[FeaturePredicate] = &[];

/// Lowest schema version that can represent `recipe` (3 or 4).
pub fn required_schema_version(recipe: &Recipe) -> u32 {
    let _ = recipe;
    unimplemented!("LR-SCHEMA")
}

/// Names of the schema 4 features `recipe` uses, in list order.
pub fn v4_features_used(recipe: &Recipe) -> Vec<&'static str> {
    let _ = recipe;
    unimplemented!("LR-SCHEMA")
}

/// Newest stored `schema_version` this build will write.
pub fn max_writable_schema_version() -> u32 {
    unimplemented!("LR-SCHEMA")
}

#[allow(dead_code)]
fn active_predicates() -> &'static [FeaturePredicate] {
    #[cfg(test)]
    if let Some(predicates) = test_override::current() {
        return predicates;
    }
    V4_FEATURE_PREDICATES
}

#[cfg(test)]
pub(crate) mod test_override {
    //! Test-only replacement of the predicate list, per test thread, so the
    //! harness can be proven while the real list stays empty.
    use std::cell::Cell;

    use super::FeaturePredicate;

    thread_local! {
        static PREDICATES: Cell<Option<&'static [FeaturePredicate]>> = const { Cell::new(None) };
    }

    pub(crate) fn current() -> Option<&'static [FeaturePredicate]> {
        PREDICATES.with(Cell::get)
    }

    /// Runs `f` with `predicates` in place of the real list.
    pub(crate) fn with<R>(predicates: &'static [FeaturePredicate], f: impl FnOnce() -> R) -> R {
        struct Reset(Option<&'static [FeaturePredicate]>);
        impl Drop for Reset {
            fn drop(&mut self) {
                PREDICATES.with(|p| p.set(self.0));
            }
        }
        let _reset = Reset(PREDICATES.with(|p| p.replace(Some(predicates))));
        f()
    }
}

#[cfg(test)]
mod v4_feature_predicates {
    use serde_json::Value;

    use super::*;
    use crate::id::ImageId;
    use crate::recipe::EditMeta;

    /// Harness every schema 4 lane copies: `mutate` must make a schema 3
    /// recipe require schema 4 by way of the predicate called `name`, every
    /// write path must then emit 4 without touching the caller's struct, and
    /// the written document must reload and re-save byte-identically.
    pub(super) fn assert_bumped_only_when_present(name: &str, mutate: impl FnOnce(&mut Recipe)) {
        assert!(
            active_predicates().iter().any(|(n, _)| *n == name),
            "{name} is not in V4_FEATURE_PREDICATES"
        );
        let mut recipe = Recipe::new(ImageId(7));
        recipe
            .edit(EditMeta::user("Exposure", 1), |s| s.tone.exposure = 0.25)
            .unwrap();
        assert_eq!(required_schema_version(&recipe), RECIPE_SCHEMA_VERSION);
        assert!(v4_features_used(&recipe).is_empty());
        assert_eq!(written_version(&recipe.to_json().unwrap()), 3);

        mutate(&mut recipe);
        assert_eq!(required_schema_version(&recipe), RECIPE_SCHEMA_VERSION_V4);
        assert!(v4_features_used(&recipe).contains(&name));
        assert_eq!(
            recipe.schema_version, RECIPE_SCHEMA_VERSION,
            "caller mutated"
        );
        let bytes = recipe.to_json().unwrap();
        assert_eq!(written_version(&bytes), 4);
        assert_eq!(serde_json::to_value(&recipe).unwrap()["schema_version"], 4);
        let reloaded = Recipe::from_json(&bytes).unwrap();
        assert_eq!(reloaded.schema_version, RECIPE_SCHEMA_VERSION_V4);
        assert_eq!(reloaded.to_json().unwrap(), bytes);
    }

    fn written_version(bytes: &[u8]) -> u64 {
        serde_json::from_slice::<Value>(bytes).unwrap()["schema_version"]
            .as_u64()
            .unwrap()
    }

    const TEST_FEATURE: &str = "lr_schema_test_feature";
    const TEST_PREDICATES: &[FeaturePredicate] =
        &[(TEST_FEATURE, |r| r.unknown.contains_key(TEST_FEATURE))];

    fn fixture_recipes() -> Vec<(String, Recipe)> {
        let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../..");
        let mut out = vec![
            ("default".to_string(), Recipe::default()),
            (
                "recipe-1.2.json".to_string(),
                Recipe::from_json(include_bytes!("../../tests/fixtures/recipe-1.2.json")).unwrap(),
            ),
        ];
        // Committed sidecar envelopes written by the app (`{"recipe": ..}`).
        for envelope in [
            "tools/orchestrate/wp/UX-04/export-preview-20260927/fixtures/raw-sidecars/sony-arw.json",
            "tools/orchestrate/wp/UX-03/evidence/develop-recovery-gui-2026-09-28/photos/.edits/recovery-check.json",
        ] {
            let bytes = std::fs::read(format!("{root}/{envelope}"))
                .unwrap_or_else(|e| panic!("fixture {envelope} missing: {e}"));
            let value: Value = serde_json::from_slice(&bytes).unwrap();
            let recipe = serde_json::to_vec(&value["recipe"]).unwrap();
            out.push((envelope.to_string(), Recipe::from_json(&recipe).unwrap()));
        }
        out
    }

    #[test]
    fn real_list_is_empty() {
        assert!(V4_FEATURE_PREDICATES.is_empty());
        assert_eq!(max_writable_schema_version(), RECIPE_SCHEMA_VERSION);
    }

    #[test]
    fn empty_list_requires_schema_3_for_every_fixture() {
        for (name, recipe) in fixture_recipes() {
            assert_eq!(required_schema_version(&recipe), 3, "{name}");
            assert!(v4_features_used(&recipe).is_empty(), "{name}");
            assert_eq!(written_version(&recipe.to_json().unwrap()), 3, "{name}");
        }
    }

    #[test]
    fn harness_proves_a_test_only_predicate() {
        test_override::with(TEST_PREDICATES, || {
            assert_eq!(max_writable_schema_version(), RECIPE_SCHEMA_VERSION_V4);
            assert_bumped_only_when_present(TEST_FEATURE, |r| {
                r.unknown.insert(TEST_FEATURE.into(), Value::Bool(true));
            });
        });
    }

    #[test]
    fn unused_test_predicate_leaves_bytes_unchanged() {
        for (name, recipe) in fixture_recipes() {
            let plain = recipe.to_json().unwrap();
            let with_list = test_override::with(TEST_PREDICATES, || {
                assert!(v4_features_used(&recipe).is_empty(), "{name}");
                recipe.to_json().unwrap()
            });
            assert_eq!(plain, with_list, "{name}");
        }
    }

    #[test]
    fn newer_than_v4_stays_read_only_with_a_v4_list() {
        test_override::with(TEST_PREDICATES, || {
            let r = Recipe::from_json(br#"{"schema_version":5}"#).unwrap();
            assert!(matches!(
                r.to_json(),
                Err(crate::error::EngineError::SchemaVersion {
                    found: 5,
                    supported: 4,
                    ..
                })
            ));
        });
    }
}
