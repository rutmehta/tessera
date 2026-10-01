#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/../../../.."
root="$PWD"
run="$root/tools/orchestrate/wp/B5-43/evidence/background-live"
mkdir -p "$run/photos" "$run/appdir" "$run/out"
cp "$root/fixtures/raw/sample.dng" "$run/photos/sample.dng"
open -g -n --env TESSERA_FILTER_LAYOUT_REPRO=1 \
  --stdout "$run/stdout.log" --stderr "$run/stderr.log" \
  "$root/apps/mac/build/Tessera.app" --args --nonactivating \
  --app-dir "$run/appdir" --folder "$run/photos" --filter-selftest "$run/out" \
  -NSViewLayoutFeedbackLoopDebugging YES -NSConstraintBasedLayoutLogUnsatisfiable YES
for attempt in {1..180}; do
  if grep -q 'done, .* failure(s)' "$run/stderr.log" 2>/dev/null; then
    cat "$run/stderr.log"
    grep -q 'done, 0 failure(s)' "$run/stderr.log"
    exit $?
  fi
  sleep 2
done
cat "$run/stderr.log"
echo 'Background self-test did not complete within 360 seconds' >&2
exit 1
