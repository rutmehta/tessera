# M5-26 CPU public controls

## APIs

- `compositor::adjust::PhotoFilterPreset`: serde snake_case enum with Warming85, WarmingLba, Warming81, Cooling80, CoolingLbb, Cooling82, Red, Orange, Yellow, Green, Cyan, Blue, Violet, Magenta, Sepia, DeepRed, DeepBlue, DeepEmerald, DeepYellow, Underwater, and `Custom([f32; 3])`.
- `PhotoFilterPreset::resolve(self, density: f32, preserve_luminosity: bool) -> EngineResult<Adjustment>` returns existing `Adjustment::PhotoFilter`. Density must be finite 0..100, custom RGB finite 0..1. Named colors are documented approximate native sRGB byte swatches normalized by 255, not optical measurements or Adobe numerical-equivalence claims. No pixel formulas changed. Non-sRGB consumers must convert the resolved color themselves.
- `Adjustment::match_color_from_layer(doc: &DocState, source_layer: LayerId, target: &[[f32; 3]]) -> EngineResult<Adjustment>` resolves nested layer IDs with `DocState::find`, reads level-zero raw straight RGBA from pixel layers/text proxies, excludes zero alpha and equally weights positive-alpha samples. Rejects root/missing IDs, non-raster sources, non-RGBA rasters, tagged documents, invalid alpha and empty/nonfinite sample populations. Does not apply masks, selection, opacity, visibility, effects or group/smart-object rendering. Destination samples remain caller-selected straight sRGB. It freezes statistics; later source edits require reconstruction. Current implementation scans the full raster and collects source RGB in memory.
- Existing `Adjustment::match_color_from_pixels(source_layer, source, target)` retained as explicit low-level API. Its ID is metadata, not a verified source association.
- `Adjustment::validate(&self) -> EngineResult<()>` checks every float-bearing variant for finite parameters; ColorLookup requires size 2..256, exactly size^3 finite samples; MatchColor requires nonroot identity and nonnegative finite standard deviations; ShadowsHighlights delegates to its validation. Other existing finite-value clamping/fallback behavior is intentionally unchanged.
- `Adjustment::from_versioned_json(&str) -> Result<Adjustment, serde_json::Error>` now calls validation after checking version 1. Direct enum/envelope serde decoding remains unchanged. `to_versioned_json` remains the existing serializer; invalid floats can serialize to null and are rejected by checked decoding.

## Parent integration required

Call `adjustment.validate()?` in CPU/GPU executor entry paths before `compile()` for direct enum inputs. ColorLookup's existing compile assertion remains an internal invariant, not a public checked-error interface. ShadowsHighlights still requires its neighborhood execution path. No renderer/resident/shadow/PSD/Cargo/color.rs files were edited by this pass. Full clippy is deferred to parent; current build warnings are in color.rs (parentheses) and shadows.rs (missing docs).

## Verification

- TDD red observed for malformed versioned lookup acceptance, missing preset API, and missing layer-resolving API; each subsequently green.
- Versioned serde round trips explicitly cover all 14 new variants (asserted count); direct legacy enum encoding remains tested. Preset tests cover every named choice plus custom, invalid density/color, and unknown preset.
- Tests cover nested source identity, missing/root/nonraster IDs, transparent exclusion, empty source, nonfinite controls/LUT, invalid LUT sizes/counts, unsupported versions and invalid shadow settings.
- `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-26 cargo test -p compositor --lib --quiet`: 53 passed, 1 ignored, 0 failed.
- Same target, `cargo test -p compositor --test m5_26_gpu --quiet`: 6 passed, 0 failed; existing CPU/GPU parity unchanged.
- Owned Rust files formatted only with `rustfmt --edition 2024 --config skip_children=true`; scoped `git diff --check` passed.
- Removed redundant `adjust/cpu_harness.rs` and `adjust/run_tests.py`; tests now run through the real compositor crate. No commits.
