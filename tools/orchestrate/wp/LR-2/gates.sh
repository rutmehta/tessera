#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-2-tone-curves"
export CARGO_BUILD_JOBS=3 RAYON_NUM_THREADS=3
crates=(-p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p pipeline-adobe -p image-core -p sidecar)
# External RAW inputs deliberately excluded; ignored benchmarks remain ignored.
cargo test --locked "${crates[@]}" --features import-lrcat/fixture --no-fail-fast -- \
  --skip raw_fixture_goldens \
  --skip real_opcode_fixtures_when_available \
  --skip fixture_as_shot_roundtrip_and_slider_directions \
  --skip fixture_level3_tolerance_per_operator_and_output \
  --skip preview_approximation_is_bounded_on_real_fixtures \
  --skip fixture_level3_matches_pipeline_cpu \
  --skip fixture_level3_m2_extremes_are_finite \
  --skip cfa_dng_stays_raw_and_corrupt_linear_dng_does_not_fall_back
cargo clippy --locked "${crates[@]}" --all-targets --features import-lrcat/fixture -- -D warnings
cargo fmt --all --check
bash tools/orchestrate/wp/LR-2/e2e.sh
bash tools/orchestrate/wp/LR-2/compat.sh
