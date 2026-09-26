#!/usr/bin/env bash
# Coordinator fallback: when the Fable 5.1 session is downgraded (usage limit),
# the (now Opus 5.5) coordinator asks GPT-6 Astra (900k context) via the Codex CLI
# to act as supervisor/planner. Usage:
#   supervise.sh plan            # next wave: which packages, owners, briefs to write
#   supervise.sh review <wp-id>  # judge a finished package: merge / fix / escalate
#   supervise.sh ask "<question>"
set -euo pipefail
ROOT="$(git -C "$(dirname "$0")" rev-parse --show-toplevel)"
MODEL="${SUPERVISOR_MODEL:-gpt-6-astra-900k}"
mode="${1:-plan}"; shift || true
ctx=$(mktemp)
{
  echo "# Tessera coordinator context"
  echo; echo "## docs/11-execution-plan.md"; cat "$ROOT/docs/11-execution-plan.md"
  echo; echo "## docs/STATUS.md"; cat "$ROOT/docs/STATUS.md"
  echo; echo "## Board"; python3 "$ROOT/tools/orchestrate/board.py" list
  echo; echo "## git log (last 40)"; git -C "$ROOT" log --oneline -40
  echo; echo "## Open worktrees"; git -C "$ROOT" worktree list
} > "$ctx"
case "$mode" in
  plan) prompt="You are the supervisor/planner for the Tessera build (a multi-model orchestration: Opus 5.5 = design/UI/reviews, GPT-6 Astra via Hermes = engine executor, Sol = computer-use verifier). Read the context file $ctx. Decide the next wave: list up to 6 work packages with id, owner model, one-paragraph brief (acceptance test command + allowed paths), and dependencies/conflicts to avoid. Also list any merges/fixes the coordinator should do first. Be concrete and terse.";;
  review) wp="$1"; prompt="You are the supervisor for the Tessera build. Read the context file $ctx, then the package brief $ROOT/tools/orchestrate/wp/$wp/brief.md, its verdict.json and the latest attempts/*.log, and the diff of branch wp/$wp against main (run git in $ROOT). Judge: merge as-is, merge with a named follow-up, or send back with precise instructions. Give the exact git/commands the coordinator should run.";;
  ask) prompt="You are the supervisor for the Tessera build. Read the context file $ctx and answer: $*";;
  *) echo "usage: supervise.sh plan|review <wp>|ask <q>" >&2; exit 2;;
esac
codex exec -m "$MODEL" --dangerously-bypass-approvals-and-sandbox -C "$ROOT" -o "$ROOT/tools/orchestrate/supervisor-last.md" "$prompt" >/dev/null 2>&1 || true
cat "$ROOT/tools/orchestrate/supervisor-last.md"
rm -f "$ctx"
