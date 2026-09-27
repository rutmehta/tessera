# Machine A / Machine B coordination

This directory continues the two-machine workflow after the Claude coordinators
reached their usage limit on 2026-09-27. Both machines now use Codex. The original
work-package briefs and acceptance criteria still apply.

## Ownership

- Machine A owns engine work and integration into `main`.
- Machine B owns the layered-document app/UI work and its document FFI paths.
- Machine B pushes `wp/B5-*` branches and records readiness in
  `tools/orchestrate/wp/READY.md` on those branches. Machine A reviews and merges.
- Existing worktrees, uncommitted changes, test evidence, and unfinished workers
  must be preserved. A runner's old `pass` verdict is not sufficient when a later
  report or acceptance measurement fails.

## Communicating through Git

The Codex instance on Machine A currently lists only its local host. Machine B's
chat is not visible to it; no direct chat connection is established.

1. Machine A maintains `docs/coordination/MACHINE-A.md` on `main`.
2. Machine B has published its status at
   `tools/orchestrate/wp/B5-16/CODEX-TAKEOVER.md` on `wp/B5-16` (first recovered
   note: `fb16481`). Continue using that existing note and branch; agree on a new
   location in both notes before retiring the branch.
3. Fetch before reading the other machine's status. Machine A can read B's update
   without switching checkout:
   `git show origin/wp/B5-16:tools/orchestrate/wp/B5-16/CODEX-TAKEOVER.md`.
   Machine B can read A's update with
   `git show origin/main:docs/coordination/MACHINE-A.md`.
4. Include an update timestamp, branch and commit, active ownership, completed
   gates with actual failure counts, incomplete acceptance checks, and requests.
   Record acknowledgement of the other machine's last update.
5. Fetching or publishing a status does not wake the other Codex chat. Read the
   status when resuming work and before integration; automatic monitoring has
   not been configured.

Each machine edits only its own status file. The shared protocol is coordinated
by Machine A. Do not overwrite the other machine's file or merge feature work
solely because a status file calls it ready. Use the strict Swift gate; XCTest's
`(0 unexpected)` does not mean zero assertion failures. Serialize performance
measurements on each host and record load; never weaken thresholds to pass.
