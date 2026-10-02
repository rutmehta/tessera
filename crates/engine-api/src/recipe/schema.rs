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
//! ("point_colors", |r| !r.settings.color.point_colors.is_empty()),
//! ```
//!
//! and a test that calls `assert_bumped_only_when_present("point_colors", ..)`.
//! The first entry also lifts [`max_writable_schema_version`] to 4, so this
//! build can re-save the schema 4 documents it writes.
//!
//! Consequences once a feature is used:
//! - Writing never mutates the caller, so a recipe saved and reloaded compares
//!   unequal on `schema_version` (3 in memory, 4 reloaded). Round-trip
//!   equality tests (`sidecar/tests/roundtrip.rs`, `merge/tests/recipe.rs`)
//!   must ignore `schema_version` for recipes using the feature.
//! - The bump is sticky: a recipe written as 4 keeps loading as 4 and is
//!   written as 4 even after the feature is removed
//!   (`max(schema_version, required)`).

use super::{Recipe, RECIPE_SCHEMA_VERSION};

/// Schema version carried by recipes that use a schema 4 feature.
pub const RECIPE_SCHEMA_VERSION_V4: u32 = 4;

/// A named schema 4 feature test.
pub type FeaturePredicate = (&'static str, fn(&Recipe) -> bool);

/// Every schema 4 feature, by diagnostic name.
const V4_FEATURE_PREDICATES: &[FeaturePredicate] =
    &[
        ("local_curves", |r| local_feature(r, |p| p.curves.is_some())),
        ("local_curves_extended", |r| {
            local_feature(r, |p| p.curves_extended.is_some())
        }),
        ("local_point_colors", |r| {
            local_feature(r, |p| p.point_colors.is_some())
        }),
        ("local_color_overlay", |r| {
            local_feature(r, |p| p.color_overlay.is_some())
        }),
        ("local_defringe", |r| local_feature(r, |p| p.defringe != 0.)),
        ("retouch", |r| {
            !r.settings.locals.retouch.is_empty() || !r.history.base.locals.retouch.is_empty()
        }),
        ("point_colors", |r| {
            !r.settings.color.point_colors.is_empty()
        }),
        ("mask_instance_hint", |r| {
            mask_feature(r, |c| {
                c.adobe_ai
                    .as_ref()
                    .is_some_and(|a| a.instance_hint.is_some())
            })
        }),
        ("adobe_ai_mask", |r| {
            mask_feature(r, |c| c.adobe_ai.is_some())
        }),
        ("mask_luminance_display", |r| {
            mask_feature(r, |c| {
                matches!(
                    c.kind,
                    super::MaskKind::LuminanceRange {
                        luminance_domain: super::mask::LuminanceDomain::Display,
                        ..
                    }
                )
            })
        }),
        ("lens_blur", |r| {
            r.settings.effects.lens_blur.as_ref().is_some_and(|b| {
                b.focus_falloff.is_some() || b.adobe.is_some() || b.depth.is_some()
            })
        }),
        ("upright_homography", |r| {
            r.settings.geometry.upright.homography.is_some()
        }),
        ("upright_homography_mode", |r| {
            r.settings.geometry.upright.homography_mode.is_some()
        }),
        ("legacy_ca_red", |r| r.settings.lens.legacy_ca_red.is_some()),
        ("legacy_ca_blue", |r| {
            r.settings.lens.legacy_ca_blue.is_some()
        }),
        ("monochrome", |r| {
            r.settings
                .color
                .monochrome
                .as_ref()
                .is_some_and(|m| m.enabled || m.mixer != Default::default())
        }),
        ("curves_extended", |r| {
            r.settings.tone.curves_extended.is_some()
        }),
        ("legacy_pv2010", |r| r.settings.tone.legacy_pv2010.is_some()),
        ("mask_component_disabled", |r| {
            mask_feature(r, |c| !c.enabled)
        }),
        ("mask_groups", |r| mask_feature(r, |c| c.group.is_some())),
        ("mask_luminance_bounds", |r| {
            mask_feature(r, |c| c.luminance_bounds.is_some())
        }),
    ];

fn local_feature(recipe: &Recipe, uses: fn(&super::LocalParams) -> bool) -> bool {
    [&recipe.settings, &recipe.history.base]
        .into_iter()
        .any(|s| s.locals.adjustments.iter().any(|g| uses(&g.params)))
}

fn mask_feature(recipe: &Recipe, uses: fn(&super::MaskComponent) -> bool) -> bool {
    // Include disabled subtrees and retouch areas: re-enabling them must not
    // expose data already discarded by an older writer. The history base is
    // typed settings too; JSON patches themselves preserve unknown fields.
    [&recipe.settings, &recipe.history.base]
        .into_iter()
        .any(|settings| {
            let mut stack: Vec<_> = settings
                .locals
                .adjustments
                .iter()
                .flat_map(|g| &g.components)
                .collect();
            for op in &settings.locals.retouch {
                if let super::mask::RetouchTarget::Area { components } = &op.target {
                    stack.extend(components);
                }
            }
            while let Some(c) = stack.pop() {
                if uses(c) {
                    return true;
                }
                if let Some(children) = &c.group {
                    stack.extend(children);
                }
            }
            false
        })
}

/// Lowest schema version that can represent `recipe` (3 or 4).
pub fn required_schema_version(recipe: &Recipe) -> u32 {
    if active_predicates().iter().any(|(_, uses)| uses(recipe)) {
        RECIPE_SCHEMA_VERSION_V4
    } else {
        RECIPE_SCHEMA_VERSION
    }
}

/// Names of the schema 4 features `recipe` uses, in list order (diagnostics).
pub fn v4_features_used(recipe: &Recipe) -> Vec<&'static str> {
    active_predicates()
        .iter()
        .filter(|(_, uses)| uses(recipe))
        .map(|(name, _)| *name)
        .collect()
}

/// Newest stored `schema_version` this build will write: 3 until the first
/// schema 4 feature is registered, then 4.
pub fn max_writable_schema_version() -> u32 {
    if active_predicates().is_empty() {
        RECIPE_SCHEMA_VERSION
    } else {
        RECIPE_SCHEMA_VERSION_V4
    }
}

/// Version every serialisation writes: `max(schema_version, required)` when a
/// schema 4 feature is used, otherwise the stored version unchanged, so
/// existing documents (including in-memory legacy versions) stay
/// byte-identical.
pub(crate) fn written_schema_version(recipe: &Recipe) -> u32 {
    let required = required_schema_version(recipe);
    if required > RECIPE_SCHEMA_VERSION {
        recipe.schema_version.max(required)
    } else {
        recipe.schema_version
    }
}

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

    #[test]
    fn adobe_ai_mask() {
        assert_bumped_only_when_present("adobe_ai_mask", |r| {
            let mut component =
                super::super::MaskComponent::new(super::super::MaskKind::Subject { model: None });
            component.adobe_ai = Some(super::super::mask::AdobeAiMask {
                instance_hint: None,
                resource_id: Some("opaque".into()),
                category: "Subject".into(),
                mask_key: Some([3; 32]),
                regenerate: false,
            });
            r.edit(EditMeta::user("AI mask", 2), |s| {
                s.locals.adjustments.push(super::super::LocalAdjustment {
                    components: vec![component],
                    ..Default::default()
                })
            })
            .unwrap();
        });
    }

    #[test]
    fn local_curves() {
        assert_bumped_only_when_present("local_curves", |r| {
            r.edit(EditMeta::user("Local", 2), |s| {
                let mut g = super::super::LocalAdjustment::default();
                g.params.curves = Some(Default::default());
                s.locals.adjustments.push(g);
            })
            .unwrap();
        });
    }
    #[test]
    fn local_curves_extended() {
        assert_bumped_only_when_present("local_curves_extended", |r| {
            r.edit(EditMeta::user("Local", 2), |s| {
                let mut g = super::super::LocalAdjustment::default();
                g.params.curves_extended = Some(Default::default());
                s.locals.adjustments.push(g);
            })
            .unwrap();
        });
    }
    #[test]
    fn local_point_colors() {
        assert_bumped_only_when_present("local_point_colors", |r| {
            r.edit(EditMeta::user("Local", 2), |s| {
                let mut g = super::super::LocalAdjustment::default();
                g.params.point_colors = Some(vec![]);
                s.locals.adjustments.push(g);
            })
            .unwrap();
        });
    }
    #[test]
    fn local_color_overlay() {
        assert_bumped_only_when_present("local_color_overlay", |r| {
            r.edit(EditMeta::user("Local", 2), |s| {
                let mut g = super::super::LocalAdjustment::default();
                g.params.color_overlay = Some([120., 50.]);
                s.locals.adjustments.push(g);
            })
            .unwrap();
        });
    }
    #[test]
    fn local_defringe() {
        assert_bumped_only_when_present("local_defringe", |r| {
            r.edit(EditMeta::user("Local", 2), |s| {
                let mut g = super::super::LocalAdjustment::default();
                g.params.defringe = 50.;
                s.locals.adjustments.push(g);
            })
            .unwrap();
        });
    }
    #[test]
    fn mask_instance_hint() {
        assert_bumped_only_when_present("mask_instance_hint", |r| {
            let component = serde_json::from_value(serde_json::json!({"kind":"object", "model":null, "points":[[0.5,0.5]], "adobe_ai":{"category":"Object","resource_id":null,"mask_key":null,"regenerate":true,"instance_hint":{"InstanceIDs":[{"InstanceID":1}]}}})).unwrap();
            r.edit(EditMeta::user("Instance hint", 2), |s| {
                s.locals.adjustments.push(super::super::LocalAdjustment {
                    components: vec![component],
                    ..Default::default()
                })
            })
            .unwrap();
        });
    }

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

    #[test]
    fn lr1c_point_colors_bumps_only_when_present() {
        assert_bumped_only_when_present("point_colors", |r| {
            r.settings.color.point_colors.push(Default::default());
        });
    }

    #[test]
    fn lr7d_homography() {
        assert_bumped_only_when_present("upright_homography", |r| {
            r.settings.geometry.upright.homography =
                Some([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
        });
    }
    #[test]
    fn lr7d_mode_tag() {
        assert_bumped_only_when_present("upright_homography_mode", |r| {
            r.settings.geometry.upright.homography_mode =
                Some(crate::recipe::settings::UprightMode::Auto);
        });
    }
    #[test]
    fn lr7d_red() {
        assert_bumped_only_when_present("legacy_ca_red", |r| {
            r.settings.lens.legacy_ca_red = Some(35.)
        });
    }
    #[test]
    fn lr7d_blue() {
        assert_bumped_only_when_present("legacy_ca_blue", |r| {
            r.settings.lens.legacy_ca_blue = Some(-25.)
        });
    }

    #[test]
    fn lens_blur_requires_v4_only_when_present() {
        let mut native = Recipe::default();
        native.settings.effects.lens_blur = Some(Default::default());
        native.history.base = native.settings.clone();
        assert_eq!(required_schema_version(&native), 3);
        assert_eq!(written_version(&native.to_json().unwrap()), 3);
        for field in ["focus_falloff", "adobe", "depth"] {
            assert_bumped_only_when_present("lens_blur", |r| {
                let value = match field {
                    "focus_falloff" => serde_json::json!([0.1, 0.2]),
                    _ => serde_json::json!({}),
                };
                r.settings.effects.lens_blur =
                    Some(serde_json::from_value(serde_json::json!({field: value})).unwrap());
            });
        }
    }

    fn written_version(bytes: &[u8]) -> u64 {
        serde_json::from_slice::<Value>(bytes).unwrap()["schema_version"]
            .as_u64()
            .unwrap()
    }

    #[test]
    fn lr2d_monochrome_bumps_only_when_enabled_or_nonzero() {
        assert_bumped_only_when_present("monochrome", |r| {
            r.settings.color.monochrome = Some(crate::recipe::settings::MonochromeSettings {
                enabled: true,
                ..Default::default()
            });
        });
        assert_bumped_only_when_present("monochrome", |r| {
            r.settings.color.monochrome = Some(Default::default());
            r.settings.color.monochrome.as_mut().unwrap().mixer.red = 25.;
        });
    }

    #[test]
    fn lr2d_curves_extended_bumps_only_when_present() {
        assert_bumped_only_when_present("curves_extended", |r| {
            r.settings.tone.curves_extended = Some(Default::default());
        });
    }

    #[test]
    fn lr2d_legacy_pv2010_bumps_only_when_present() {
        assert_bumped_only_when_present("legacy_pv2010", |r| {
            r.settings.tone.legacy_pv2010 = Some(Default::default());
        });
    }

    fn mask_recipe(r: &mut Recipe, component: serde_json::Value) {
        r.settings.locals.adjustments.push(
            serde_json::from_value(serde_json::json!({
                "components": [component]
            }))
            .unwrap(),
        );
    }

    #[test]
    fn lr4c_disabled_component_requires_v4() {
        assert_bumped_only_when_present("mask_component_disabled", |r| {
            mask_recipe(
                r,
                serde_json::json!({"kind":"brush","strokes":[],"enabled":false}),
            )
        });
    }
    #[test]
    fn lr4c_nested_group_requires_v4() {
        assert_bumped_only_when_present("mask_groups", |r| {
            mask_recipe(
                r,
                serde_json::json!({"kind":"brush","strokes":[],"group":[]}),
            )
        });
    }
    #[test]
    fn lr4c_luminance_bounds_requires_v4() {
        assert_bumped_only_when_present("mask_luminance_bounds", |r| {
            mask_recipe(
                r,
                serde_json::json!({"kind":"luminance_range","range":[0.2,0.8],"luminance_bounds":[0.1,0.2,0.8,0.9]}),
            )
        });
    }

    #[test]
    fn lr4c_disabled_masks_in_retouch_and_history_base_require_v4() {
        for path in ["/settings", "/history/base"] {
            let mut value = serde_json::to_value(Recipe::default()).unwrap();
            value.pointer_mut(path).unwrap()["locals"]["retouch"] = serde_json::json!([{
                "id":0,"kind":{"kind":"heal","source_offset":[0,0]},
                "target":{"kind":"area","components":[{"kind":"brush","strokes":[],"enabled":false}]}
            }]);
            let r: Recipe = serde_json::from_value(value).unwrap();
            assert_eq!(required_schema_version(&r), 4, "{path}");
        }
    }

    #[test]
    fn lr4d_approximate_dabs_and_color_samples_use_existing_v3_fields() {
        for component in [
            serde_json::json!({"kind":"brush","strokes":[{"points":[[0.2,0.3,1.0]],"radius":0.1,"feather":40.0,"flow":50.0,"erase":true}]}),
            serde_json::json!({"kind":"color_range","samples":[[0.5,0.1,0.2]],"amount":25.0}),
        ] {
            let mut recipe = Recipe::default();
            mask_recipe(&mut recipe, component);
            assert!(v4_features_used(&recipe).is_empty());
            assert_eq!(required_schema_version(&recipe), 3);
            let bytes = recipe.to_json().unwrap();
            assert_eq!(written_version(&bytes), 3);
            let reloaded = Recipe::from_json(&bytes).unwrap();
            assert_eq!(reloaded.settings.locals, recipe.settings.locals);
            assert_eq!(reloaded.to_json().unwrap(), bytes);
        }
    }

    #[test]
    fn lr4e_display_luminance_bumped_only_when_present() {
        assert_bumped_only_when_present("mask_luminance_display", |r| {
            mask_recipe(
                r,
                serde_json::json!({"kind":"luminance_range","range":[0.2,0.8],"luminance_domain":"display"}),
            )
        });
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
    fn retouch_bumps_only_when_present() {
        assert_bumped_only_when_present("retouch", |r| {
            r.settings.locals.retouch.push(serde_json::from_value(serde_json::json!({
                "id":1,"kind":{"kind":"clone","source_offset":[0.5,0.0]},
                "target":{"kind":"area","components":[]},"opacity":50.0,"feather":0.0,"enabled":true
            })).unwrap());
        });
    }

    #[test]
    fn retouch_in_history_base_bumps_only_when_present() {
        assert_bumped_only_when_present("retouch", |r| {
            r.history.base.locals.retouch.push(serde_json::from_value(serde_json::json!({
                "id":1,"kind":{"kind":"clone","source_offset":[0.5,0.0]},
                "target":{"kind":"area","components":[]},"opacity":50.0,"feather":0.0,"enabled":true
            })).unwrap());
        });
    }

    #[test]
    fn writable_max_follows_the_list() {
        let expected = if V4_FEATURE_PREDICATES.is_empty() {
            RECIPE_SCHEMA_VERSION
        } else {
            RECIPE_SCHEMA_VERSION_V4
        };
        assert_eq!(max_writable_schema_version(), expected);
    }

    #[test]
    fn fixtures_require_only_base_schema_while_unused() {
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
