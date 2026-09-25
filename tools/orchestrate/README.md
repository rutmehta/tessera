Orchestration harness

1. Add a WP brief at wp/<id>/brief.md and optionally acceptance.md.
2. Run run-luna.sh <id> [--model gpt-6-luna] [--test '<cmd>'] [--max-attempts 3] [--paths 'glob1,glob2'].
3. The runner creates/reuses .worktrees/<id>, runs Hermes, tests, checks allowed paths, and records attempts and verdict.json.
4. Passing work is committed as wp(<id>): <brief title>.
5. Run verify-sol.sh <id> [--model gpt-6-sol] for acceptance verification with computer use and evidence.
6. Review the transcript and verdict under wp/<id>/evidence/.
7. board.py list displays the board; board.py add <id> <owner> '<title>' creates a card.
8. board.py set <id> <field> <value> updates title, owner, status, attempts, or notes.
9. merge-wp.sh <id> merges a passing branch into the current branch and removes its worktree.
10. The merge helper refuses to run with a dirty working tree.
11. Statuses: todo, running, pass, fail, escalate, merged.
12. Owner values: fable, opus, luna, sol.
13. Keep model-generated changes within the selected WP's allowed paths.
14. The runner records stdout in attempts/<n>.log and retries failures up to its configured limit.
15. Retries receive the prior test output and path violations.
16. Self-test the harness with bash tools/orchestrate/selftest.sh.
