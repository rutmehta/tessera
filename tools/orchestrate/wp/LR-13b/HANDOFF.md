# LR-13b — proxy correctness and library follow-through

Status: STEP 1 and STEP 2 complete; STEP 3 partly complete (M5, M6, M7,
minors, LR-8h); M4, M8, M10 not done (see table). Base: `2ee8bfc6` (`wp/LR-8R`). Binding rulings:
`A-LR13-REVIEW.md` and `A-LR8-REVIEW.md`, read completely.
Synthetic fixtures only. No foreground GUI or system-settings changes.
No board or lockfile changes. Commits up to `0875ac46` are by a Codex worker
(no trailers); later commits are by Claude Opus 5.5 (Co-Authored-By trailer,
"Mixed authorship" noted where finishing the Codex worker's uncommitted work).

## Findings and coverage

| Finding | Code | Test / status |
| --- | --- | --- |
| LR-13c deferred, bounded and cached near-duplicate work | Ported four commits in order; no conflicts | Initial open RED: 32 pixel calls vs required 0; 879 passed, 0 failed, 42 ignored |
| Proxy export/print retain imported AI/depth masks; unavailable AI is an error | `export::proxy_recipe`, `ai_masks::ready_masks` and process-aware proxy hook | RED: 2 failures; GREEN: 2 passed (Native and Adobe, AI and cached depth) |
| Proxy lens blur and retouch use actual dependencies | Resource-aware plan; CPU/Adobe retouch and pre-geometry depth hooks; Develop/export/thumbnail routes. Follow-up (Claude): Develop's `ignored_settings`/`render_notices` now judge the plan with the session depth provider (`Shared::proxy_plan_fields`), which every render snapshot installs; before, Lens Blur was rendered yet reported as omitted | 3 integration tests + 1 thumbnail test, RED then GREEN. Follow-up found by the full FFI run: `tests/lrcat.rs::lr13_imported_jxl_proxy_reaches_app_preview_analysis_and_develop` failed ("Develop optional settings failed") at the Codex tip. **Expectation changed per A-LR13 item 2**: before, `lens_blur` was in `ignored_settings` with notice "Lens Blur is not rendered on Smart Preview yet." and the frame succeeded; after, Lens Blur is neither ignored nor announced, exactly as for originals (an ordinary JPEG original with `lens_blur {}` and no depth weights produces no frame and no error within 60 s too). The other three notices and their frame are asserted unchanged |
| Proxy mask raster errors surface in thumbnails/export | Proxy scalar hook propagates errors; thumbnails reject unavailable imported planes | Core 1 test + FFI 1 test RED; core 1 and FFI 2 GREEN (including effects regression) |
| Embedded-profile parse errors do not block Native proxy exports | `tessera-ffi/src/export.rs` `Source::open(path, orientation, process)`: embedded profile read only for `ProcessFamily::Adobe`; call sites in export/print/document pass `recipe.process_version`. Minimal call-site change, no DCP/BaselineExposure code touched. `export::needs_segmenter` excludes depth components (served from the depth cache) | `tessera-ffi/tests/lr13b_proxy_bridge.rs`: Native print+export with a pixel-valid TIFF field the profile reader rejects; cached depth range exports without a segmentation model. RED 0/2, GREEN 2/2 |
| Persistent notice survives status changes without per-frame FFI locking | `DevelopController.renderNotices` cached, refreshed on open/history/commit/non-interactive flush/mask flush (`refreshRenderNotices`); `AppModel.developRenderNoticeList` + `developRenderNotices(for:)`; `SmartPreviewLoupeBadge` (`loupe.smart-preview-notice`, badge label lists notices) | `SmartPreviewNoticeAccessibilityTests` (2) restored from 1f64ff6f; `DevelopRecoveryAdmissionBehaviorTests.testProxyRenderNoticeOwnsOnlyItsPhotoAndPreservesNewerStatus` adds zero `renderNotices()` calls across 5 frame callbacks and the persistent-list assertions. RED compile; GREEN 3/3 |
| Thumbnail identity includes raster availability and render-plan version | `tessera-ffi/src/preview.rs`: `RequestKey.render = imported_render_identity(recipe, support)` = blake3(`IMPORTED_RENDER_PLAN_VERSION` = 2, each referenced imported raster key + presence + pinned payload revision). Zero for ordinary originals | `lr13b_thumbnail_request_identity_tracks_mask_rasters_and_plan_version`: missing raster fails, storing it re-renders (not the cached failure), bytes delivered; version and raster removal change the identity. RED compile, then RED 0/1 with identity disabled; GREEN 16/16 preview tests |
| Thumbnail same-level render_region comparison | test only | `lr13_imported_proxy_thumbnail_renders_at_thumbnail_level` now asserts exact equality with `Renderer::render_region` at levels 2, 1, 0 |
| Admission assertion in ab924590 | Kept (justified): the old "every external DNG declines" assertion encoded a limitation; the Metal regression checks GPU submissions and CPU agreement | `export/tests/lrcat_jxl.rs::external_dng_resident_tail_admits_only_identity_native_rgb`: identity/none orientation admitted; rotated 3/6/8, lens blur and retouch still decline (CPU route). 4/4 |
| M4 relink uses ordinary RAW GPU/lens route | **Not done.** A relinked original keeps `catalog_orientation`, so `image-core` render dispatch (render.rs ~636/676/718) sends it to the scalar camera-linear route. Routing it to the normal raw path needs M8's frame decision first (proxy edits are normalized after catalog orientation is consumed; ordinary originals use main's decoder orientation), and touches image-core render.rs, which LR-8d is changing | — |
| M5 remaining library-listing work | `catalog::IndexedMetadata` (index provider) records `tessera.listing.recipe` (recipe hash) and `tessera.listing.original`; `catalog::project` uses them while the hash is current (else one recipe read); cull sessions and `list_images` pass the facts from SQL | `session.rs::lr13b_cold_library_open_reads_no_proxy_recipes`: cold open of 4 proxies (one relinked) = 0 recipe reads, correct source/offline/name; engine edit re-indexes, still 0; stale facts = exactly 1 read. Compile RED; GREEN |
| M6 catalog filename display/export, never accessibility identity | `display_name` on `SessionImage`/`ImageSummary` (file name of the catalog original path); export names via `catalog::proxy_display_name`; Swift `EngineLibrary` item name, recovery/caption messages. Identifiers stay index-based | `tests/lr13b_library_followups.rs::proxies_are_keyed_by_image_and_named_from_the_catalog` (list_images, cull session, PNG export names; ordinary rows `None`). Compile RED; GREEN |
| M7 copies keyed by image ID | `Lightroom Proxies/<catalog image id>.dng`, rewritten when bytes differ | Same test, copy run: two photos with byte-identical Smart Previews stay two images with distinct paths. Finding, not changed: in-place Lightroom-owned proxies use the sidecar store's content-keyed protected recipes, so byte-identical in-place proxies share one recipe (sidecar store design; needs its own ruling) |
| M8 common orientation/geometry frame and lens ordering | **Not done** (see M4): needs a ruling on the frame (Lightroom crop/mask coordinates vs catalog orientation) and changes in pipeline-cpu/image-core render ordering next to LR-8d's BaselineExposure work | — |
| M10 normalized proxy edits survive full-resolution relink | **Not done**: a proxy-vs-original parity test is only meaningful after LR-8d settles BaselineExposure on proxies (B4) and after M8 | — |
| Minor: copy toggle accessibility ID | Already closed in LR-8R2 (`library.import.copyProxies`) | — |
| Minor: import override test-only | `TESSERA_LRCAT_SMART_PREVIEWS` read only under `cfg(test)` | `tests/lr13b_env_override.rs` (integration tests link production code): RED 0/1 ((0,1) vs (1,0)), GREEN 1/1 |
| Minor: library.json only in app support | `default_options` drops the catalog-folder fallback (app support `Imported Libraries` instead); `apply` refuses the catalog's folder | `lr13b_library_followups.rs::library_json_is_never_written_next_to_the_catalog`; `tests/lrcat_streaming_parity.rs` used the catalog's folder as library and now uses a sibling folder (setup only, assertions unchanged) |
| LR-8h compressed tiles streamed one at a time | `raw-decode/src/lossy_dng.rs`: read → decode into output → drop, per tile (each tile read once); IFD validation stays up front; output allocated after the first tile decodes; active-area crop in place | `tests/lr8f_safety.rs::lr8h_tile_stream_peak_is_output_plus_one_tile` (live-peak meter added beside the cumulative one): RED peak 100668462 vs bound 63716686; GREEN peak 57448805 (output 50331648). Existing read-once and mutation tests pass |

## Attempts

1. Imported both LR-13c test commits. Full lazy-preview test target could not
   compile before implementation: the async preview API did not yet exist.
2. Ran the original first test commit's `lr13c_open_never_calls_pixel_provider`
   against the unchanged implementation: **0 passed, 1 failed**; actual 32
   pixel calls, expected 0. Restored the complete imported test file unchanged.
3. Cherry-picked LR-13c implementation and handoff without conflicts.
4. `cargo test --locked --release -p cull -p raw-decode -p tessera-ffi
   --no-fail-fast`: **879 passed, 0 failed, 42 ignored**, exit 0.

5. Proxy masks initial RED: **0 passed, 2 failed** (missing mask silently
   dropped; stored zero/one alpha produced identical print pixels).
6. First GREEN attempt reached an invalid depth fixture (depth ranges cannot
   carry Adobe segmentation metadata). Corrected only fixture construction to
   use the existing depth cache; retained all pixel assertions and bounds.
7. Corrected depth fixture RED: **1 passed, 1 failed**, unsupported AI mask kind.
8. Proxy mask GREEN: **2 passed, 0 failed**, including Native/Adobe and
   print/file output. Full export suite: **114 passed, 0 failed, 7 ignored**, exit 0.

9. Proxy effects RED: **0 passed, 3 failed** (available effects omitted and
   invalid depth accepted). First build after the fix had one missing context
   argument; corrected the call. GREEN: **3 passed, 0 failed**.
10. Thumbnail effect RED: **0 passed, 1 failed**; the thumbnail lacked the
    registered retouch/depth resources. GREEN: **1 passed, 0 failed**.
    No profile parsing, DCP, BaselineExposure, or golden changes.

11. Raster errors: core RED **0 passed, 1 failed** and thumbnail RED
    **0 passed, 1 failed**. GREEN: core **1 passed**, thumbnail/effects **2 passed**.
    Core checks both processes at levels 0, 1 and 2 and zero emitted tiles on error.
15. STEP 3: env override RED 0/1, GREEN 1/1. M5/M6/M7/library tests compile
    RED, then GREEN after: (a) the first M6 run showed both in-place proxies
    named lost-01.jpg (content-keyed protected recipe shared by identical
    proxies; restructured, see table); (b) an export-warning note file is
    filtered by extension; (c) M5's engine edit re-indexes the file, so the
    expectation is 0 reads, with a separate stale-facts case = 1 read;
    (d) EmbeddedMetadata's exact-value tests stayed unchanged by moving
    facts to the IndexedMetadata wrapper.
16. LR-8h: RED peak 100668462 vs bound 63716686; GREEN 57448805.
17. Full tessera-ffi run found the Codex STEP 2 Lens Blur regression in
    tests/lrcat.rs (see table): 696 passed / 4 failed, then 699 / 1,
    then all pass.

12. Profile deferral: the Codex RED fixture corrupted CalibrationIlluminant1
    out of file range, which the pixel reader also rejects (test failed on its
    precondition `lossy_dng::read(..).unwrap()`, 1 passed / 1 failed with the
    fix applied). Fixture-only fix (`4e3755a6`): IFD0 NewSubFileType retyped LONG→SHORT. Assertions unchanged.
    RED without fix 0/2; GREEN 2/2.
13. Notice: first GREEN 1/3 (badge label read back nil: SwiftUI reports a
    static-text element's label as AXValue). Badge made a plain element. 3/3.
14. Thumbnail identity: compile RED; behavioural RED with identity forced to
    zero 0/1 (cached failure returned after the raster was stored); GREEN.

## Final gates (2026-10-07, Claude Opus 5.5)

Env: `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-13b`, `CARGO_BUILD_JOBS=5`,
`RAYON_NUM_THREADS=5`. Before the gates: `cargo clean --release -p cull -p export -p image-core
-p index -p pipeline-adobe -p pipeline-cpu -p raw-decode -p tessera-ffi` (1015 files, 5.1 GiB).

| Gate | Result |
| --- | --- |
| `cargo test --release --workspace --no-fail-fast` | Exit 101: **3382 passed, 3 failed, 107 ignored** (load average 29 to 42). The failures are `ml-embed/tests/grouping.rs` `missing_vectors_use_real_jpeg_hashes_conservatively` and `real_cull_session_uses_snapshot_to_gate_jpeg_near_duplicates`, and `ml-quality/tests/culling.rs` `computed_signals_drive_default_best_and_defect_review`. A serialized rerun (`--test-threads=1`, load 40 to 27) fails the same 3 deterministically. They pass at the base `2ee8bfc6` and fail the same way on `local/rut-build` `8fabb780`, so the cause is LR-13c's deferred hashing (STEP 1 port). LR-13e has fixed them on local/rut-build ("finish deferred hashing before ml grouping assertions"); they come over with the LR-13d/e port. Not changed here |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | Attempt 1: one `unused_doc_comments` error (mine), fixed in `09b0ff6e`. Attempt 2: exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0. Generated bindings are committed in `03887afa` (new `display_name` fields); the final regeneration left no drift |
| `tools/orchestrate/swift-gate.sh` | Attempt 1: 937 executed, 3 skipped, 2 failures. `ThemeLintTests` (mine) was fixed in the final commit; the other was `IncrementalLibraryTests.testAppModelKeepsSelectionFilterAndGridAcrossUpdates`. Attempt 2: 937 executed, 3 skipped, **1 failure**, which is `IncrementalLibraryTests...AcrossUpdates` (`counter.updates` 2 vs 1). This is the LR-13c fixture issue that LR-13d `749b8712` fixed (dissimilarPhotos fixture). **Not SWIFT GATE OK** |
| strict release build | exit 0 (191.85 s). Only the known BLAKE3 neon linker warning |
