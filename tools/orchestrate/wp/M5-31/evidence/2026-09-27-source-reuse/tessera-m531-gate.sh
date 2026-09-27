#!/bin/bash
set -euo pipefail
cd /Users/rutmehta/Developer/tessera/.worktrees/M5-31
export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-31
export CARGO_BUILD_JOBS=2
cargo test --locked -p compositor --release --lib resident::styles_runtime::tests -- --nocapture > /tmp/tessera-m531-final-focused.log 2>&1
cargo test --locked -p compositor --release --lib only_dissolve_retains_layer_seed_in_program_identity -- --nocapture > /tmp/tessera-m531-final-seed.log 2>&1
cargo test --locked -p compositor --release --test resident_styles_live > /tmp/tessera-m531-final-live.log 2>&1
cargo test --locked -p compositor --release --test resident_styles_large --no-run > /tmp/tessera-m531-final-timing-build.log 2>&1
failed=0
for fixture in twenty_mp_five_styles_1368x912_l1_timing twenty_mp_five_styles_1368x912_l1_unique_ids_timing; do
  for run in 1 2 3; do
    logfile="/tmp/tessera-m531-final-${fixture}-${run}.log"
    uptime > "${logfile}.host"
    ps -axo pid,etime,pcpu,comm >> "${logfile}.host"
    if cargo test --locked -p compositor --release --test resident_styles_large "$fixture" -- --exact --ignored --nocapture --test-threads=1 > "$logfile" 2>&1; then
      printf '%s run%s exit0\n' "$fixture" "$run"
    else
      code=$?
      printf '%s run%s exit%s\n' "$fixture" "$run" "$code"
      failed=1
    fi
  done
done
cargo test --locked -p compositor --release > /tmp/tessera-m531-final-suite.log 2>&1
cargo clippy --locked -p compositor --all-targets -- -D warnings > /tmp/tessera-m531-final-clippy.log 2>&1
cargo fmt --all -- --check > /tmp/tessera-m531-final-fmt.log 2>&1
printf 'FINAL_GATES_FINISHED timing_failures=%s\n' "$failed"
exit "$failed"
