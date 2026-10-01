#!/usr/bin/env bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-2-tone-curves"
export CARGO_BUILD_JOBS=3 RAYON_NUM_THREADS=3
mkdir -p "$CARGO_TARGET_DIR"
# Use the exact feature-unified artifacts from this build, not newest-file guesses.
cargo build --locked -p import-lrcat --features fixture -p pipeline-cpu --message-format=json > "$CARGO_TARGET_DIR/lr2-compat-artifacts.jsonl"
python3 tools/orchestrate/wp/LR-2/link.py compat "$CARGO_TARGET_DIR/lr2-compat-artifacts.jsonl"
