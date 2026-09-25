#!/usr/bin/env bash
set -euo pipefail
ROOT="$(git rev-parse --show-toplevel)"; [[ $# == 1 ]] || { echo 'Usage: merge-wp.sh <wp-id>' >&2; exit 2; }; WP=$1
[[ -z $(git -C "$ROOT" status --porcelain) ]] || { echo 'Refusing: working tree is dirty' >&2; exit 2; }
VER="$ROOT/tools/orchestrate/wp/$WP/verdict.json"; [[ -f $VER ]] || { echo 'Missing verdict' >&2; exit 2; }
[[ $(python3 -c 'import json,sys; print(json.load(open(sys.argv[1])).get("status"))' "$VER") == pass ]] || { echo 'Verdict is not pass' >&2; exit 2; }
git -C "$ROOT" merge --no-ff "wp/$WP" -m "Merge wp/$WP"
git -C "$ROOT" worktree remove "$ROOT/.worktrees/$WP"
python3 "$ROOT/tools/orchestrate/board.py" set "$WP" status merged
