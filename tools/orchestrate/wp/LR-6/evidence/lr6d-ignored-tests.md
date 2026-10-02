# LR-6d upstream ignored-test inventory

The gate passes no `--skip` filters. This inventory preserves upstream opt-in
boundaries; it does not introduce new ignores. Benchmarks are distinct from the
ordinary Liquify p95 and frame-delivery assertions, which run in the full suite
and are rerun serially. Real-catalog qualification is never opted into.

| Test | Source | Reason |
| --- | --- | --- |
| `bench_tone_only_change_at_level_2` | `crates/image-core/tests/fixture.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `synthetic_import_to_cpu_render` | `crates/image-core/tests/lr6_depth_import.rs` | Ignored only in the broad run; explicitly executed by gate.sh with the synthetic imported recipe. |
| `local_trained_adapter_preserves_sensor_mask_for_every_pattern` | `crates/image-core/tests/ml_cfa_local.rs` | requires locally trained weights; run after documented training/export |
| `streaming_import_memory_is_flat_and_time_bounded` | `crates/import-lrcat/tests/scale.rs` | timing and allocation bounds are for release builds |
| `benchmark_three_24mp_layers` | `crates/merge/tests/layers.rs` | 3x24 MP allocation/time benchmark; run release explicitly |
| `cached_model_fixture_and_partition` | `crates/ml-depth/tests/model.rs` | opt-in cached model inference; never part of the default LR gate |
| `five_actual_raws_auto_lens_and_upright_are_finite` | `crates/pipeline-cpu/tests/lens_fixtures.rs` | full-resolution five-camera acceptance: cargo test --release -p pipeline-cpu --test lens_fixtures -- --ignored --nocapture |
| `real_legacy_v1_asset_reopen_and_edit_parity` | `crates/pipeline-cpu/tests/smart_preview_codec.rs` | requires TESSERA_CODEC_LEGACY_ASSET, TESSERA_CODEC_LEGACY_REPORT and TESSERA_CODEC_OUTPUT_DIR |
| `real_raw_fixture_measurement` | `crates/pipeline-cpu/tests/smart_preview_codec.rs` | requires TESSERA_CODEC_RAW_FIXTURE and TESSERA_CODEC_OUTPUT_DIR |
| `engine_proxy_cache_frame_qualification` | `crates/tessera-ffi/src/develop/decision_reuse_qualification.rs` | exclusive lane; explicit fixture/variant and preregistered runner |
| `engine_iosurface_matched_viewport_qualification` | `crates/tessera-ffi/src/develop/preview_qualification.rs` | exclusive Metal lane, real read-only RAW, explicit qualification route/output |
| `engine_same_engine_unchanged_proxy_reopen_baseline` | `crates/tessera-ffi/src/develop/preview_qualification.rs` | exclusive runtime lane, explicit read-only fixture and output; no cache implementation |
| `bench_detail_preview` | `crates/tessera-ffi/tests/develop.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_panel_latency` | `crates/tessera-ffi/tests/develop.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_slider_latency` | `crates/tessera-ffi/tests/develop.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_interactive_recomposite_100_layers_20mp` | `crates/tessera-ffi/tests/document.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `memory_of_a_pixel_apply_at_100_megapixels_f32` | `crates/tessera-ffi/tests/document_adaptive_memory.rs` | Upstream opt-in 100-megapixel allocation/memory benchmark; excluded from ordinary tests. |
| `memory_of_a_pixel_apply_at_100_megapixels_u8` | `crates/tessera-ffi/tests/document_adaptive_memory.rs` | Upstream opt-in 100-megapixel allocation/memory benchmark; excluded from ordinary tests. |
| `memory_of_a_smart_object_begin_and_apply_at_100_megapixels_u8` | `crates/tessera-ffi/tests/document_adaptive_memory.rs` | Upstream opt-in 100-megapixel allocation/memory benchmark; excluded from ordinary tests. |
| `timing_on_a_12_megapixel_layer` | `crates/tessera-ffi/tests/document_adaptive_ui.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `timing_on_a_24_megapixel_layer` | `crates/tessera-ffi/tests/document_adaptive_ui.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_camera_raw_24mp` | `crates/tessera-ffi/tests/document_camera_raw_preview.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_gaussian_preview_20mp` | `crates/tessera-ffi/tests/document_filters.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `b515_bench_p16_background_export` | `crates/tessera-ffi/tests/document_perf.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_p13_4k_viewport_l0_100_layers_20mp` | `crates/tessera-ffi/tests/document_perf.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_p14_mutations_during_slow_style_frames` | `crates/tessera-ffi/tests/document_perf.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_p16_export_flat_sync` | `crates/tessera-ffi/tests/document_perf.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_p16_export_parity_files` | `crates/tessera-ffi/tests/document_perf.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_p17_document_frames_during_photo_export` | `crates/tessera-ffi/tests/document_perf.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_p19_smart_filter_drag_20mp` | `crates/tessera-ffi/tests/document_perf.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_styled_large_viewport_frame` | `crates/tessera-ffi/tests/document_styles_ui.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `typing_preview_latency_20mp` | `crates/tessera-ffi/tests/document_text_ui.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_interactive_dabs_20mp` | `crates/tessera-ffi/tests/document_tools.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_warp_drag_20mp` | `crates/tessera-ffi/tests/document_transform_ui.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `bench_edr_slider_latency` | `crates/tessera-ffi/tests/hdr.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `catalog_copy_indexes_eight_accessible_references` | `crates/tessera-ffi/tests/lrcat.rs` | requires TESSERA_LRCAT_COPY and eight accessible originals |
| `ffi_streaming_memory_is_bounded` | `crates/tessera-ffi/tests/lrcat_streaming.rs` | release memory and time gate |
| `bench_mask_latency` | `crates/tessera-ffi/tests/masks.rs` | Upstream opt-in performance benchmark; requires its explicit release/measurement invocation. |
| `public_engine_offline_restart_sync_original_export_and_conflict` | `crates/tessera-ffi/tests/smart_preview_workflow.rs` | requires TESSERA_SMART_PREVIEW_RAW and the exclusive native test lane |
