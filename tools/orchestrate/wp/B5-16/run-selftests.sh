#!/bin/bash
# B5-16: runs the document self-tests against this worktree's apps/mac/build/Tessera.app in the
# background only: `open -g -n ... --nonactivating` (never activated, never fronted, no keyboard focus),
# screenshots only of its own window (`screencapture -x -o -l <windowID>`) for steps that print a
# window id, then quits only its own PID. Usage: run-selftests.sh [document tools filter retouch styles
# channels text vector] (default: all). Prints each test's `done, N failure(s)` line.
set -u
WT=/Users/rutmehta/Developer/lightroom/.worktrees/B5-16
APP=$WT/apps/mac/build/Tessera.app
SP=${SP:-/private/tmp/claude-501/-Users-rutmehta-Developer-lightroom/74acb14f-b2e3-4e99-8ce1-272270467adc/scratchpad/selftests}
EV=$WT/tools/orchestrate/wp/B5-16/evidence/selftests
mkdir -p "$SP" "$EV"

run() {
  local name=$1 prefix=$2 timeout=$3; shift 3
  local D=$SP/$name LOG=$SP/$name/stderr.log
  rm -rf "$D"; mkdir -p "$D/app" "$D/folder" "$D/out"
  cp "$WT"/fixtures/raw/* "$D/folder/" 2>/dev/null
  : > "$LOG"
  local envs=() args=()
  for a in "$@"; do
    case "$a" in ENV:*) envs+=(--env "${a#ENV:}");; *) args+=("${a//@OUT@/$D/out}");; esac
  done
  open -g -n --stderr "$LOG" ${envs[@]+"${envs[@]}"} "$APP" --args --nonactivating --app-dir "$D/app" --folder "$D/folder" "${args[@]}"
  sleep 2
  local PID
  PID=$(pgrep -nf "$APP/Contents/MacOS/Tessera")
  echo "[$name] pid $PID"
  local seen="" end=$(( $(date +%s) + timeout ))
  while kill -0 "$PID" 2>/dev/null && [ "$(date +%s)" -lt "$end" ]; do
    # Steps that name their window: capture that window only (vector waits for an ack file).
    while read -r n wid; do
      case " $seen " in *" $n "*) continue;; esac
      screencapture -x -o -l "$wid" "$EV/$name-$n.png" 2>>"$D/capture.err"
      touch "$D/out/ack-${n%%-*}"
      seen="$seen $n"
    done < <(sed -n -E -e "s/^$prefix: step ([0-9]+[^ ]*) .*window-id ([0-9]+).*$/\1 \2/p" \
                       -e "s/^$prefix: step ([0-9]+) .* sheet ([0-9]+)$/\1 \2/p" "$LOG")
    grep -q "$prefix: done" "$LOG" && break
    sleep 0.3
  done
  sleep 3
  if kill -0 "$PID" 2>/dev/null; then kill "$PID"; sleep 1; fi
  cp "$LOG" "$EV/$name-selftest.log"
  local line; line=$(grep -m1 "$prefix: done" "$LOG")
  echo "[$name] ${line:-NO DONE LINE (timeout or crash)}"
  grep -E "$prefix: check .*FAIL" "$LOG" | head -20
}

tests=("$@"); [ ${#tests[@]} -eq 0 ] && tests=(document tools filter retouch styles channels text vector)
for t in "${tests[@]}"; do
  case $t in
    document) run document document-selftest 900 --new-document --document-selftest @OUT@ --document-selftest-hold 0.3 ;;
    tools)    run tools tools-selftest 900 --new-document --tools-selftest @OUT@ --tools-selftest-hold 0.3 ;;
    filter)   run filter filter-selftest 900 --new-document --filter-selftest @OUT@ --filter-selftest-hold 0.3 ;;
    retouch)  run retouch retouch-selftest 1500 --new-document --retouch-selftest=@OUT@ --retouch-selftest-hold 0.3 ;;
    styles)   run styles styles-selftest 900 --new-document --styles-selftest @OUT@ --styles-selftest-hold 0.3 ;;
    channels) run channels channels-selftest 900 ENV:TESSERA_CHANNELS_SELFTEST=$SP/channels/out ENV:TESSERA_CHANNELS_SELFTEST_HOLD=0.3 --new-document ;;
    text)     run text text-selftest 1500 ENV:TESSERA_TEXT_SELFTEST=$SP/text/out --new-document ;;
    vector)   run vector vector-selftest 1800 --new-document --vector-selftest=@OUT@ ;;
    *) echo "unknown $t" ;;
  esac
done
