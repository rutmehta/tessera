#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-2-tone-curves"
export CARGO_BUILD_JOBS=3 RAYON_NUM_THREADS=3
crates=(-p import-lrcat -p engine-api -p sidecar -p merge -p pipeline-cpu -p pipeline-gpu -p image-core -p tessera-ffi)
log_dir="${LR2F_LOG_DIR:-/tmp/tessera-lr2f-gates}"
mkdir -p "$log_dir"
cargo clean -p import-lrcat -p engine-api -p sidecar -p merge > "$log_dir/clean.log" 2>&1
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
run_gate test cargo test --release --locked "${crates[@]}" --no-fail-fast -- --nocapture
run_gate liquify-serial cargo test --release --locked -p tessera-ffi \
  --test document_liquify_ui brush_latency_on_a_20_megapixel_layer \
  -- --exact --nocapture --test-threads=1
run_gate clippy cargo clippy --locked "${crates[@]}" --all-targets -- -D warnings
run_gate fmt cargo fmt --all --check
exit "$gate_status"
