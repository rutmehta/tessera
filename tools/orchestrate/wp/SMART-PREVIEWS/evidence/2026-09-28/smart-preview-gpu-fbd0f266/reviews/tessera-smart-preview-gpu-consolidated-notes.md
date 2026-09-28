# Consolidated Smart Preview GPU candidate

**Source only; unbuilt and unrun.** Base `8c5fd747` (Build new native Smart Previews with explicit Compact tier). Current main at preparation is `0cc525bd`; only a non-overlapping smart_preview.rs build-tier call differs in the relevant FFI subtree. This candidate does not change native build_smart_preview generation/tier choice.

## Application

Use ONE patch: `/tmp/tessera-smart-preview-gpu-consolidated.patch`. `git apply --check` passed against an exact temporary8c5fd747 source base and current main0cc525bd. Do not also apply the individual patches. It contains, in order, original GPU, extra tests, coarse GPU, FFI opt-in, FFI coarse proposals. The five individual proposals remain untouched for history.

Exact temporary sources: `/tmp/tessera-smart-preview-gpu-consolidated/{base,new}`. Files listed in `paths.txt`; immutable base commit in `base-head`; candidate SHA256 per file in `source-sha256.json`; every #[test] function in changed files in `test-inventory.json`. No tracked/shared sources edited and no compiler lane used. Root's Swift integration gate precedes GPU runtime qualification.

Two rebase conflicts were resolved structurally: resident_tail_plan is inserted after the existing Compact-aware constructor including its `tier` field; the new image-core tail-plan test is appended after the existing Compact scale-three test. The original helper body was not altered. Other patch stages applied cleanly. A new explicit Compact2048 persisted scale-three GPU test was added because the earlier GPU fixtures used legacy Detail2560; existing fixtures now explicitly retain Detail2560 semantics via a fixture helper.

## Calibration audit

The CPU Smart Preview route remains the intentional default unless TESSERA_SMART_PREVIEW_GPU=1. Under that switch, Engine::develop_renderer passes actual recipe settings to a separate per-session proxy backend selection. It shares the Metal device but cannot initialize or poison the original renderer OnceLock. TESSERA_RENDER_BACKEND=cpu still overrides the opt-in. Automatic proxy calibration measures actual IOSurface/histogram output at the same <=2048 default screen-level rule used by Develop; Compact assets usually selectL0 while larger Detail assets selectL1. It does not probe proxy L2 with default settings as the original path did. Original calibration remains default-settingsL2, unchanged behavior. Later actual viewport/adaptive levels use the resident coarse path, with explicit scalar fallback for unsupported recipes/backends. Runtime level/viewport timing qualification is still required; the pre-attach default is not claimed to represent every display.

Coarse GPU uses the shared extracted resident tail, fullL0 development and geometry, resident linear box reduction, then Output. Original fused stage ordering is preserved. See `/tmp/tessera-smart-preview-gpu-coarse-notes.md` for contracts and remaining risks.

## Test inventory (all candidate tests unrun)

10 pipeline-gpu smart_preview integration tests (including3 macOS surface tests):

1. camera_linear_matches_scalar_with_captured_ca_vignette_geometry_wb_and_cache_edits
2. f32_negative_hdr_prefix_remains_finite_on_cold_warm_and_rejected_cache_paths
3. unsupported_tail_locals_keep_scalar_fallback_and_export_guards
4. reduced_nonsquare_guided_geometry_uses_proxy_dimensions_and_matches_scalar
5. display_linear_tiles_keep_scene_signed_hdr_until_the_display_transform
6. coarse_proxy_post_geometry_linear_reduction_matches_scalar_across_edits
7. surface_checks::camera_linear_surface_matches_scalar
8. surface_checks::coarse_proxy_sdr_and_edr_surfaces_match_scalar_without_pixel_readback
9. surface_checks::camera_linear_half_surface_matches_scalar_edr_with_no_pixel_readback
10. compact_scale_three_codec_reopen_uses_resident_pyramid_and_captured_geometry

8 image-core smart_preview integration tests (7 retained,1 added tail-plan):

1. source_preserves_physical_metadata_and_separates_owner_and_render_identity
2. ordinary_and_m2_wb_region_tiles_and_progressive_are_scalar_camera_linear
3. unsupported_prefix_process_and_external_masks_fail_before_delivery
4. resident_metrics_surfaces_and_export_rows_decline_without_touching_buffers
5. cancellation_from_sink_prevents_later_tile_delivery
6. persisted_proxy_reopens_into_the_same_camera_linear_render_route
7. compact_scale_three_persisted_route_keeps_original_metadata_and_edit_geometry
8. camera_linear_tail_plan_keeps_prefix_immutable_and_geometry_editable

3 existing backend unit tests retained:

- adobe_gpu_backend_uses_host_barriers_at_preview_level
- calibration_measures_surface_presentation_without_pixel_readback
- selection_requires_measured_gpu_advantage

Changed-file inventory additionally lists48 existing Develop and2 existing lib unit tests; these are not new GPU qualification tests. Full FFI regression remains required. Renderer-level transitions simulate adaptive requests but do not replace actual publicEngine listener/adaptive/viewport benchmarking. Existing full-quality export/offline persistence workflow must also run with opt-in to establish unchanged ownership/export behavior.

GPU execution gate hardened in consolidation: all10 GPU tests create contexts through metal_context(), which fails on device unavailability, asserts the backend is Metal, and prints actual adapter info. No silent skip/return path exists. Added missing GPU submission/dispatch assertions to the signedF32 test and the L0 surface check. Positive surface tests also assert zero pixel-readback delta and histogram population; tile numerical tests intentionally read pixels for comparison. These test-harness changes do not constitute execution evidence. Root must run the macOS surface tests and preserve their adapter/submission evidence during the assigned GPU lane.

## Suggested later runtime commands (not executed here)

Every command must use the root-assigned sole native lane and `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated CARGO_BUILD_JOBS=2`.

- `cargo test -p pipeline-gpu --test smart_preview --release -- --nocapture --test-threads=1` (all10 GPU tests, no ignored tests/device skips)
- `cargo test -p image-core --test smart_preview --release -- --nocapture --test-threads=1` (8 representation/ownership/Compact tests)
- `cargo test -p tessera-ffi backend::tests --lib --release -- --nocapture --test-threads=1` (3 calibration/regression tests)
- Public `smart_preview_workflow` ignored integration explicitly enabled with TESSERA_SMART_PREVIEW_RAW pointing at the approved read-only fixture and TESSERA_SMART_PREVIEW_GPU=1; test uses a disposable copy. Run automatic backend selection and explicitGPU override, preserving logs that show actualrepresentation/level/backend. Verify expected test name/count and fixture hash rather than accepting an empty/skipped invocation.
- Full FFI units, relevant Original/Develop/export regressions, strict Clippy and formatting after fixes. Then actual matched-visible viewport/listener timing at L0/L1/L2 with current recipes, cold+warm, tone+WB+presence, SDR+EDR. Compare OriginalMetal and proxyGPU, retaining all actual frame levels and pixel dimensions. This last measurement is a separate acceptance task, not supplied by numerical tests.
