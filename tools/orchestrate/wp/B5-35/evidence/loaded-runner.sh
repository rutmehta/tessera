#!/bin/bash
set -euo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-35"
CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-35-load" cargo build --locked --release -p compositor > /tmp/B5-35-load-build.log 2>&1 &
load_pid=$!
echo "load build pid=$load_pid started $(date -u +%FT%TZ)"
for run in $(seq 1 10); do
  kill -0 "$load_pid"
  echo "RUN $run START $(date -u +%FT%TZ) load_alive=yes"
  cargo test --locked --release -p tessera-ffi --test document_viewport composite_thumbnails_reuse_mips_across_edits -- --exact --nocapture > "/tmp/B5-35-run-$run.log" 2>&1
  kill -0 "$load_pid"
  grep -E 'composite thumbnail:|mip counters:|drag ticks:|test result:' "/tmp/B5-35-run-$run.log"
  echo "RUN $run END $(date -u +%FT%TZ) load_alive=yes"
done
wait "$load_pid"
echo 'LOAD BUILD OK'
