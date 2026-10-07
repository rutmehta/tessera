# ENG-6 handoff: honest image-core vs pipeline-cpu fixture gate

Branch `wp/ENG-6` from main `a61703d4`. Worker: Claude Opus 5.5.

## Summary

`fixture_level3_matches_pipeline_cpu` compared only the Sony ARW unless
`IMAGE_CORE_ALL_FIXTURES` was set. It also stopped at the first failing camera,
so the known CR3 mismatch hid a larger RAF one. Neither image-core nor
pipeline-cpu was wrong. The test's preview reference was wrong: it applied lens
Geometry at full resolution before level-pixel Detail, which is not the
documented preview contract and not `StageId` order. The reference now models
that contract. Every fixture runs by default. Absent fixtures print an
uncaptured `SKIPPED` line, or fail when `TESSERA_REQUIRE_RAW_FIXTURES` is set.
No golden, fingerprint or tolerance changed.

## Per-camera max diff, `fixture_level3_matches_pipeline_cpu` (L3, default settings)

| Fixture | Before (main a61703d4): linear / display | After: linear / display | Lens resolved by `Auto` |
|---|---|---|---|
| canon-cr3.CR3 (EOS M50, 500x500) | 4.33e-2 / 24 | 0 / 0 | image auto-calibration, k1 = -0.1008 |
| fuji-raf.RAF (X-E2S, X-Trans, 612x408) | 2.64e-1 / 116 | 0 / 0 | image auto-calibration, k1 = -0.1335 |
| nikon-nef.NEF (D800, 923x616) | 0 / 0 | 0 / 0 | none (Manual) |
| sample.dng (Leica M9, 652x434) | 0 / 0 | 0 / 0 | none (Manual) |
| sony-arw.ARW (NEX-6, 615x410) | 0 / 0 | 0 / 0 | none (Manual) |

The tolerance is unchanged: linear `<= 1e-5`, display `== 0`. The new L0 test
measures 0 for all five cameras: CR3 4000x4000, RAF 4896x3262, NEF 7378x4924,
DNG 5212x3468, ARW 4920x3276.

## Root cause

1. No fixture has an embedded opcode or a database profile. The default
   `LensProfileSource::Auto` therefore falls through to image auto-calibration
   (`pipeline_cpu::lens_resolve::resolve_with`). On the CR3 and RAF it finds a
   radial distortion. On NEF, DNG and ARW it finds nothing.
2. The old `tests/common/preview.rs::linear` called `render_linear_scaled(base,
   .., 8)` with that lens. Because a calibration sample exists, the reference
   takes its M2 branch: Geometry (distortion warp) at full resolution, then
   downsample, then level-pixel Detail. Geometry ran before Detail.
3. The engine follows the documented preview contract in the `render.rs` module
   docs ("At levels above zero, M2 operates on requested-level WB"). It
   downsamples the pre-geometry WB, then runs Detail and Tone, then the composed
   lens Geometry, all on level pixels. That is `StageId` order (Detail 7 before
   Geometry 12).

Evidence that the engine and pipeline-cpu are both right (diagnostics were not
committed):

- With `LensProfileSource::None`, all five cameras match bit-exactly at L3. So
  demosaic, black/white levels, WB, colour matrix, crop and orientation agree.
- `resolve_lens_sensor`, which the engine and the new model use (sparse CFA
  patches), gives exactly the same `CalibrationSample` as `resolve_lens` on the
  full demosaiced frame, which the reference uses. Checked on all five.
- Engine L0 equals `render_linear_scaled(.., 1)` with max diff 0 on all five.
  This is the documented full-resolution contract and includes the
  auto-calibrated warp. It is now a permanent test.
- Engine L3 equals a contract-order model with max diff 0 on all five. The model
  is a downsampled pre-geometry WB, then level Detail, then
  `ResolvedLens::apply_geometry`.

LibRaw was not needed as an independent reference: with lens correction off,
the colour and demosaic paths agree bit-exactly, and `raw_fixture_goldens`
(lens off, immutable pipeline-cpu PNG goldens) still passes. The CR3 and RAF
auto-calibrated k1 values are plausible for those uncorrected kit zooms
(barrel). They were not checked against manufacturer profiles, which is outside
this lane.

## Item table (finding → code → test)

| # | Finding | Code | Test / evidence |
|---|---|---|---|
| 1 | The gate ran ARW only, opted in by an env var | `image-core/tests/fixture.rs`: every fixture by default; `IMAGE_CORE_FIXTURES=arw,cr3` narrows by extension or name; a filter that matches nothing fails | RED `fdd39102`: CR3 4.33e-2/24, RAF 2.64e-1/116 |
| 2 | One camera's failure hid the others | `fixture.rs`: compare every camera, then one assert listing them all | RED output lists CR3 and RAF together |
| 3 | The preview reference warped before Detail | `image-core/tests/common/preview.rs`: defer the common distortion out of the prefix (`distortion_scale = 0`, `manual_distortion = 0`), then level Detail, then `resolve_lens_sensor(..).apply_geometry` | GREEN `8301ef56`: all five 0/0; `render.rs` synthetic level tests still pass |
| 4 | No real-fixture check of the L0 contract | new `fixture_level0_matches_pipeline_cpu_reference` (exact 0) | all five 0 |
| 5 | Absent `fixtures/raw` passed silently | `tests/common/raw_fixtures.rs` (image-core, with a copy in pipeline-cpu): `all`/`selected`/`skipped`/`notice`. An absent or empty set writes `test <name> ... SKIPPED: ..` to the real stderr, visible without `--nocapture`, and panics when `TESSERA_REQUIRE_RAW_FIXTURES` is set | Manual: `PIPELINE_RAW_FIXTURES=/nonexistent` prints SKIPPED for each test; with `TESSERA_REQUIRE_RAW_FIXTURES=1` the tests fail |
| 6 | Other silent tests in image-core and pipeline-cpu | `fixture_level3_m2_extremes_are_finite`, `ml_cfa.rs::estimate_available_raw_fixtures_without_weights`, `pipeline-cpu golden.rs::raw_fixture_goldens`, `opcode_fixtures.rs::real_opcode_fixtures_when_available` now use the helper | `fda2929f`; both modes checked by hand |
| 7 | Coverage the present fixtures cannot provide went unreported | `notice`: no fixture carries DNG opcode lists, so opcode rendering is not exercised; the X-Trans RAF is not estimated by the Bayer-only noise test | printed in plain `cargo test` output |
| 8 | CI fetches fixtures but would still accept skips | `.github/workflows/ci.yml`: `TESSERA_REQUIRE_RAW_FIXTURES=1` on the `ci.sh` step | `31e62d52`, YAML parses |

## Fixture-driven tests elsewhere that still pass silently when fixtures are absent (not fixed, outside image-core/pipeline-cpu)

- raw-decode `src/lib.rs` `fixtures_decode`; libraw-ffi `src/lib.rs`
  `fixture_decode` and `src/sensor_tests.rs`
  `fixture_previews_select_largest_jpeg_not_bitmap`.
- index `tests/catalog.rs` `raw_fixtures_scan_incrementally_when_available`.
- tessera-cli `tests/raw_workflow.rs` `raw_workflow` (passes if any one of the
  five is missing).
- pipeline-gpu `tests/local_tone_resident.rs`
  `preview_approximation_is_bounded_on_real_fixtures`.
- tessera-ffi:
  - `develop.rs`: `process_version_is_undoable_and_persisted`,
    `as_shot_sliders_round_trip_on_every_fixture_and_single_slider_touch` (skips
    each missing extension), `session_renders_into_surfaces_and_persists_undoable_edits`,
    `edited_previews_follow_the_recipe_hash`, `panels_crop_masking_detail_and_history`,
    `slow_interactive_frames_are_not_starved`, and
    `export_batch_does_not_starve_slider_drag` (which also returns when `CI` is set).
  - `document.rs`: `open_document_from_image_is_the_developed_raw`.
  - `export.rs`: `raw_export_with_the_web_preset_and_a_binned_print_render`.
  - `hdr.rs`: `edr_ring_renders_headroom_and_sdr_ring_stays_bit_identical`.
  - `masks.rs`: `gradient_brush_range_overlay_undo_and_persistence`,
    `ai_masks_segment_in_a_job_and_render_through_the_mask_cache`, and
    `subject_mask_on_the_canon_fixture_with_cached_models`.
  - The helpers `fixture()` and `harness()` / `Open::new` in `develop.rs`,
    `hdr.rs` and `masks.rs` feed most of these.
- Subset by default: pipeline-gpu `tests/fixtures.rs`
  `fixture_level3_tolerance_per_operator_and_output` runs ARW only unless
  `PIPELINE_GPU_ALL_FIXTURES=1`. It compares GPU against the CPU engine, not
  the preview model.
- Model-weight tests (ml-caption, ml-embed, ml-enhance, ml-faces, ml-filters,
  ml-segment, filters `remove_models`, ml-runtime `guard`) print SKIP and pass
  without weights. Some honour `TESSERA_REQUIRE_MODELS` or
  `TESSERA_REQUIRE_COREML`.
- Already loud: Swift tests (`XCTUnwrap`/`XCTSkip`), previews, export,
  white_balance_v2, lens_fixtures, linear_dng and eng3_luminance panic or are
  `#[ignore]`.

Suggested follow-up: give these crates the same `raw_fixtures` helper (or one
shared dev-only crate, which needs a Cargo.lock change and so belongs to the
coordinator) and use a single location variable (`PIPELINE_RAW_FIXTURES`).

## Goldens and fingerprints

None changed. No file under `fixtures/golden`, no pixel golden and no hash pin
was touched. Only test reference code, test helpers and CI env changed.

## Gates

All on `31e62d52` (code identical to the pushed head; the HANDOFF commit only
adds this file), with `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-6`,
`CARGO_BUILD_JOBS=5` and `RAYON_NUM_THREADS=5`, after
`cargo clean --release -p image-core -p pipeline-cpu` (852 files removed).

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | exit 0. 640 suites: 3291 passed, 0 failed, 99 ignored (uptime load 10.05 at start, 22.80 at end). No wall-clock retries needed |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0. Worktree clean afterwards, so no bindings drift |
| `tools/orchestrate/swift-gate.sh` | `SWIFT GATE OK`. XCTest: 987 executed, 3 skipped, 0 failures; Swift Testing: 5 tests passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | exit 0. Only the existing `ld` macOS-version warning for `blake3_neon.o` |

The new `fixture_level0_matches_pipeline_cpu_reference` adds about 90 s of
wall time to the image-core `fixture` suite (five full-resolution renders,
each with its reference). On an earlier focused run the L3 test took 40 s
before the fix and about 20 s after.

## Commits

- `fdd39102` test(ENG-6): run the L3 engine/CPU fixture gate on every camera (RED)
- `8301ef56` fix(ENG-6): model level-pixel Geometry in the image-core preview reference
- `fda2929f` test(ENG-6): stop image-core/pipeline-cpu fixture tests passing silently
- `31e62d52` ci(ENG-6): fail fixture tests that skip after fixtures/fetch.sh
- this HANDOFF commit
