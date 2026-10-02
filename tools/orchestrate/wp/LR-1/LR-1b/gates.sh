#!/bin/sh
# Run from the worktree root. External RAW fixtures are explicitly excluded.
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-1-point-color"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
cargo test --locked -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p image-core -p sidecar -p tessera-ffi --no-fail-fast -- --test-threads=3 \
  --skip raw_fixture_goldens \
  --skip real_opcode_fixtures_when_available \
  --skip fixture_as_shot_roundtrip_and_slider_directions \
  --skip fixture_level3_tolerance_per_operator_and_output \
  --skip preview_approximation_is_bounded_on_real_fixtures \
  --skip fixture_level3_matches_pipeline_cpu \
  --skip fixture_level3_m2_extremes_are_finite \
  --skip cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back \
  --skip estimate_available_raw_fixtures_without_weights \
  --skip missing_jpeg_returns_pending_then_callback_and_cached_bytes \
  --skip open_document_from_image_is_the_developed_raw \
  --skip process_version_is_undoable_and_persisted \
  --skip as_shot_sliders_round_trip_on_every_fixture_and_single_slider_touch \
  --skip session_renders_into_surfaces_and_persists_undoable_edits \
  --skip edited_previews_follow_the_recipe_hash \
  --skip panels_crop_masking_detail_and_history \
  --skip slow_interactive_frames_are_not_starved \
  --skip export_batch_does_not_starve_slider_drag \
  --skip raw_export_with_the_web_preset_and_a_binned_print_render \
  --skip edr_ring_renders_headroom_and_sdr_ring_stays_bit_identical \
  --skip gradient_brush_range_overlay_undo_and_persistence \
  --skip ai_masks_segment_in_a_job_and_render_through_the_mask_cache \
  --skip subject_mask_on_the_canon_fixture_with_cached_models
