#!/usr/bin/env bash
set -euo pipefail
cd "$(git rev-parse --show-toplevel)"
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-6-lens-blur"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
export RUST_TEST_THREADS=3
export LR6_RECIPE
LR6_RECIPE=$(mktemp /tmp/tessera-lr6-recipe.XXXXXX)
trap 'rm -f "$LR6_RECIPE"' EXIT
logs=tools/orchestrate/wp/LR-6/evidence
mkdir -p "$logs"
packages=(-p import-lrcat -p engine-api -p image-core -p mask-store -p sidecar -p pipeline-cpu -p ml-depth -p tessera-ffi)
# Explicit exclusions: real RAW fixture readers and optional cached/trained model
# inference. Synthetic RAW metadata, injected depth, and model-cache bookkeeping
# remain enabled. No --ignored here, so no model test can be opted in by accident.
set +e
cargo test --locked "${packages[@]}" --no-fail-fast -- \
  --skip fixture_level3 --skip bench_tone_only_change_at_level_2 \
  --skip estimate_available_raw_fixtures_without_weights \
  --skip cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back \
  --skip five_actual_raws_auto_lens_and_upright_are_finite \
  --skip raw_fixture_goldens --skip real_opcode_fixtures_when_available \
  --skip fixture_as_shot_roundtrip_and_slider_directions \
  --skip cached_model --skip cached_post_adapter_uses_automatic_sigma \
  --skip local_trained_adapter \
  --skip process_version_is_undoable_and_persisted \
  --skip as_shot_sliders_round_trip_on_every_fixture_and_single_slider_touch \
  --skip session_renders_into_surfaces_and_persists_undoable_edits \
  --skip edited_previews_follow_the_recipe_hash \
  --skip panels_crop_masking_detail_and_history \
  --skip slow_interactive_frames_are_not_starved \
  --skip bench_slider_latency \
  --skip bench_panel_latency \
  --skip bench_detail_preview \
  --skip export_batch_does_not_starve_slider_drag \
  --skip gradient_brush_range_overlay_undo_and_persistence \
  --skip ai_masks_segment_in_a_job_and_render_through_the_mask_cache \
  --skip subject_mask_on_the_canon_fixture_with_cached_models \
  --skip bench_mask_latency \
  --skip edr_ring_renders_headroom_and_sdr_ring_stays_bit_identical \
  --skip bench_edr_slider_latency \
  --skip missing_jpeg_returns_pending_then_callback_and_cached_bytes \
  --skip raw_export_with_the_web_preset_and_a_binned_print_render \
  --skip open_document_from_image_is_the_developed_raw \
  --skip bench_p17_document_frames_during_photo_export > "$logs/lr6c-test.log" 2>&1
test_status=$?
cargo test --locked -p image-core --test lr6_depth_import synthetic_import_to_cpu_render \
  -- --ignored --exact > "$logs/lr6c-e2e.log" 2>&1
e2e_status=$?
cargo clippy --locked "${packages[@]}" --all-targets -- -D warnings > "$logs/lr6c-clippy.log" 2>&1
clippy_status=$?
cargo fmt --all -- --check > "$logs/lr6c-fmt.log" 2>&1

fmt_status=$?
printf "test=%s e2e=%s clippy=%s fmt=%s\n" "$test_status" "$e2e_status" "$clippy_status" "$fmt_status"
exit $((test_status || e2e_status || clippy_status || fmt_status))
