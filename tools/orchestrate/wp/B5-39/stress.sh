#!/usr/bin/env bash
# One Rust build at a time in an isolated target, alongside serial Swift test invocations.
set -uo pipefail
export PATH="$HOME/.cargo/bin:$PATH"
root="$(git rev-parse --show-toplevel)"
evidence="$root/tools/orchestrate/wp/B5-39/evidence"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-39-layout-stress"
filter='LayoutProbeHarnessTests|ShellLayoutTests|DocumentInspectorLayoutTests|MasksPanelLayoutTests|DocumentHistoryHeightControlTests|DocumentHistoryKeyboardTraversalTests|AgentReviewLayoutTests|PeopleLayoutTests|DocumentInspectorActionButtonTests|DocumentDitherCheckboxTests|LayoutContractTests'
stop="$evidence/stress.stop"
rm -f "$stop"
(
  n=0
  while [ ! -e "$stop" ]; do
    n=$((n+1))
    printf '%s build %s start\n' "$(date -u +%FT%TZ)" "$n"
    cargo build --locked --release -p tessera-ffi > "$evidence/cargo-stress-latest.log" 2>&1
    rc=$?
    printf '%s build %s exit %s\n' "$(date -u +%FT%TZ)" "$n" "$rc"
    [ "$rc" -eq 0 ] || exit "$rc"
    [ -e "$stop" ] && break
    cargo clean --release -p tessera-ffi >> "$evidence/cargo-stress-latest.log" 2>&1
  done
) > "$evidence/cargo-stress-timeline.log" 2>&1 &
load_pid=$!
trap 'touch "$stop"; wait "$load_pid"; rm -f "$stop"' EXIT
cd "$root/apps/mac"
failed=0
for run in 1 2 3 4 5; do
  printf '%s focused %s start; cargo PID %s\n' "$(date -u +%FT%TZ)" "$run" "$load_pid"
  kill -0 "$load_pid" || exit 2
  swift test -c release -Xswiftc -enable-testing --skip-build --filter "$filter" > "$evidence/focused-$run.log" 2>&1
  rc=$?
  printf '%s focused %s exit %s\n' "$(date -u +%FT%TZ)" "$run" "$rc"
  tail -5 "$evidence/focused-$run.log"
  [ "$rc" -eq 0 ] || failed=1
  kill -0 "$load_pid" || exit 2
done
exit "$failed"
