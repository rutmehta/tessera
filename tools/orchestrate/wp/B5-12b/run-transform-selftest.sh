#!/bin/bash
# Launches this worktree's Tessera in the background (open -g -n and --nonactivating: never activated), runs --transform-selftest,
# captures only its own window (screencapture -x -o -l <windowID>) when the test asks, then quits only its PID.
set -u
WT=/Users/rutmehta/Developer/lightroom/.worktrees/B5-12b
SP=${SP:-${TMPDIR:-/tmp}/transform-run-b512b}
EV=$WT/tools/orchestrate/wp/B5-12b/evidence
rm -rf "$SP"; mkdir -p "$SP/app" "$SP/folder" "$SP/test" "$EV"
LOG=$SP/selftest.log
: > "$LOG"
APP=$WT/apps/mac/build/Tessera.app
open -g -n --stderr "$LOG" ${SKIP20:+--env TRANSFORM_SELFTEST_20MP=0} ${RENDERLOG:+--env TESSERA_DOC_RENDER_LOG=1} "$APP" --args --nonactivating --app-dir "$SP/app" --folder "$SP/folder" \
  "--transform-selftest=$SP/test"
sleep 2
PID=$(pgrep -nf "$APP/Contents/MacOS/Tessera")
echo "pid $PID"
end=$(( $(date +%s) + ${TIMEOUT:-1500} ))
while kill -0 "$PID" 2>/dev/null && [ "$(date +%s)" -lt "$end" ]; do
  for req in "$SP"/test/*.req; do
    [ -e "$req" ] || continue
    name=$(basename "$req" .req); wid=$(cat "$req"); rm -f "$req"
    screencapture -x -o -l "$wid" "$SP/test/$name.tmp.png" 2>>"$SP/capture.err"
    cp "$SP/test/$name.tmp.png" "$EV/$name.png" 2>/dev/null
    mv "$SP/test/$name.tmp.png" "$SP/test/$name.png"
    echo "captured $name"
  done
  grep -q "transform-selftest: done" "$LOG" && break
  sleep 0.3
done
sleep 3
if kill -0 "$PID" 2>/dev/null; then kill "$PID"; echo "killed $PID"; fi
cp "$LOG" "$EV/transform-selftest.log"
grep -E "transform-selftest: (check .*FAIL|done|399|382 latency)" "$LOG"
