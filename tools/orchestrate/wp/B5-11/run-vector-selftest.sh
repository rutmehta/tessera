#!/bin/bash
# Launches this worktree's Tessera in the background (never activated), runs --vector-selftest,
# captures its own window (screencapture -l <windowID>) at each step, then quits only its PID.
set -u
WT=/Users/rutmehta/Developer/lightroom/.worktrees/B5-11
SP=/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/74acb14f-b2e3-4e99-8ce1-272270467adc/scratchpad/vector-run
EV=$WT/tools/orchestrate/wp/B5-11/evidence
rm -rf "$SP"; mkdir -p "$SP/app" "$SP/folder" "$SP/test" "$EV"
LOG=$SP/selftest.log
: > "$LOG"
APP=$WT/apps/mac/build/Tessera.app
open -g -n --stderr "$LOG" "$APP" --args --app-dir "$SP/app" --folder "$SP/folder" --new-document "--vector-selftest=$SP/test" ${HOLD:+--vector-selftest-hold $HOLD}
sleep 2
PID=$(pgrep -nf "$APP/Contents/MacOS/Tessera")
echo "pid $PID"
done_steps=""
end=$(( $(date +%s) + ${TIMEOUT:-900} ))
while kill -0 "$PID" 2>/dev/null && [ "$(date +%s)" -lt "$end" ]; do
  while read -r n name wid; do
    case " $done_steps " in *" $n "*) continue;; esac
    screencapture -x -o -l "$wid" "$EV/vector-$n-$name.png" 2>>"$SP/capture.err"
    touch "$SP/test/ack-$n"
    done_steps="$done_steps $n"
    echo "captured $n $name"
  done < <(sed -n "s/^vector-selftest: step \([0-9]*\)-\([^ ]*\) window-id \([0-9]*\)$/\1 \2 \3/p" "$LOG")
  grep -q "vector-selftest: done" "$LOG" && break
  sleep 0.3
done
sleep 2
if kill -0 "$PID" 2>/dev/null; then kill "$PID"; echo "killed $PID"; fi
cp "$LOG" "$EV/vector-selftest.log"
grep -E "vector-selftest: (check .*FAIL|done|affine drag latency|document)" "$LOG"
