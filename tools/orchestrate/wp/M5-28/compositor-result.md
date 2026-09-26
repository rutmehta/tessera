# M5-28 compositor implementation report

Status: implementation delivered in this worktree, uncommitted. Hardware verification is BLOCKED: Metal adapter discovery fails in this execution environment. This is not a GPU parity PASS.

## Delivered

- Positive-radius Shadows/Highlights resident execution: GPU backdrop prefix replay, two normalized separable bilateral luminance passes, retained auxiliary bases, then ordinary adjustment blending. No CPU pixel evaluation/readback in this path.
- Real halos across tiles using conservative whole-level backing (also for viewport requests), document-edge replication, level-scaled support, sequential local operators, current isolated/pass-through/clip frames, masks, opacity, blend modes and alpha.
- Cached CPU rendering now accepts spatial adjustments. Root cache keys use whole-document revisions, partial-tile reuse is disabled, and group expansion preserves prefix targets. Neighboring edits invalidate affected results conservatively.
- Explicitly changed native CPU Shadows/Highlights from direct 2D bilateral to horizontal-then-vertical bilateral, matching the GPU formulation. It remains edge-aware, not Gaussian. Spatial sigma=max(radius/2,0.5), range sigma=.15, original alpha weights in each pass, vertical range against horizontal bases, f32 accumulation. Independent reference and hard-edge tests distinguish the new formulation from the old operator.
- HDR Toning CPU and resident GPU: Local Adaptation radius/strength, gamma/exposure, detail, shadows/highlights, vibrance/saturation, and sampled toning curve; frozen Equalize Histogram CDF; Exposure-Gamma; Highlight Compression. Validation and native versioned serialization included. Identity preserves over-range inputs.
- HDR is explicitly native-only for PSD: compositor export errors instead of inventing a tag. PSD crate unchanged.
- Documentation in COMPOSITOR.md §§4.3–4.4 records formulas, cache behavior, native bilateral contract change, intended absolute 1e-4 CPU/GPU tolerance, PSD scope, and memory/performance limits.

## Test-first evidence

- `compositor-cpu-red.log`: cached positive-radius rendering failed with Unsupported before the cache/executor change.
- `compositor-bilateral-red.log`: independent separable reference failed against old direct 2D CPU implementation (0.5771456 versus 0.5776644).
- `compositor-hdr-spatial-red.log`: HDR tiled result failed the full padded reference before local execution integration.
- `compositor-resident-red.log`: resident program compiler rejected positive-radius Shadows/Highlights before implementation.

## Verification

Every Cargo command used `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-28`.

- Targeted CPU/native interchange: `cargo test -p compositor --release --test hdr_toning --test m5_26_shadows --test m5_26_validation --test m5_26_psd`. Final result: 33 passed, 0 failed (`compositor-targeted-final.log`).
- Resident positive-radius compiler test and Naga parsing/full validation of document and bilateral shaders pass (`compositor-resident-green.log`, `compositor-shaders.log`). Shader validation is not GPU execution.
- `cargo clippy -p compositor --all-targets -- -D warnings`: PASS, including final repeat (`compositor-clippy-final.log`).
- `cargo fmt -p compositor --check`: PASS, including final repeat (`compositor-fmt-final.log`).
- Full `cargo test -p compositor --release --no-fail-fast`: 194 passed, 45 failed, 10 ignored (`compositor-tests.log`). All 45 failures report **No suitable graphics adapter found**; no non-adapter failure was present. Some pre-existing tests return early when an adapter is unavailable, so reported passes are not all evidence of GPU execution.
- New required GPU tests (`m528_resident`) fail explicitly when no adapter exists. Fixtures cover L0/L2 seams, straight/premultiplied alpha, isolated/pass-through/clipping groups, group opacity, masks, SoftLight, sequential radii, viewport reuse, neighboring edits and all HDR methods. Latest attempt: `compositor-gpu.log`.

## Remaining verification / tradeoffs

Run `cargo test -p compositor --release --test m528_resident --test m5_26_gpu` on a Metal-capable runner, then the parent package gate. The 1e-4 GPU parity tolerance is implemented as assertions but cannot be certified in this environment. No GPU success is claimed.

Spatial resident execution deliberately uses whole-level buffers and replays prefixes rather than sparse halo windows; auxiliary size and storage limits are checked before materialization. Large spatial documents can return ResourceExhausted. CPU prefix replay is conservative and can be costly for many sequential operators. Native formulas do not claim Adobe numerical equivalence. Existing resident layer-style restriction remains; explicit fresh CPU reference still rejects styled documents. Parent image-core/filters changes were not edited by this compositor task. No commits were created.

## Full-suite failures (all adapter discovery)

- `resident::output::tests::profiled_output_matches_cpu_and_caches_preparation`
- `resident::output::tests::profiles_reject_unresolved_invalid_and_unsupported_contracts`
- `resident::output::tests::encoded_profiles_to_linear_displays_match_lcms_grid`
- `resident::output::tests::validates_target_bounds_source_policy_and_lut`
- `resident::output::tests::linear_profiled_hdr_preserves_highlights_and_caps_headroom`
- `resident::output::tests::lut_transforms_straight_rgb_before_premultiplication_and_flattening`
- `resident::output::tests::lut_clamps_input_but_preserves_extended_output`
- `resident::output::tests::edr_preserves_extended_values_alpha_and_offsets`
- `m5_08_hard_mix_boundary`
- `profiled_viewport_rebases_and_rejects_stale_or_foreign_document`
- `resident_viewport_presents_edr_and_rejects_unrendered_regions`
- `huge_smart_child_renders_only_the_sampling_window`
- `nonzero_viewport_rgba8_matches_full_render_after_pan_and_specialization`
- `output_storage_tracks_viewport_not_canvas`
- `oversized_full_render_does_not_poison_viewport_mips`
- `compact_smart_windows_match_cpu_across_parent_tiles`
- `large_parent_coordinates_keep_fractional_footprints`
- `lanczos_support_reaches_beyond_object_bounds_into_adjacent_tile`
- `identity_writes_straight_planar_at_page_offset`
- `lanczos_level_zero_matches_independent_reference_and_is_deterministic`
- `resident_quality_switch_invalidates_nested_caches_and_higher_levels_stay_bilinear`
- `gpu_child_and_resampling_match_cpu_smart_tiles`
- `spatial_shadows_gpu_l0_l2_cross_tile_alpha_groups`
- `hdr_methods_gpu_l0_l2_alpha_and_seams`
- `shadows_highlights_zero_radius_exact`
- `gradient_spatial_dither_multitile_exact`
- `brightness_contrast_exact`
- `pointwise_edge_cases_exact`
- `shadows_highlights_resident_positive_radius`
- `perceptual_hdr_exact`
- `pointwise_adjustments_exact`
- `automatic_transform_kinds_and_three_stage_nested_stack`
- `prefix_cache_revision_invalidation_and_budget`
- `every_metal_filter_routes_and_matches_cpu`
- `mask_edits_reuse_gpu_stages_and_cpu_only_is_layer_local`
- `adjustments_and_cpu_only_content_aware`
- `median_fallback_keeps_unrelated_uploaded_pages`
- `lossless_opaque_nearest_chain_is_one_displacement_stage`
- `styled_smart_source_falls_back_without_omitting_effects`
- `resident_and_fallback_receive_native_child_context`
- `resident_invert_is_not_silently_omitted`
- `resident_level_transform_is_explicit_and_non_destructive`
- `precise_free_transform_matches_cpu_all_kernels`
- `rejects_invalid_geometry_dimensions_and_buffer_contracts`
- `warp_projective_edges_and_large_coordinates_match_cpu`
