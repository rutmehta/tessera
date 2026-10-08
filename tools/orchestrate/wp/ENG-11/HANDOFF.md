# ENG-11 handoff: an honest merge gate for fixture- and weight-dependent tests

Branch `wp/ENG-11` from main `1c344175`. Worker: Claude Opus 5.5.

## Summary

The RAW-fixture helper that ENG-6 duplicated in image-core and pipeline-cpu is
now one dev-only crate, `crates/test-fixtures` (`publish = false`, used only
as a `[dev-dependencies]` entry). Every test in the workspace that used to pass
silently when `fixtures/raw` or cached model weights were absent now uses it:

- RAW fixtures (`test_fixtures::raw`): every fixture present runs; an absent
  one prints an uncaptured `test <name> ... SKIPPED: ..` line, and
  `TESSERA_REQUIRE_RAW_FIXTURES` (set in CI after `fixtures/fetch.sh`) makes
  it a failure.
- Model weights (`test_fixtures::models`): the same pattern with
  `TESSERA_REQUIRE_MODEL_WEIGHTS` (the older `TESSERA_REQUIRE_MODELS`, which
  ml-faces honoured, still counts). CI does not download weights
  (`.github/workflows/ci.yml` only runs `fixtures/fetch.sh`), so CI does not
  set it.
- Opt-in checks whose input never comes from the repository or CI (private
  Smart Preview sample, a user JPEG, the CFA training venv, the CoreML download
  audit) print `test <name> ... SKIPPED (opt-in): ..` and never fail.

The pipeline-gpu fixture gate now runs all five cameras by default (it ran the
Sony ARW only). All five pass the unchanged tolerances. No test that now runs
for the first time failed, so nothing was marked `#[ignore]`.

No assertion, tolerance, golden or fingerprint changed.

## Newly exposed failures

None. Tests that now run where they did not before, and their result:

| Test | Before | Now |
|---|---|---|
| pipeline-gpu `fixture_level3_tolerance_per_operator_and_output` | ARW only (CR3, RAF, NEF, DNG never compared) | all five pass. Max linear error: CR3 1.25e-6, RAF 3.58e-6, NEF 1.02e-5, DNG 1.38e-5, ARW 1.20e-5 (tolerance 1e-4); display 1 code (tolerance 1); DeltaE2000 at most 3.8e-3 (tolerance 0.5) |

With fixtures present every other converted RAW test already ran before ENG-11
(the silent path only triggered when fixtures were missing), and they all pass.
The weight tests cannot run here: no model cache exists on this machine (see
"Not verified" below).

## Item table (finding → code → test)

| # | Finding | Code | Test / evidence |
|---|---|---|---|
| 1 | `raw_fixtures.rs` duplicated verbatim in image-core and pipeline-cpu | `crates/test-fixtures/src/raw.rs` (moved, behaviour identical: same location variables `PIPELINE_RAW_FIXTURES` → `RAW_DECODE_FIXTURES` → `fixtures/raw`, same SKIPPED text, same failure, same `selected` filter). The six users now `use test_fixtures::raw as raw_fixtures;`. New: `file`, `files`, `with_extension`, `current_test()` | crate unit tests; all image-core/pipeline-cpu fixture tests pass |
| 2 | The gate runs from worktrees whose `fixtures/raw` is a symlink | `raw::root()` is resolved from the crate's own manifest (`crates/test-fixtures/../../fixtures/raw`); `read_dir`/`is_file` follow the symlink | `scan_follows_a_symlinked_fixture_directory`, `default_root_is_the_repository_fixtures_directory`; this worktree's own `fixtures/raw` is such a symlink and every fixture test ran |
| 3 | RAW tests outside image-core/pipeline-cpu returned silently | see "Converted tests" | `PIPELINE_RAW_FIXTURES=/nonexistent` → SKIPPED line; plus `TESSERA_REQUIRE_RAW_FIXTURES=1` → failure (checked on raw-decode `fixtures_decode`) |
| 4 | pipeline-gpu `fixtures.rs` ARW-only unless `PIPELINE_GPU_ALL_FIXTURES=1` | `fixtures(test)` = `raw::selected(test, "PIPELINE_GPU_FIXTURES")`; `PIPELINE_GPU_ALL_FIXTURES` removed; `OPERATORS.md` updated | all five cameras pass (table above) |
| 5 | Weight tests printed SKIP and passed | `test_fixtures::models::skipped` | `TESSERA_REQUIRE_MODEL_WEIGHTS=1` run, see Gates |
| 6 | ml-faces `models.rs` printed one SKIP for the first test only (OnceLock); later tests returned silently | the OnceLock now stores the skip reason and every caller reports it | same |
| 7 | Gate tooling | No script under `tools/orchestrate` runs `cargo test` (`swift-gate.sh` already refuses a missing `fixtures/raw`; `run-luna.sh` symlinks it). `ci.sh` is run by CI, which already sets `TESSERA_REQUIRE_RAW_FIXTURES=1`. Nothing added (no new infra) | — |

## Converted tests

RAW fixtures (`test_fixtures::raw`):

- raw-decode `src/lib.rs` `fixture_tests::fixtures_decode`
- libraw-ffi `src/lib.rs` `fixture_decode` (now only RAW extensions, not every
  file in the directory); `src/sensor_tests.rs`
  `fixture_previews_select_largest_jpeg_not_bitmap`
- index `tests/catalog.rs` `raw_fixtures_scan_incrementally_when_available`
- tessera-cli `tests/raw_workflow.rs` `raw_workflow`
- pipeline-gpu `tests/local_tone_resident.rs`
  `preview_approximation_is_bounded_on_real_fixtures`
- pipeline-gpu `tests/fixtures.rs`
  `fixture_level3_tolerance_per_operator_and_output` (all cameras; also the
  ignored benches `bench_full_level2_tone_only_gpu_vs_cpu`,
  `bench_full_level2_m2_chain_gpu_vs_cpu`)
- tessera-ffi `tests/develop.rs` (via `fixture()`/`harness()`):
  `process_version_is_undoable_and_persisted`,
  `as_shot_sliders_round_trip_on_every_fixture_and_single_slider_touch`,
  `session_renders_into_surfaces_and_persists_undoable_edits`,
  `edited_previews_follow_the_recipe_hash`,
  `panels_crop_masking_detail_and_history`,
  `slow_interactive_frames_are_not_starved`,
  `export_batch_does_not_starve_slider_drag`, and every other `harness()` user
  in the file
- tessera-ffi `tests/document.rs` `open_document_from_image_is_the_developed_raw`
- tessera-ffi `tests/export.rs` `raw_export_with_the_web_preset_and_a_binned_print_render`
- tessera-ffi `tests/hdr.rs` (via `fixture()`):
  `edr_ring_renders_headroom_and_sdr_ring_stays_bit_identical` and every other
  user
- tessera-ffi `tests/masks.rs` (via `fixture()`/`Open::new`):
  `gradient_brush_range_overlay_undo_and_persistence`,
  `ai_masks_segment_in_a_job_and_render_through_the_mask_cache`,
  `subject_mask_on_the_canon_fixture_with_cached_models`
- tessera-ffi `tests/eng7c_legacy_smart_preview.rs` (already loud; now uses the
  shared helper)
- tessera-ffi `tests/document_perf.rs` `bench_p17_document_frames_during_photo_export` (ignored bench)
- image-core / pipeline-cpu ENG-6 users unchanged apart from the import

Model weights (`test_fixtures::models`):

- filters `tests/remove_models.rs` (both `registry()` users)
- ml-faces `tests/models.rs` (all three `registry()` users),
  `tests/multiface_cached.rs` `generated_multiface_cached_models`
- ml-segment `tests/models.rs` `cached_models_segment_synthetic_and_fixture`
- ml-caption `tests/florence.rs` `cached_caption_ocr_and_partition_report`,
  `tests/siglip.rs` `cached_red_disc_ranks_circle_and_red_in_top_five`
- ml-embed `tests/siglip.rs` `cached_siglip_synthetic_similarity_and_cube_retrieval`
- ml-enhance `tests/offline.rs` `optional_cached_models_execute_without_downloads`
  (each skipped part reported), `tests/models.rs` (2), `tests/model_seams.rs`
  (2), `tests/denoiser.rs` (all six `registry()` users)
- ml-filters `tests/jpeg.rs` `cached_q30_jpeg_improves_psnr`,
  `tests/colorize.rs` `real_cached_ddcolor_cpu`, `tests/bench.rs` (ignored benches)
- image-core `tests/automatic_post_denoise.rs`, `tests/ml_denoise.rs`
- export `tests/upscale.rs` (2, each skipped factor reported),
  `src/denoise_tests.rs` `drunet_raw_is_nonblack_reduces_flat_noise_and_matches_preview`
- tessera-cli `tests/export.rs` `cached_models_cli_upscale_smoke`
- ml-depth `tests/model.rs` `cached_model_fixture_and_partition` (ignored)
- tessera-ffi `tests/masks.rs` `subject_mask_on_the_canon_fixture_with_cached_models`

Opt-in, visible but never a failure (`test_fixtures::opt_in_skipped`):

- raw-decode `tests/smart_preview.rs`, export `tests/lrcat_jxl.rs`
  `private_sample_cpu_render` (`TESSERA_SMART_PREVIEW_SAMPLE`, a private file)
- ml-depth photograph part (`TESSERA_DEPTH_FIXTURE`)
- ml-enhance `tests/cfa_model.rs` (training venv; disabled under CI)
- ml-runtime `tests/guard.rs` `registered_models_coreml_guard`
  (`TESSERA_REQUIRE_COREML=1` downloads and audits every model)

## Deliberately not converted

- Tests that already fail loudly when a fixture is missing (`unwrap` on open,
  `expect`, `assert!(is_file)`): previews `src/raw.rs`, `src/lib.rs`,
  `tests/revision.rs`; tessera-ffi `fallback.rs`, `lrcat_mask_tests.rs`;
  pipeline-cpu `white_balance_v2`, `lens_fixtures`; image-core `linear_dng`;
  raw-decode `baseline`; pipeline-gpu `eng3_luminance`, `resident_large`,
  `operator_performance`; ml-embed `job.rs`; the RAW parts of the ml-faces,
  ml-segment, ml-caption and ml-embed weight tests.
- Ignored benchmarks that print `NO TIMINGS` or `INTERACTIVE_BENCH_SKIP`
  (pipeline-gpu `cfa_resident`, `interactive_performance`), and the criterion
  bench `raw-decode/benches/decode.rs`.
- Hardware skips (no Metal device, `CI` set for the two latency tests in
  tessera-ffi `develop.rs`), and tessera-ffi `enhance.rs`
  `missing_weights_are_an_offline_job_error_without_output`, which skips when a
  cache *is* configured. They are not fixture or weight absences.

## Cargo.lock

The common rules forbid Cargo.lock changes, but the task asks for a shared
crate, which needs one. The diff only adds the `test-fixtures` package (no
external dependencies) and `test-fixtures` to the dependency lists of the
crates that use it. No version of any existing package changed.

## Not verified

- No model weight cache exists on this machine, so the cache-based weight tests
  were only exercised in their skip and require-failure modes, never with
  weights. (ml-faces resolves its models through the registry download and
  ran normally.)
- `TESSERA_REQUIRE_MODEL_WEIGHTS` fails ml-faces `models.rs` only when the
  network download fails; with network access those tests download into
  `crates/ml-faces/.model-cache` as before.

## Gates

All on `b3a066ce` (the HANDOFF commit only adds this file), with
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-11`, `CARGO_BUILD_JOBS=5`,
`RAYON_NUM_THREADS=5`, after `cargo clean --release -p` of every touched crate
(179 files removed).

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | exit 0. 687 suites: 3544 passed, 0 failed, 107 ignored (load 6.5 at start, 25.1 at end). 32 SKIPPED lines, all weight or opt-in skips; **no RAW SKIPPED line** |
| `TESSERA_REQUIRE_RAW_FIXTURES=1 cargo test --release --workspace --no-fail-fast` | 3543 passed, 1 failed, 107 ignored; no RAW SKIPPED line, so nothing silently skips. The one failure is unrelated: ml-embed `vector::hnsw_top_five_matches_exact_for_seeded_thousand_vectors` (`left: ImageId(..d9)`, `right: ImageId(..2c7)`, top-1 mismatch). `hnsw_rs` assigns random levels, so the graph is not deterministic; the file is untouched by ENG-11 and passed in the default run. Rerun serialized with `TESSERA_REQUIRE_RAW_FIXTURES=1 -- --test-threads=1` five times (load 19.8, 14.3, 13.4, 11.9, 11.1): 5/5 passed. Not weakened; reported as a pre-existing flake |
| `TESSERA_REQUIRE_MODEL_WEIGHTS=1` (ml-enhance `denoiser`, `models`) | fails, naming each test and the missing cache, as intended |
| `TESSERA_REQUIRE_RAW_FIXTURES=1 PIPELINE_RAW_FIXTURES=/nonexistent` (raw-decode `fixtures_decode`, tessera-ffi `open_document_from_image_is_the_developed_raw`) | fails, as intended; without the require variable prints SKIPPED and passes |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0; worktree clean afterwards (no bindings drift) |
| `tools/orchestrate/swift-gate.sh` | `SWIFT GATE OK`. XCTest: 996 executed, 3 skipped, 0 failures; Swift Testing: 5 passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | exit 0 (only the existing `ld` macOS-version warning for `blake3_neon.o`) |

## Commits

- `8b475e69` test(ENG-11): move the raw-fixture helper into a shared dev-only crate
- `afc3604f` test(ENG-11): stop RAW-fixture tests outside image-core/pipeline-cpu passing silently
- `2c7ed9e3` test(ENG-11): run the pipeline-gpu fixture gate on every camera by default
- `b3a066ce` test(ENG-11): make ML-weight tests report skips and honour TESSERA_REQUIRE_MODEL_WEIGHTS
- this HANDOFF commit
