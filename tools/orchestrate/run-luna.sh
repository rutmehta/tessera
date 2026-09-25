#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"
usage(){ echo "Usage: run-luna.sh <wp-id> [--model MODEL] [--test CMD] [--max-attempts N] [--paths GLOBS]" >&2; exit 2; }
[[ $# -ge 1 ]] || usage
WP=$1; shift
MODEL=gpt-6-luna; TEST_CMD=; MAX=3; PATHS='tools/orchestrate/wp/'"$WP"'/**'
while [[ $# -gt 0 ]]; do case "$1" in --model) MODEL=$2; shift 2;; --test) TEST_CMD=$2; shift 2;; --max-attempts) MAX=$2; shift 2;; --paths) PATHS=$2; shift 2;; *) usage;; esac; done
export CARGO_TARGET_DIR="$HOME/.cache/photo-engine-target/$WP"; mkdir -p "$CARGO_TARGET_DIR"
BASE="$ROOT/tools/orchestrate/wp/$WP"; BRIEF="$BASE/brief.md"; WT="$ROOT/.worktrees/$WP"
[[ -f $BRIEF ]] || { echo "Missing $BRIEF" >&2; exit 2; }
mkdir -p "$BASE/attempts"
if [[ ! -d $WT ]]; then mkdir -p "$ROOT/.worktrees"; git -C "$ROOT" worktree add -b "wp/$WP" "$WT" main; fi
python3 - "$BRIEF" "$BASE/prompt.txt" "$WT" "$PATHS" "$TEST_CMD" <<'PY'
import pathlib,sys,os
brief,out,wt,paths,test=sys.argv[1:]
t=pathlib.Path(brief).read_text()
p=f'''You are implementing work package {pathlib.Path(brief).parent.name}. Work only inside the worktree at {wt}. Only touch allowed paths matching: {paths}. IMPORTANT: the repo path contains a colon, which breaks cargo on macOS unless CARGO_TARGET_DIR points outside the repo. It is already exported in your environment as {os.environ.get('CARGO_TARGET_DIR','')}; keep it set for every cargo command (never build into ./target, never commit target/). Run this test command yourself before finishing: {test or '(no test command supplied)'}. Finish by printing a line RESULT: PASS or RESULT: FAIL <reason>.\n\n{t}'''
pathlib.Path(out).write_text(p)
PY
status=fail; last_exit=0; violations=(); previous=''
for ((n=1;n<=MAX;n++)); do
  prompt=$(cat "$BASE/prompt.txt"); [[ -z $previous ]] || prompt+=$'\n\nPrevious attempt failed\n'"$previous"
  set +e; hermes -z "$prompt" --provider openai-codex -m "$MODEL" --yolo --ignore-user-config --in "$WT" >"$BASE/attempts/$n.log" 2>&1; hrc=$?; set -e
  testout=''; last_exit=0
  if [[ -n $TEST_CMD ]]; then set +e; testout=$(cd "$WT" && bash -lc "$TEST_CMD" 2>&1); last_exit=$?; set -e; fi
  changed=$(git -C "$WT" diff --name-only "$(git -C "$WT" merge-base main HEAD)")
  changed+=$'\n'"$(git -C "$WT" ls-files --others --exclude-standard)"
  violations=(); IFS=',' read -r -a pats <<< "$PATHS"
  while IFS= read -r f; do [[ -z $f ]] && continue; ok=0; for p in "${pats[@]}"; do
    case "$p" in */\*\*) prefix=${p%\*\*}; [[ $f == "$prefix"* ]] && ok=1;; *\*) prefix=${p%\*}; [[ $f == "$prefix"* ]] && ok=1;; *) [[ $f == "$p" ]] && ok=1;; esac
  done; ((ok)) || violations+=("$f"); done <<< "$changed"
  if (( hrc == 0 && last_exit == 0 )) && ((${#violations[@]}==0)) && grep -q '^RESULT: PASS' "$BASE/attempts/$n.log"; then status=pass; break; fi
  violation_text="${violations[*]-}"
  previous="$(printf '%s\n' "$testout" | tail -n 200) violations: $violation_text hermes_exit=$hrc"
done
if [[ $status == pass ]]; then git -C "$WT" add -A; git -C "$WT" diff --cached --quiet || git -C "$WT" commit -q -m "wp($WP): $(head -n 1 "$BRIEF" | sed 's/^# *//')"; fi
if [[ $status != pass && $n -gt $MAX ]]; then status=escalate; fi
python3 - "$BASE/verdict.json" "$WP" "$status" "$n" "$last_exit" "${violation_text:-}" <<'PY'
import json,sys,pathlib
p,wp,status,n,exitcode,v=sys.argv[1:]
pathlib.Path(p).write_text(json.dumps(dict(wp=wp,status=status,attempts=int(n),last_test_exit=int(exitcode),violations=v.split() if v else []),indent=2)+'\n')
PY
[[ $status == pass ]] && exit 0
[[ $status == escalate ]] && exit 2
exit 1
