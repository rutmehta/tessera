#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"; [[ $# -ge 1 ]] || { echo 'Usage: verify-sol.sh <wp-id> [--model gpt-6-sol]' >&2; exit 2; }
WP=$1; shift; MODEL=gpt-6-sol; [[ $# -eq 0 ]] || { [[ $1 == --model && $# == 2 ]] || exit 2; MODEL=$2; }
BASE="$ROOT/tools/orchestrate/wp/$WP"; WT="$ROOT/.worktrees/$WP"; [[ -d $WT ]] || WT="$ROOT"; ACC="$BASE/acceptance.md"; [[ -f $ACC ]] || { echo "Missing $ACC" >&2; exit 2; }
mkdir -p "$BASE/evidence"; PROMPT=$(python3 - "$ACC" "$BASE/evidence" <<'PY'
import pathlib,sys
print('Use the computer_use tools to perform each numbered step in the acceptance criteria. After each step take a screenshot with the computer_use screenshot tool (do NOT use screencapture; it has no screen-recording permission from this shell). Never edit source files. End with a JSON block {"steps":[{"n":1,"pass":true,"note":"..."}], "overall": true}.\n\n'+pathlib.Path(sys.argv[1]).read_text())
PY
)
START=$(date +%s); touch "$BASE/evidence/.start"; hermes -z "$PROMPT" --provider openai-codex -m "$MODEL" --yolo --ignore-user-config -t computer_use --in "$WT" >"$BASE/evidence/transcript.log" 2>&1
# collect the screenshots hermes cached during this run as evidence
find "$HOME/.hermes/cache/images" -name "computer_use_*.png" -newer "$BASE/evidence/.start" 2>/dev/null | sort | nl | while read -r n f; do cp "$f" "$BASE/evidence/shot-$(printf %02d "$n").png"; done
python3 - "$BASE/evidence/transcript.log" "$BASE/verdict.json" "$WP" <<'PY'
import sys,json,pathlib,re
text=pathlib.Path(sys.argv[1]).read_text(); starts=[m.start() for m in re.finditer(r'\{\s*"steps"\s*:',text)]
if not starts: raise SystemExit('No trailing verdict JSON found')
data=json.JSONDecoder().raw_decode(text[starts[-1]:])[0]; data.update(wp=sys.argv[3],status='pass' if data.get('overall') else 'fail')
pathlib.Path(sys.argv[2]).write_text(json.dumps(data,indent=2)+'\n')
PY
