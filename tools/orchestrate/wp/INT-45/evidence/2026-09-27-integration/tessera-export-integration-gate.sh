#!/bin/bash
set -euo pipefail
cd /Users/rutmehta/.codex/worktrees/export-integration/tessera
expected_tree=9fe7a245e2bc070e65fb3f4c0a4301f233ee473d
[ "$(git write-tree)" = "$expected_tree" ] || { echo 'Candidate tree changed; review before running'; exit 2; }
[ "$(git rev-parse MERGE_HEAD)" = 69bcd3e5d3d30ac334720ec6a02e1688b6f6b5cb ] || exit 2
export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d
export CARGO_BUILD_JOBS=2
run_stage() {
  local stage="$1"
  shift
  "$@" >"/tmp/tessera-export-integration-${stage}.log" 2>&1
}
run_stage tests cargo test --locked -p export -p tessera-ffi -p tessera-cli -p tessera-mcp -p sidecar --release
run_stage clippy cargo clippy --locked -p export -p tessera-ffi -p tessera-cli -p tessera-mcp -p sidecar --all-targets -- -D warnings
run_stage workspace cargo check --locked --workspace
cd apps/mac
run_stage ffi ./build-ffi.sh
run_stage swift swift build
printf 'INTEGRATION_BUILD_GATE_OK tree=%s\n' "$expected_tree"
