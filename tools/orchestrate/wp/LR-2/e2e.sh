#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-2-tone-curves"
export CARGO_BUILD_JOBS=3 RAYON_NUM_THREADS=3
# Link existing workspace dependencies without changing Cargo.toml or Cargo.lock.
cargo build --locked -p import-lrcat --features fixture -p pipeline-cpu
args=()
for name in engine_api import_lrcat pipeline_cpu rusqlite tempfile; do
    lib=$(ls -t "$CARGO_TARGET_DIR/debug/deps/lib${name}-"*.rlib | head -1)
    args+=(--extern "$name=$lib")
done
rustc --edition=2024 tools/orchestrate/wp/LR-2/e2e.rs -L "dependency=$CARGO_TARGET_DIR/debug/deps" "${args[@]}" -o "$CARGO_TARGET_DIR/lr2-e2e"
"$CARGO_TARGET_DIR/lr2-e2e"
