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
packages=(-p import-lrcat -p engine-api -p image-core -p mask-store -p sidecar -p pipeline-cpu -p ml-depth)
# Explicit exclusions: real RAW fixture readers and optional cached/trained model
# inference. Synthetic RAW metadata, injected depth, and model-cache bookkeeping
# remain enabled. No --ignored here, so no model test can be opted in by accident.
cargo test --locked "${packages[@]}" --no-fail-fast -- \
  --skip fixture_level3 --skip bench_tone_only_change_at_level_2 \
  --skip estimate_available_raw_fixtures_without_weights \
  --skip cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back \
  --skip five_actual_raws_auto_lens_and_upright_are_finite \
  --skip raw_fixture_goldens --skip real_opcode_fixtures_when_available \
  --skip fixture_as_shot_roundtrip_and_slider_directions \
  --skip cached_model --skip cached_post_adapter_uses_automatic_sigma \
  --skip local_trained_adapter > "$logs/lr6b-test.log" 2>&1
cargo test --locked -p image-core --test lr6_depth_import synthetic_import_to_cpu_render \
  -- --ignored --exact > "$logs/lr6b-e2e.log" 2>&1
cargo clippy --locked "${packages[@]}" --all-targets -- -D warnings > "$logs/lr6b-clippy.log" 2>&1
cargo fmt --all -- --check > "$logs/lr6b-fmt.log" 2>&1
