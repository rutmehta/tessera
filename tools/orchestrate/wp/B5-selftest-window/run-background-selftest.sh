#!/bin/bash
# Runs one in-app document self-test in the background: `open -g -n` plus `--nonactivating`, so the app is never
# activated and no window is made key or ordered front (SelfTestHost hosts the content in a window behind every
# other app). Captures only its own window when the test asks (`<name>.req` files or `step <n>-<name> window-id <id>`
# lines, answered with `ack-<nn>`), checks the frontmost app did not change, then quits only its own PID.
#
# usage: run-background-selftest.sh <name> [extra app args…]
#   name: transform | vector | channel-paint | camera-raw | retouch | filter | styles | tools | document |
#         liquify | channels | text | stack
# env:   APP (default: this worktree's apps/mac/build/Tessera.app), SP (scratch dir), TIMEOUT (s, default 900),
#        FOLDER (library folder; default: a copy of sample.dng for the tests that need a RAW)
set -u
NAME=${1:?usage: $0 <name> [extra args…]}; shift
ROOT=$(cd "$(dirname "$0")/../../../.." && pwd)
APP=${APP:-$ROOT/apps/mac/build/Tessera.app}
SP=${SP:-${TMPDIR:-/tmp}/tessera-bg-selftest/$NAME}
rm -rf "$SP"; mkdir -p "$SP/app" "$SP/folder" "$SP/test"
LOG=$SP/selftest.log; : > "$LOG"
FIX=$ROOT/fixtures/raw/sample.dng
ENV=(); ARGS=()
case $NAME in
  transform|vector|channel-paint|liquify) ARGS=("--$NAME-selftest=$SP/test");;
  camera-raw|retouch|filter|styles|tools|document)
    ARGS=("--$NAME-selftest" "$SP/test")
    [ -z "${FOLDER:-}" ] && cp "$FIX" "$SP/folder/";;
  channels|text|stack) ENV=(--env "TESSERA_$(echo "$NAME" | tr a-z A-Z)_SELFTEST=$SP/test");;
  *) echo "unknown self-test $NAME" >&2; exit 2;;
esac
FOLDER=${FOLDER:-$SP/folder}
FRONT_BEFORE=$(lsappinfo info -only name "$(lsappinfo front)")
open -g -n --stderr "$LOG" ${ENV[@]+"${ENV[@]}"} "$APP" --args --nonactivating --app-dir "$SP/app" --folder "$FOLDER" ${ARGS[@]+"${ARGS[@]}"} "$@"
sleep 2
PID=$(pgrep -nf "$APP/Contents/MacOS/Tessera")
echo "pid $PID"
acked=""
end=$(( $(date +%s) + ${TIMEOUT:-900} ))
while kill -0 "$PID" 2>/dev/null && [ "$(date +%s)" -lt "$end" ]; do
  for req in "$SP"/test/*.req; do
    [ -e "$req" ] || continue
    n=$(basename "$req" .req); wid=$(cat "$req"); rm -f "$req"
    screencapture -x -o -l "$wid" "$SP/test/$n.tmp.png" 2>>"$SP/capture.err"; mv "$SP/test/$n.tmp.png" "$SP/test/$n.png" 2>/dev/null \
      || touch "$SP/test/$n.png"
  done
  while read -r n step wid; do
    case " $acked " in *" $n "*) continue;; esac
    screencapture -x -o -l "$wid" "$SP/test/step-$n-$step.png" 2>>"$SP/capture.err"
    touch "$SP/test/ack-$(printf %02d "$((10#$n))")"; acked="$acked $n"
  done < <(sed -n 's/^[a-z-]*selftest: step \([0-9]*\)-\([^ ]*\) window-id \([0-9]*\)$/\1 \2 \3/p' "$LOG")
  grep -qE "^[a-z-]*selftest: done" "$LOG" && break
  sleep 0.3
done
sleep 3
if kill -0 "$PID" 2>/dev/null; then kill "$PID"; sleep 1; kill -9 "$PID" 2>/dev/null; echo "killed $PID"; fi
FRONT_AFTER=$(lsappinfo info -only name "$(lsappinfo front)")
echo "front before: $FRONT_BEFORE"; echo "front after:  $FRONT_AFTER"
grep -E "selftest-host|selftest: (check .*FAIL|FAIL|done)" "$LOG"
grep -qE "^[a-z-]*selftest: done, 0 failure" "$LOG" && [ "$FRONT_BEFORE" = "$FRONT_AFTER" ]
