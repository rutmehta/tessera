# LR-7d gate evidence

Base: `38684d9b`. RED: `cb05baa9`. Implementation: `2abae0bab9d17900ae8c17b300fe8f697509bf29`.

## Reproduction

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-7-upright"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
cargo clean -p engine-api -p import-lrcat -p sidecar -p merge -p pipeline-cpu -p image-core -p tessera-ffi -p pipeline-gpu
cargo test --locked --release -p engine-api -p import-lrcat -p sidecar -p merge -p pipeline-cpu -p image-core -p pipeline-gpu -p tessera-ffi --no-fail-fast -- --test-threads=3
cargo clippy --locked -p engine-api -p import-lrcat -p sidecar -p merge -p pipeline-cpu -p image-core -p pipeline-gpu -p tessera-ffi --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
(cd apps/mac && ./build-ffi.sh)
tools/orchestrate/swift-gate.sh
(cd apps/mac && swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors)
```

The clean removed 112028 files / 34.9 GiB. The completed release Rust gate exited
0: **1340 passed, 0 failed, 48 ignored**, across 210 target summaries.
The initial debug gate was interrupted, not counted as complete.
Clippy, fmt, FFI rebuild, Swift gate, and strict Swift build all exited 0.

## Selected final-gate assertions

```text
test recipe::schema::v4_feature_predicates::lr7d_blue ... ok
test recipe::schema::v4_feature_predicates::lr7d_homography ... ok
test recipe::schema::v4_feature_predicates::lr7d_mode_tag ... ok
test recipe::schema::v4_feature_predicates::lr7d_red ... ok
test resident_model::legacy_ca_without_profile_resident_matches_cpu ... ok
test diagnostics::tests::no_other_source_writes_the_key_directly ... ok
test synthetic_catalog_output_including_retained_source ... ok
test lr7d_hooks_finish_one_replayable_import_transaction ... ok
test lr7d_pv2012_ignored_ca_is_info_not_warning ... ok
test streaming_import_memory_is_flat_and_time_bounded ... ok
test lr7d_synthetic_feature_imports_write_v4 ... ok
test matrix_guard_rejects_a_retained_key_claimed_as_translated ... ok
test matrix_guard_accepts_a_complete_approximate_row ... ok
test matrix_guard_rejects_a_translated_row_carrying_an_approximate_diagnostic ... ok
test matrix_guard_rejects_an_approximate_row_without_a_synthetic_value ... ok
test matrix_guard_rejects_approximate_diagnostic_naming_another_field ... ok
test matrix_guard_rejects_approximate_with_warnings ... ok
test matrix_guard_rejects_approximate_without_a_diagnostic ... ok
test matrix_guard_rejects_approximate_without_retained_source ... ok
test matrix_guard_rejects_the_fixture_row_against_the_unconverted_parser ... ok
test matrix_guard_rejects_approximate_without_the_recipe_field ... ok
test translation_matrix_matches_synthetic_import ... ok
test unrelated_recipe_bytes_remain_identical ... ok
test recipe_is_editable_and_embedded_in_float_dng ... ok
test lr7d_geometry_rejects_existing_history_without_mutation ... ok
test lr7d_standalone_keeps_xmp_author_and_one_replayable_edit ... ok
test lr7d_default_acr_packet_has_no_catalog_bucket ... ok
test recipe_atomic_roundtrip_and_schema_guard ... ok
test lrcat::lrcat_resume_tests::approximate_groups_count_every_photo_but_cap_examples_at_five ... ok
test lrcat::lrcat_resume_tests::approximate_translations_populate_the_report ... ok
test smart_preview::tests::lr7d_local_save_bumps_feature_envelope ... ok
test export_batch_does_not_starve_slider_drag ... ok
test slow_interactive_frames_are_not_starved ... ok
test brush_latency_on_a_20_megapixel_layer ... ok
test lr7d_ffi_mode_change_clears_saved_matrix ... ok
```

## Timing repeats

Exact freshly built release binaries, not a separate rebuild. The full gate passed
these tests; these repeats expose the numeric output and serial comparison.

### liquify-parallel

```text
Command: /Users/rutmehta/.cache/tessera-target/LR-7-upright/release/deps/document_liquify_ui-c30e90a718ef6d06 --nocapture --test-threads=3
LIQUIFY 20MP (5472x3648, cell 16, proxy 1824x1216 /3): open 124 ms; brush+preview per event median 10.9 ms, p95 15.5 ms, max 19.4 ms (brush 0.3 ms, preview 10.7 ms median); full-res apply 68 ms
test result: ok. 15 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.26s
```

### liquify-serial

```text
Command: /Users/rutmehta/.cache/tessera-target/LR-7-upright/release/deps/document_liquify_ui-c30e90a718ef6d06 brush_latency_on_a_20_megapixel_layer --exact --nocapture --test-threads=1
test brush_latency_on_a_20_megapixel_layer ... LIQUIFY 20MP (5472x3648, cell 16, proxy 1824x1216 /3): open 119 ms; brush+preview per event median 13.2 ms, p95 23.2 ms, max 69.8 ms (brush 0.3 ms, preview 12.6 ms median); full-res apply 84 ms
test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 14 filtered out; finished in 2.40s
```

### frames-parallel

```text
Command: /Users/rutmehta/.cache/tessera-target/LR-7-upright/release/deps/develop-1fc2996500bcc192 starve --nocapture --test-threads=3
39 frames during a 222.8915ms burst; final L1
slider during export: 120 frames in 2.424812708s (118 at L2): render p50 2.5 ms p90 4.2 ms max 145.5 ms; set→frame p50 3.1 ms p90 5.6 ms max 145.8 ms; export 5 images in 6.229512583s, 0 completed during the drag
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 13.99s
```

### frames-serial

```text
Command: /Users/rutmehta/.cache/tessera-target/LR-7-upright/release/deps/develop-1fc2996500bcc192 starve --nocapture --test-threads=1
slider during export: 120 frames in 2.193971084s (120 at L2): render p50 2.2 ms p90 3.6 ms max 11.0 ms; set→frame p50 2.6 ms p90 4.3 ms max 11.4 ms; export 5 images in 5.836070167s, 0 completed during the drag
39 frames during a 209.207625ms burst; final L1
test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out; finished in 19.62s
```

## Swift

```text
Build complete! (59.15s)
	 Executed 912 tests, with 3 tests skipped and 0 failures (0 unexpected) in 282.827 (282.953) seconds
✔ Test run with 5 tests in 2 suites passed after 0.040 seconds.
SWIFT GATE OK
[5/6] Linking Tessera
Build of product 'Tessera' complete! (279.92s)
```

Swift skips: `LibraryTests.testTwentyThousandEngineLibraryMeasurement` requires
`TESSERA_RUN_20K_LIBRARY=1`; the two `SmartPreviewNativeWorkflowTests.testActual*`
acceptance tests require `TESSERA_SMART_PREVIEW_RAW`. Both variables were unset.

## Unchanged Rust ignores

No command-line skip/filter was applied to the full gate. These are pre-existing
opt-in qualifications/benchmarks; no catalog or exclusive runtime was supplied.
For unnamed `#[ignore]` attributes, the reason below describes the test scope.

| Test | Reason not forced in this lane |
| --- | --- |
| `bench_tone_only_change_at_level_2` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `local_trained_adapter_preserves_sensor_mask_for_every_pattern` | requires locally trained weights; run after documented training/export |
| `benchmark_three_24mp_layers` | 3x24 MP allocation/time benchmark; run release explicitly |
| `five_actual_raws_auto_lens_and_upright_are_finite` | full-resolution five-camera acceptance: cargo test --release -p pipeline-cpu --test lens_fixtures -- --ignored --nocapture |
| `real_legacy_v1_asset_reopen_and_edit_parity` | requires TESSERA_CODEC_LEGACY_ASSET, TESSERA_CODEC_LEGACY_REPORT and TESSERA_CODEC_OUTPUT_DIR |
| `real_raw_fixture_measurement` | requires TESSERA_CODEC_RAW_FIXTURE and TESSERA_CODEC_OUTPUT_DIR |
| `resident::tests::single_pass_preserves_dispatch_and_clear_order` | requires Metal; checks >4096 dispatches and ordered clear in one pass |
| `tone_local::tests::shared_means_benchmark` | manual wall-clock benchmark; includes allocation, upload and readback |
| `benchmark_real_cfa_first_frame_then_tone_only` | requires Metal, RAW fixtures, pinned CFA weights and measured per-fixture calibration |
| `bench_full_level2_m2_chain_gpu_vs_cpu` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_full_level2_tone_only_gpu_vs_cpu` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_interactive_per_operator` | requires real RAW fixtures and Metal; prints interactive frame times |
| `bench_l2_composed_geometry_budget` | requires a real >=30 MP NEF, Metal and release build; asserts geometry drag budget |
| `bench_level_barrier` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_nef_per_operator_l2_l0` | requires a real NEF, Metal, and potentially several minutes at full L0 |
| `tone_signed_rgb_near_zero_luminance_matches_f64_reference` | engine follow-up: signed-luminance conditioning; main exceeds f64-reference bound |
| `bench_nef_level2_resident` | requires Metal and the Nikon NEF fixture; prints measurements, not a target guarantee |
| `full_nef_transaction_keeps_device_alive` | requires the full Nikon NEF fixture and Metal |
| `locals_24mp` | 24MP GPU/CPU regression; run explicitly |
| `develop::preview_qualification::decision_reuse_qualification::engine_proxy_cache_frame_qualification` | exclusive lane; explicit fixture/variant and preregistered runner |
| `develop::preview_qualification::engine_iosurface_matched_viewport_qualification` | exclusive Metal lane, real read-only RAW, explicit qualification route/output |
| `develop::preview_qualification::engine_same_engine_unchanged_proxy_reopen_baseline` | exclusive runtime lane, explicit read-only fixture and output; no cache implementation |
| `bench_detail_preview` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_panel_latency` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_slider_latency` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_interactive_recomposite_100_layers_20mp` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `memory_of_a_pixel_apply_at_100_megapixels_f32` | Opt-in 100 MP memory benchmark. |
| `memory_of_a_pixel_apply_at_100_megapixels_u8` | Opt-in 100 MP memory benchmark. |
| `memory_of_a_smart_object_begin_and_apply_at_100_megapixels_u8` | Opt-in 100 MP memory benchmark. |
| `timing_on_a_12_megapixel_layer` | Opt-in large-layer timing benchmark. |
| `timing_on_a_24_megapixel_layer` | Opt-in large-layer timing benchmark. |
| `bench_camera_raw_24mp` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_gaussian_preview_20mp` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `b515_bench_p16_background_export` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_p13_4k_viewport_l0_100_layers_20mp` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_p14_mutations_during_slow_style_frames` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_p16_export_flat_sync` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_p16_export_parity_files` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_p17_document_frames_during_photo_export` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_p19_smart_filter_drag_20mp` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_styled_large_viewport_frame` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `typing_preview_latency_20mp` | Opt-in 20 MP typing latency benchmark. |
| `bench_interactive_dabs_20mp` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_warp_drag_20mp` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `bench_edr_slider_latency` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `catalog_copy_indexes_eight_accessible_references` | requires TESSERA_LRCAT_COPY and eight accessible originals |
| `bench_mask_latency` | Opt-in manual performance benchmark; unchanged #[ignore]. |
| `public_engine_offline_restart_sync_original_export_and_conflict` | requires TESSERA_SMART_PREVIEW_RAW and the exclusive native test lane |
