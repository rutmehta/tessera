# WP M0-02 — Orchestration harness

Build the scripts under `tools/orchestrate/` that the coordinator uses to run worker models. Bash (zsh-compatible), plus jq-free (use python3 for JSON). All scripts live under `tools/orchestrate/` only. Do not touch anything outside that directory.

## Facts (verified)
- Worker CLI is Hermes: `hermes -z "<prompt>" --provider openai-codex -m gpt-6-luna --yolo --ignore-user-config --in <dir>` runs one non-interactive turn in `<dir>` and prints the final reply to stdout. Sol is the same with `-m gpt-6-sol`, and computer use is enabled by adding `-t computer_use`.
- Repo root: the directory containing `tools/`. Worktrees go in `.worktrees/<wp-id>` on branch `wp/<wp-id>`; `.worktrees/` must be git-ignored (add to `.gitignore` at repo root — this is the one exception to the path rule).
- Each WP has `tools/orchestrate/wp/<id>/brief.md`, optional `acceptance.md`, and the scripts write `tools/orchestrate/wp/<id>/attempts/<n>.log`, `verdict.json`.

## Deliverables
1. `run-luna.sh <wp-id> [--model gpt-6-luna] [--test "<cmd>"] [--max-attempts 3] [--paths "glob1,glob2"]`
   - Creates the worktree if missing (`git worktree add -b wp/<id> .worktrees/<id> main`, reuse if exists).
   - Builds the prompt: contents of brief.md, plus a fixed preamble that states: work only inside the worktree, only touch the allowed paths, run the test command yourself before finishing, finish by printing a line `RESULT: PASS` or `RESULT: FAIL <reason>`.
   - Runs Hermes with `--in <worktree>`. Captures stdout to attempts/<n>.log.
   - After each attempt, runs the test command in the worktree (if given), and runs `git -C <worktree> diff --name-only main` and rejects (FAIL) any changed file outside the allowed path globs.
   - On failure, re-runs with the previous attempt's tail (last 200 lines of test output + violations) appended under a heading "Previous attempt failed", up to max attempts.
   - Writes `verdict.json` {wp, status: pass|fail|escalate, attempts, last_test_exit, violations:[...]} and exits 0 on pass, 2 on escalate.
   - Commits in the worktree after a pass with message `wp(<id>): <first line of brief title>`.
2. `verify-sol.sh <wp-id> [--model gpt-6-sol]`
   - Prompt = acceptance.md + preamble: use the computer_use tools to perform each numbered step, after each step run the shell command `screencapture -x <abs evidence dir>/step-<n>.png` to save evidence (the computer_use screenshot tool cannot write files; the terminal tool can), never edit source files, end with a JSON block `{"steps":[{"n":1,"pass":true,"note":"..."}], "overall": true}`.
   - Runs Hermes with `-t computer_use`, saves stdout to `evidence/transcript.log`, extracts the trailing JSON into `verdict.json`.
3. `board.py` — `python3 board.py {list|set <id> <field> <value>|add <id> <owner> "<title>"}` over `tools/orchestrate/board.json` (list of {id, title, owner: fable|opus|luna|sol, status: todo|running|pass|fail|escalate|merged, attempts, notes}). Pretty table on list.
4. `merge-wp.sh <wp-id>` — from repo root: verifies verdict.json status is pass, `git merge --no-ff wp/<id>` into current branch, removes the worktree, marks board merged. Refuses if working tree is dirty.
5. `README.md` in tools/orchestrate describing usage in 20 lines.

## Test command for this WP
`bash tools/orchestrate/selftest.sh` — write this too: it creates a temp git repo with a dummy WP whose brief says "create hello.txt containing hi", runs run-luna.sh with `--test "test -f hello.txt"` and `--model gpt-6-luna`, and asserts verdict.json status == pass and that the worktree commit exists. It must also assert that a WP with `--paths "allowed/**"` that writes outside gets status fail/escalate (simulate by pre-seeding a change rather than relying on the model). Keep the real-model call to one attempt so the selftest is cheap.
