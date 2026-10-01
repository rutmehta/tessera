#!/bin/bash
# Run only from this worktree; never activate or capture the application.
set -euo pipefail
cd "$(dirname "$0")/../../../.."
root="$PWD"
mode="${1:-after-full}"
evidence="$root/tools/orchestrate/wp/B5-40/evidence"
run="$evidence/$mode"
mkdir -p "$run/out" "$run/appdir"
if [[ "${QUIET_ALREADY_WAITED:-0}" != 1 ]]; then
  /usr/bin/python3 "$root/tools/orchestrate/wp/B5-40/wait-quiet.py" "$run/quiet.jsonl"
else
  printf '%s\n' 'Authorized contended baseline after bounded quiet wait; see quiet-before-continuation.jsonl and before-export-1/quiet.jsonl.' > "$run/quiet-fallback.txt"
fi
/usr/bin/python3 -u - "$run/load-during.jsonl" <<'PYLOAD' &
import datetime, json, os, sys, time
with open(sys.argv[1], 'a', buffering=1) as log:
    while True:
        log.write(json.dumps(dict(time=datetime.datetime.now().astimezone().isoformat(), load1=os.getloadavg()[0])) + '\n')
        time.sleep(1)
PYLOAD
load_sampler=$!
trap 'kill "$load_sampler" 2>/dev/null || true' EXIT
uptime > "$run/host-load.txt"
df -h "$root" >> "$run/host-load.txt"
args=(--env TESSERA_FILTER_PERF=1)
if [[ "$mode" == *export* ]]; then args+=(--env TESSERA_FILTER_PERF_EXPORT_ONLY=1); fi
open -g -n "${args[@]}" --stdout "$run/stdout.log" --stderr "$run/stderr.log" \
  "$root/apps/mac/build/Tessera.app" --args --nonactivating \
  --app-dir "$run/appdir" --folder "$evidence/photos" \
  --filter-selftest "$run/out" --timing-output "$run/spans.json"
for attempt in {1..30}; do
  pid=$(pgrep -f "^$root/apps/mac/build/Tessera.app/Contents/MacOS/Tessera .*--app-dir $run/appdir" || true)
  [[ -n "$pid" ]] && break
  sleep 1
done
[[ -n "$pid" ]] || { echo 'Failed to find owned self-test PID'; exit 1; }
printf '%s\n' "$pid" > "$run/pid.txt"
profiler=""
if [[ "${PROFILE:-1}" == 1 ]]; then
xctrace record --template 'Time Profiler' --attach "$pid" --time-limit 5m \
  --output "$run/time-profiler.trace" > "$run/xctrace.log" 2>&1 &
profiler=$!
fi
while kill -0 "$pid" 2>/dev/null; do sleep 5; done
if [[ -n "$profiler" ]]; then wait "$profiler" || true; fi
uptime >> "$run/host-load.txt"
cat "$run/stderr.log"
