# LR-5 — AI masks, Machine B

## LR-5b rebase-only integration — 2026-10-02

Current base: `origin/wp/LR-4-parametric-masks` at
`5e0633e1217df25c22438b7d5f01cf1c01972f80`. Rebased with
`git rebase --onto origin/wp/LR-4-parametric-masks 4ecf521d` after fetching.
Exactly the three LR-5 commits were replayed, without squashing or duplicating
LR-4 history:

| Original | Rebased | Purpose |
| --- | --- | --- |
| `9f73a838` | `9a7f2382` | LR-5 tests |
| `dc8df0a8` | `9026873f` | LR-5 implementation |
| `ece8be49` | `25273c9b` | LR-5 documentation |

### Conflicts and integration checks

- `crates/engine-api/src/recipe/schema.rs`: retained both the predecessor's
  `point_colors` predicate and LR-5's `adobe_ai_mask` predicate, together with
  all existing predicates and tests. No schema constant or first-lane checklist
  changes.
- `crates/import-lrcat/README.md`: kept the complete LR-2e and LR-1 sections,
  then appended the LR-5 resource-injection section.
- `crates/export/src/depth.rs` merged automatically; its render hook retains
  predecessor arguments and adds LR-5's explicit mask support root.
- Translation matrix merged automatically. A row-by-row comparison confirms
  all 118 base rows remain, with only six LR-5 rows changed. Main's shared
  `approximate` guard is untouched.
- All predecessor importer hooks remain. LR-7's `geometry::finish` still owns
  the single Import entry. `diagnostics::Entry.field` remains `Option<String>`;
  the new integration test reads it with `as_deref()` through `entries()`.

### Combined synthetic integration test

Integration-test and generated-bindings commit: `fecd699f`.

`lr5b_combined_lanes_resolve_and_regenerate_in_one_import` in
`crates/tessera-ffi/src/lrcat_mask_tests.rs` imports one invented Lua row with
LR-1 Point Color, LR-2 monochrome/mixer, LR-4 display-domain luminance range,
two LR-5 AI subject masks, and LR-7 Upright keys. The existing APPLY helper
receives resolver bytes for one subject and no bytes for the other. The test
checks the durable raster, pending regeneration, schema 4 serialization, one
Import-authored history entry, undo/redo, JSON round trip, and diagnostics for
all five lanes through `entries()`, including exactly one regenerated note.
CPU export uses the stored raster and invokes an injected subject segmenter
exactly once for the missing raster. Output must be finite and differ from a
no-local-adjustments baseline; rendering must preserve diagnostics/history.
No model inference or user catalog is needed.

### LR-5b gate evidence

Environment for all LR-5b gates:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-4-parametric-masks
export CARGO_BUILD_JOBS=4
export RAYON_NUM_THREADS=4
cargo clean -p engine-api -p export -p import-lrcat -p mask-ai -p mask-store -p ml-depth -p pipeline-cpu -p pipeline-gpu -p sidecar -p tessera-ffi -p tessera-mcp
cargo test --release -p import-lrcat -p engine-api -p mask-store -p mask-ai -p pipeline-cpu -p pipeline-gpu -p sidecar -p image-core -p merge -p export -p previews -p tessera-ffi -p tessera-mcp
cargo clippy --release --workspace --all-targets -- -D warnings
cargo fmt --all --check
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

- **PASS:** full release Rust gate, 1,748 top-level test passes plus two
  successful child-process tests; 59 existing ignored tests, zero failures.
  The child-process harnesses internally filter nine tests each; the gate
  command supplies no exclusions or filters. Repository RAW fixtures are
  enabled, and model inference remains opt-in.
- **PASS:** workspace release Clippy, all targets, `-D warnings` (21.16 s).
  Existing LibRaw C/C++ build-script warnings do not represent Rust/Clippy
  warnings and did not fail the gate.
- **PASS:** `cargo fmt --all --check`.
- **PASS:** `apps/mac/build-ffi.sh`; generated C and Swift bindings now expose
  the existing LR-5 mask resolver and apply method. These generated artifacts
  are included with the integration-test commit, with no new API implementation.
- **PASS:** Swift gate printed `SWIFT GATE OK`; debug build completed in
  38.66 s, XCTest executed 920 tests with three skipped and zero failures in
  199.697 s, and Swift Testing passed five tests in two suites.
- **PASS:** strict release `Tessera` build, complete concurrency checking and
  Swift warnings-as-errors, exit 0 (144.29 s). The linker emitted a warning
  that the bundled `blake3_neon.o` was built for macOS 26.2 while linking for
  15.0; this is a linker deployment-target warning, not a Swift diagnostic.
  This pass does not establish runtime compatibility on older macOS versions.
- Generated C/Swift bindings byte-match the bindgen output. Handwritten-source
  `git diff --check` passes; generated bindings retain bindgen's whitespace.

The initial focused release test also passed. All fixtures added by this pass
are synthetic. No GUI was opened, no real catalog was read, and nothing was
pushed. No Cargo manifest, Cargo.lock, board.json, dependency, or main-owned
first-lane checklist was changed. `LR-RULINGS-FROM-A.md` remains untracked.

Local gate logs: `/tmp/lr5b-clean.log`, `/tmp/lr5b-focused.log`,
`/tmp/lr5b-release-tests.log`, `/tmp/lr5b-clippy.log`, `/tmp/lr5b-fmt.log`,
`/tmp/lr5b-swift-gate.log`, and `/tmp/lr5b-swift-strict.log`.

---

## Original LR-5 implementation record (historical)

Branch `wp/LR-5-ai-masks`, local only. Base
`4ecf521de9583f8bf3d962ec6ff4cbaa72591004` includes LR-4 through LR-4e and
`origin/main` `486d069f` (including LR-SCHEMA, LR-DIAG and LR-7). The required
`e37de957` target is already an ancestor; no rebase or sibling merge was needed.
`origin/wp/LR-6-lens-blur` was inspected read-only for the LR-6e storage pattern.

## Commits and evidence

- RED `9f73a8389c26fbff461f115d03c5e6cc3f68c0d2` — `test(LR-5): require AI mask decoding and durable raster pins`.
  Release importer test failed on unsupported `Mask/Image`; mask-store test failed
  to compile because `put_pinned` / `remove_pinned` did not exist.
- GREEN `dc8df0a82d23a90ad463a55dadd1b87494ec6852` — `feat(LR-5): import AI mask resources with bounded pins and regeneration`.
- The following `docs(LR-5):` commit records the final evidence and matrix rows.

## Implementation

`MaskComponent.adobe_ai` is one additive optional recipe object, absent by default
and omitted when absent. It carries category, opaque resource ID, optional store
key and regeneration state. The `adobe_ai_mask` v4 predicate covers the object,
including disabled/history trees, with the shared bumped-only-when-present test.
`RECIPE_SCHEMA_VERSION` remains 3; no first-lane checklist changes were duplicated.
The MCP schema extractor includes the new nested type; no dependency was added.

The sidecar codec decodes recognized `Mask/Image` subtypes and explicit AI kinds;
import-lrcat audits the entire parent, retains exact Lua/XMP source and calls the
shared diagnostics helper. All new matrix mappings are `approximate`. Person
sub-parts map to subject; each part's reason states that regeneration cannot
isolate it. Objects require a box or reference-point prompt. Numeric subtype 0
with a prompt is an approximate object interpretation, not verified Adobe parity.
Unknown/malformed forms remain untranslated. Nested, disabled, inversion and
ordered add/subtract/intersect semantics remain LR-4's.

`LrcatImport::apply_with_mask_resolver` accepts an optional caller-owned opaque-ID
resolver. It resolves only during APPLY through `ml_segment::MaskStore`, which
already re-exports mask-store. Resolved full sensor-aligned grayscale PNG/TIFF
rasters are stored before recipe publication. IDs never become paths, and the
resolver must expand Adobe crop/origin data before returning bytes. No proprietary
helper is opened by this lane. Ordinary `apply` leaves regeneration pending.

Pins live at `<Tessera support>/imported-masks/pinned`, keyed by stable image ID
and compact resolved-raster slot. The per-image bound is **256 rasters and 256 MiB
total including headers/checksums**. Size and grayscale/extent checks precede
raster allocation; excess resources remain pending. Re-import replaces slots and
removes obsolete pins. Failure before publication restores prior usable slots.
Atomic replacement uses one extra raster-sized temporary file per writer.
`Engine::forget_missing` removes pins only for absent image records and retains
pins when the original still exists. Inference LRU eviction cannot remove pins.

Import attachment updates the existing Import history entry, not a new entry.
User edits, JSON and native XMP retain references. Resolution removes only the
exact pending LR-5 regeneration reason through the shared diagnostics module;
other lanes' entries survive. Resume reports read the published recipe so they
do not report already-resolved masks as pending.

The shared compositor can select by component identity. Preview and export read
the stored raster without inference; same-category components can have different
planes. Preview checks the payload checksum to invalidate in-memory alpha after
same-key replacement. Missing/wrong-size imported references fail explicitly.
Export receives the engine support root explicitly, including print rendering.
Regeneration retains the existing subject/sky/background/prompted segmenter seam;
no person-part request or new model was invented. Rendering mutates neither
import diagnostics nor history. Existing GPU admission excludes these AI kinds.

## Gates

**PASS** on the GREEN tree plus the documentation/matrix update:

- Release test command below exited 0: **1,470 top-level test passes**, plus
  three successful child-process test runs; **53 existing ignored tests**,
  zero failures. The child-process harnesses report 21 internal filtered tests;
  the command supplied no filters, skips or exclusions.
- Clippy, release, all targets, `-D warnings`: passed (13.05 s final run).
  LibRaw's existing C/C++ compiler warnings were emitted by its build script;
  no Rust/clippy warnings remained.
- `cargo fmt --all --check` and `git diff --check`: passed.
- Shared translation-matrix guard and all of its negative tests: passed.
- Unchanged B5-29c full retained-source golden, LR-4 compatibility/retained byte
  pins, streaming equivalence and schema tests: passed; no golden was re-pinned.
- Import streaming memory/time gate and FFI 20,000-image streaming memory gate:
  passed; the latter test finished in 113.37 s. No latency-flake rerun was needed.
- FFI LR-5 end-to-end apply/replace/rollback/resume/forget/export, injected
  category regeneration, 256-slot overflow, corrupt/wrong-size resource fallback,
  same-category distinct-plane rendering and same-key preview invalidation:
  passed. JSON/native-XMP round trips and conditional schema test passed.

Early RED failures and intermediate compile/matrix issues were resolved; no tests
were weakened. In particular, the matrix guard caught explicit AI categories
still carrying the legacy warning; fixing their metadata initialization removed
that warning through the shared approximation path.

Environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-4-parametric-masks"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
cargo test --release -p import-lrcat -p engine-api -p mask-store -p sidecar -p mask-ai -p export -p tessera-ffi -p tessera-mcp -p ml-depth -p pipeline-cpu -p pipeline-gpu
cargo clippy --release -p import-lrcat -p engine-api -p mask-store -p sidecar -p mask-ai -p export -p tessera-ffi -p tessera-mcp -p ml-depth -p pipeline-cpu -p pipeline-gpu --all-targets -- -D warnings
cargo fmt --all --check
```

No command-level exclusions. Repository RAW fixtures are included. Existing
ignored/env-gated model tests remain opt-in; new tests use synthetic rasters and
an injected segmenter. No Swift gate or app launch was performed under the
Machine B override.

## 29c compatibility and merge notes

The decode layer performs no raster I/O or inference. Only recognized AI forms
add state/diagnostics; exact source remains for all approximate translations.
No ordinary field default, recipe format constant, Cargo manifest, Cargo.lock,
board.json or real catalog was changed. `LR-RULINGS-FROM-A.md` remains untracked
and must not be committed. Coordination is this handoff only; nothing was pushed.

LR-6e independently adds the same mask-store pin API. When the coordinator lands
LR-6 first, reconcile `MAX_PINNED_BYTES`, `put_pinned`, `remove_pinned` and pinned
`get` once; retain LR-5's `pinned_revision` invalidation support. Both FFI resolver
entry points need composition at merge; this lane deliberately did not merge or
copy the LR-6 branch. Combine both removal hooks in `forget_missing`. The
coordinator owns final generated Swift bindings for the added resolver callback.

## Evidence limits

Adobe's catalog FAQ identifies `.lrcat-data` as AI-edit storage, distinct from
`.lrdata` previews. Public first-hand XMP examples establish descriptions and
opaque digests, not a documented binary decoder or table association. Links and
supported forms are in `crates/import-lrcat/README.md`. Automatic Adobe blob
association/decoding remains unavailable; the caller must supply decoded bytes.
Person sub-part inference is unavailable. Object descriptions without usable
prompts and unknown subtypes remain retained. No Adobe-rendered synthetic chart
or public DNG+XMP pixel-parity comparison was used, so no new mapping is claimed
as exact. All fixtures and raster values added here are invented.

## LR-5b — Machine A round-2 rework

This section supersedes the rejected LR-5 behavior and the historical gate claims
above. Base: `f84aebdb` (LR-6 restack without LR-5). Ported the original
`5e0633e1..04217312` lane and restored the removed LR-5 export/print plumbing,
combined-lane resolver assertions and matrix rows. LR-6's shared `put_pinned`,
`remove_pinned` and `pinned_revision` functions are preserved.

### Ruling by ruling: status, code location, tests

Every ruling was re-audited against the code by the continuing worker (Claude)
after the first worker (Codex) stopped; "audit" marks what that audit changed.

| Ruling | Status and how | Code location | Tests |
| --- | --- | --- | --- |
| B1 | Done. The extent an injected raster must match is measured with the renderer's own recognizer and decoder: RGB after EXIF orientation, RAW by active sensor area. No unrotated or preview fallback; computed only for an AI-masked image when a resolver is supplied. Audit: the first version read a DNG's embedded preview directory (320x216 instead of 5212x3468) and fell back to unrotated file dimensions. | `tessera-ffi/src/lrcat.rs` `render_mask_extent`, call in `apply_with_resolvers` | `lr5b_portrait_orientation_import_uses_render_extent`, `lr5b_raw_import_extent_is_the_active_sensor_area` |
| B2 | Done. Parse emits no regeneration note; apply pushes it once the resolver outcome is known. `diagnostics::remove_approximate` is deleted (no reference left in the tree); the channel is push-only. | `tessera-ffi/src/lrcat_masks.rs` `apply` (`REGENERATED`); `import-lrcat/src/mask_source.rs` | `lr5_ai_categories_are_approximate_and_source_is_retained` (no note at parse), `lr5_invalid_resource_is_pending_and_slots_are_compact`, `lr5_apply_pins_replaces_rolls_back_and_removes_masks_with_image`, `lr6f_all_lanes_one_apply_both_resources_and_both_absent` |
| B3 | Done. If any enabled AI leaf of an adjustment has no pixels (pending, failed, no model) the whole adjustment rasterizes to zero before inversion or subtraction, in preview, thumbnail overlay, file export and print. The mask UI reports pending / "Unavailable", including nested groups. | `tessera-ffi/src/masks.rs` `group_available`, `Hooks::rasterize`, `thumbnail_raster`, `component_ai_state`; `export/src/ai_masks.rs` `ReadyMasks::rasterize`, `render_with_hooks` | `lr5b_unavailable_inverted_and_subtracted_ai_has_zero_effect_and_pending_ui`, `lr5b_nested_unavailable_mask_reports_pending_ui`, `lr5b_export_without_model_skips_inverted_and_subtract_adjustments`, `lr5b_ffi_print_and_file_export_without_model_skip_unavailable_ai` |
| M1 | Done. README and matrix state in bold that the app regenerates AI masks and does not read Adobe rasters; the resolver is described as a caller injection interface the app does not supply. | `crates/import-lrcat/README.md` "LR-5b AI masks"; `docs/coordination/LR-TRANSLATION-MATRIX.md` LR-5b note and `Mask/People` row | matrix guard `translation_matrix` (import-lrcat suite) |
| M2 | Done. Person sub-parts, People/Person and a specific person instance are retained as unsupported with a warning, never rendered as Subject. Audit: a nonzero `MaskSubCategoryID` on Subject/Sky/Background (numeric subtype, `MaskType`, or `What='Mask/Subject'`) was still widened to the whole category; now unsupported. | `sidecar/src/masks.rs` `import_component` (`adobe_ai` branch), `reject_part_id` | `lr5b_person_parts_and_specific_people_are_unsupported`, `lr5b_unverified_part_ids_are_never_broadened_to_the_whole_category`, `lr5b_part_ids_on_named_ai_masks_are_unsupported` |
| M3 | Done. Rasters are immutable blobs keyed by a hash of dimensions and quantized samples. During apply the image owns the previous and the new keys; a failed publication restores the record and returns the original error even when the rollback itself fails (the LR-6 depth rollback in the same closure no longer `?`-masks it either). | `mask-store/src/lib.rs` `MaskRaster::content_key`, `put_content_pinned`; `tessera-ffi/src/lrcat_masks.rs` `apply`; `tessera-ffi/src/lrcat.rs` rollback after `result.is_err()` | `lr5b_content_keys_preserve_previous_recipe_across_reimport_and_failed_publish`, `lr5_apply_pins_replaces_rolls_back_and_removes_masks_with_image`, `lr6f_all_lanes_one_apply_both_resources_and_both_absent` |
| M4 | Done. An image without AI masks returns before any store is opened. Content pin writes never list a directory; the shared `put_pinned` skips the eviction listing for the unbounded pinned store. Audit: forgetting any image listed the owner and pin directories once per image; removal is now one batch that lists only when an ownership record was actually removed. | `tessera-ffi/src/lrcat_masks.rs` `has_masks`, `apply`, `remove_images`; `tessera-ffi/src/lrcat.rs` (`has_masks` guard); `mask-store/src/lib.rs` `put`; `tessera-ffi/src/changes.rs` `forget_missing` | `lr5b_no_ai_masks_do_not_access_store`, `lr5b_pin_write_does_not_enumerate_the_directory`, `lr5b_removing_images_without_ai_masks_does_not_scan_the_store` |
| M5 | Done. The session keeps each loaded plane by key; a ready key is never reopened or re-checksummed. | `tessera-ffi/src/masks.rs` `refresh_imported`, `ensure_ai_jobs` | `lr5b_imported_raster_cache_does_not_reopen_between_frames` (backing directory deleted after the first load) |
| M6 | Done. A missing, corrupt or wrong-extent stored raster requests regeneration with a diagnostic in preview and export; with no model the adjustment stays skipped. `Engine::prune_missing` removes missing owners and orphaned blobs. Audit: (1) a plane regenerated under an imported key at the proxy level was never rendered and a wrong-extent raster was reported ready while unused; (2) ownership records only grew, so a superseded raster was never an orphan and could not be reclaimed while its image lived. The record is now trimmed to the published recipe after a successful publication; import still never deletes or lists. | `tessera-ffi/src/masks.rs` `refresh_imported`, `group_available`, `AiMaskJob`; `export/src/ai_masks.rs` `render_with_hooks`; `tessera-ffi/src/lrcat_masks.rs` `apply`, `prune_missing`; `tessera-ffi/src/changes.rs` `Engine::prune_missing` | `lr5b_missing_stored_raster_regenerates_with_diagnostic`, `lr5b_file_export_surfaces_missing_raster_regeneration_notice`, `lr5b_regenerated_imported_plane_renders_at_proxy_extent`, `lr5b_wrong_extent_stored_raster_is_not_ready_and_requests_regeneration`, `lr5b_regenerated_imported_job_invalidates_live_frame`, `lr5b_prune_missing_collects_orphans_and_preserves_shared_live_content`, `lr5b_superseded_rasters_are_orphans_reclaimed_by_explicit_prune` |
| M7 | Done. AI import pins are u16 samples (`TSMASK02`, 48 bytes of header and checksum). LR-6 depth pins keep f32 and their API. | `mask-store/src/lib.rs` `compact_payload`, `get` | `lr5b_imported_rasters_use_two_bytes_per_pixel`, `lr6f_all_lanes_one_apply_both_resources_and_both_absent` (2 bytes per mask sample, 4 per depth sample) |
| M8 | Done. Preview, file export and print pass the engine's app directory; the AI mask renderer uses that directory (callers outside the app may set `TESSERA_APP_SUPPORT`) and no longer defaults to the home directory. MCP export rejects active AI masks before rendering. Audit: Open Developed Image also renders through the export path and now passes the same directory. | `tessera-ffi/src/masks.rs` `ensure_ai_jobs`; `tessera-ffi/src/export.rs` (`mask_support`, print call); `tessera-ffi/src/document/io.rs` `open_image`; `export/src/lib.rs` `ExportSettings.mask_support`, `render_pixels_with_mask_support`; `tessera-mcp/src/exports.rs` | `lr5b_ffi_print_and_file_export_without_model_skip_unavailable_ai` (stored pixels under the explicit root, no override), `lr5b_document_from_image_reads_imported_masks_from_the_engine_app_dir`, `lr5_apply_pins_replaces_rolls_back_and_removes_masks_with_image`, `lr5b_mcp_export_rejects_ai_masks_before_rendering` |
| M9 | Out of scope by ruling. | none | none |

### Decisions for Machine A to confirm

- **Failed segmentation in export.** "No model" is unavailable and skips the whole
  adjustment with a warning (B3). A backend that runs and then fails, or returns
  an invalid raster, stays an export error with nothing published. The first
  worker had turned that into a skip too, which broke the pre-existing
  `invalid_or_failed_segmentation_never_publishes_image_or_sidecar`. The
  existing assertion was kept and the behaviour restored.
- **No automatic model download.** `mask_ai::load_segmenter` now opens the model
  registry with downloads disabled, in line with the repository rule that
  automatic renderers stay offline. This function is shared by the interactive
  mask session and Select Subject, so on a machine without the segmentation
  weights a native Subject/Sky mask now reports "Unavailable" until the weights
  are installed from Settings, instead of downloading on first use.
- **Superseded rasters.** Import never deletes. A raster replaced by a successful
  reimport stays on disk as an orphan until `Engine::prune_missing(false)` runs.
  `lr6f_all_lanes_one_apply_both_resources_and_both_absent` therefore asserts
  removal of the old mask raster after an explicit prune (the depth slot is
  still replaced in place), and its per-sample size is 2 for masks, 4 for depth.
  That test was already failing at `8323df62` on both points; no assertion was
  removed.
- **Reimport without any AI mask.** M4 forbids a store call, so the ownership
  record of an image that had masks before is kept until the image is removed.
- Open Developed Image with a mask that needs a model that is not installed
  still fails with an error, as on main.

### Test-first attempt ledger

- Import RED: 1 passed, 2 failed (premature regeneration note and person broadening).
  GREEN: 3 passed.
- FFI unavailable-only RED: 0 passed, 1 failed; inverted missing alpha was `[1,1]`.
- FFI initial LR-5b RED: 1 passed, 6 failed (portrait extent, inversion, cache reopen,
  content-key collision, f32 storage, and no-AI store access).
- Export RED: initial test compile typo corrected; then 0 passed, 2 failed.
  GREEN: 2 passed (regeneration and unavailable-model handling).
- Pin scan RED: 0 passed, 1 failed. GREEN: both pin tests passed.
- MCP export RED: 0 passed, 1 failed. GREEN: 1 passed.
- FFI initial fix run: 11 passed, 2 failed (new cleanup behavior still RED).
  Cleanup fix run: 13 passed.
- FFI print/file RED attempt 1 caught a synthetic fixture missing its image ID;
  corrected fixture, then attempt 2 failed for the intended uncached-model error.
- File-export notice RED: 0 passed, 1 failed (internal notice not propagated).

### Continuation audit ledger (second worker)

The first worker's last statement was "targeted regressions are green"; its full
gates had only reached the clean step. Each gap below got a RED commit first.

Commits are named by subject because the lane was rebased afterwards.

- RED "require RAW sensor extent, regenerated-plane rendering and part-ID
  retention": import `lr5_ai` 3 passed, 1 failed; FFI `lr5b` 10 passed, 3 failed
  (RAW extent 320x216 vs 5212x3468, regenerated plane all zero, wrong-extent
  raster ready). GREEN "measure the render extent, render regenerated planes and
  retain part IDs": 4 passed; 13 passed.
- RED "require explicit app dir for document open and part IDs on named masks":
  import `lr5_ai` 4 passed, 1 failed; FFI `export` `lr5b` 1 passed, 1 failed.
  GREEN "open developed documents from the engine app dir and reject part IDs on
  named masks": 5 passed; 2 passed, also with `TESSERA_APP_SUPPORT` unset.
- RED "require scan-free removal of images without imported masks": FFI `lr5`
  18 passed, 1 failed. GREEN "remove image mask ownership in one batch without
  listing the store". The wider `lr` filter then ran the combined all-lane test
  for the first time in this lane: it was already failing at `8323df62`.
- RED "require superseded rasters to be reclaimable and align the combined test
  with u16 pins": FFI `lr` 38 passed, 2 failed. GREEN "own only the published
  recipe's rasters so pruning reclaims superseded content": 40 passed.
- Full gate attempt 1 (before the last fix): release tests 321 result lines ok,
  1 failed (`export`
  `invalid_or_failed_segmentation_never_publishes_image_or_sidecar`); workspace
  clippy failed with 2 `useless_conversion` errors in
  `tessera-ffi/src/masks.rs`; fmt clean. Both fixed in "keep failed or invalid
  segmentation an export error and clear clippy" without touching the test.

### Final gates (attempt 2, on the final code, docs as committed)

Environment: `CARGO_BUILD_JOBS=5 RAYON_NUM_THREADS=5`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-5b`, and `TESSERA_APP_SUPPORT`
pointing at an empty scratch directory so no test could reach the live library.
`cargo clean -p import-lrcat -p sidecar -p engine-api -p export -p mask-ai
-p mask-store -p tessera-ffi -p tessera-mcp` ran first. No test was rerun, no
bound was changed and there is no command-level exclusion.

| Gate | Result |
| --- | --- |
| `cargo test --release --no-fail-fast -p import-lrcat -p sidecar -p engine-api -p image-core -p pipeline-cpu -p pipeline-gpu -p filters -p previews -p export -p tessera-ffi -p tessera-mcp` | exit 0; 315 result lines, 1948 passed, 0 failed, 67 ignored |
| `cargo test --release --no-fail-fast -p mask-store -p mask-ai` (touched, not in the required list) | exit 0; 4 passed, 0 failed |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0; regenerated `CTesseraFFI.h` and `TesseraFFI.swift` (additions only: `LrcatMaskResolver`, `applyWithMaskResolver`, `applyWithResolvers`, `Engine.pruneMissing`), committed |
| `tools/orchestrate/swift-gate.sh` | `Executed 920 tests, with 3 tests skipped and 0 failures (0 unexpected)`; Swift Testing 5 tests in 2 suites passed; `SWIFT GATE OK` |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | exit 0, `Build of product 'Tessera' complete!` |

Import goldens: `import-lrcat` `tests/golden.rs` passes and the lane changes no
golden, fixture or pinned file (`git diff --stat` against the base lists none);
no golden was re-pinned. No `Cargo.lock` or `board.json` change. All fixtures
and mask pixels are synthetic; the RAW extent test reads the repository's
`fixtures/raw` files only.

After these gates the lane's own commits were rebased from `f84aebdb` onto
`4dba1640` (identical tree, four reworded messages below it); the rebased tree was
checked to be byte-identical to the gated one.
