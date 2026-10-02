#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-2-tone-curves"
export CARGO_BUILD_JOBS=3 RAYON_NUM_THREADS=3
crates=(-p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p pipeline-adobe -p image-core -p sidecar -p merge -p tessera-ffi)
# LR-2e final gate after LR-7 rebase. Synthetic inputs only.
# External RAW tests below and ignored benchmarks are excluded.
skip=(
  raw_fixture_goldens real_opcode_fixtures_when_available
  fixture_as_shot_roundtrip_and_slider_directions
  fixture_level3_tolerance_per_operator_and_output
  preview_approximation_is_bounded_on_real_fixtures
  fixture_level3_matches_pipeline_cpu fixture_level3_m2_extremes_are_finite
  cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back
  estimate_available_raw_fixtures_without_weights
  missing_jpeg_returns_pending_then_callback_and_cached_bytes
  process_version_is_undoable_and_persisted
  as_shot_sliders_round_trip_on_every_fixture_and_single_slider_touch
  session_renders_into_surfaces_and_persists_undoable_edits
  edited_previews_follow_the_recipe_hash panels_crop_masking_detail_and_history
  slow_interactive_frames_are_not_starved export_batch_does_not_starve_slider_drag
  edr_ring_renders_headroom_and_sdr_ring_stays_bit_identical
  gradient_brush_range_overlay_undo_and_persistence
  ai_masks_segment_in_a_job_and_render_through_the_mask_cache
  subject_mask_on_the_canon_fixture_with_cached_models
  open_document_from_image_is_the_developed_raw
  raw_export_with_the_web_preset_and_a_binned_print_render
)
args=()
for name in "${skip[@]}"; do args+=(--skip "$name"); done
log_dir="${LR2E_LOG_DIR:-/tmp/tessera-lr2e-gates}"
mkdir -p "$log_dir"
cargo clean "${crates[@]}" > "$log_dir/clean.log" 2>&1
: > "$log_dir/status.txt"
gate_status=0
run_gate() {
  local name="$1"
  shift
  local status=0
  "$@" > "$log_dir/$name.log" 2>&1 || status=$?
  echo "$name=$status" | tee -a "$log_dir/status.txt"
  if (( status != 0 )); then gate_status=$status; fi
}
run_gate test cargo test --locked "${crates[@]}" --features import-lrcat/fixture --no-fail-fast -- "${args[@]}"
# Report the load-sensitive timing separately; never erase a broad-run failure.
run_gate liquify-serial cargo test --locked -p tessera-ffi --features import-lrcat/fixture \
  --test document_liquify_ui brush_latency_on_a_20_megapixel_layer \
  -- --exact --nocapture --test-threads=1
run_gate clippy cargo clippy --locked "${crates[@]}" --all-targets --features import-lrcat/fixture -- -D warnings
run_gate fmt cargo fmt --all --check
exit "$gate_status"
