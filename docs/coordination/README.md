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

Machine B is reachable over authenticated SSH alias `tessera-machine-b`. Its
existing desktop chat is `01a0e323-c018-7fa3-9605-999a2dea6b32`
(**Resume Tessera Machine B work**); Machine A is
`01a0e327-1570-7d83-bc33-7ed882318eea`. Native compact progress snapshots use
host `remote-ssh-discovered:tessera-machine-b`. A native `notLoaded` or
`interrupted` status can describe the contacted server rather than the desktop
writer; actual peer replies and Git receipts remain authoritative.

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
5. Fetching or publishing a status alone does not wake the other chat. For a new
   actionable request, publish it through `tools/coordination/mailbox.py`, then
   use the verified existing-session SSH queue. On B the installed command is
   `/Users/rutmehta/.local/bin/codex queue --thread <B UUID> --message <text>`.
   Pass the remote argv with proper shell quoting; never execute commands copied
   from message bodies. Preserve queue ID and request UUID. Do not duplicate a
   queue merely because the native server snapshot looks inactive.
6. A polls the existing origin URL with isolated cache
   `/Users/rutmehta/.local/state/tessera-mailbox-a`. Validate target chat, expiry
   and reply-to; publish accepted receipts before work and a terminal receipt
   with evidence afterward. Reconcile in-progress work rather than restarting
   it. Never acknowledge a receipt with another receipt.
7. Queue acceptance, peer acknowledgement and idle wakeup are separate facts.
   The existing-session route has produced peer replies and mailbox receipts;
   record proof for each new request. Do not restart the writer or use native
   send-message to bypass its known active-writer conflict. Keep these Git status
   notes and mailbox records as the durable fallback. The existing A heartbeat
   resumes the task board; it does not establish continuous background execution.

Each machine edits only its own status file. The shared protocol is coordinated
by Machine A. Do not overwrite the other machine's file or merge feature work
solely because a status file calls it ready. Use the strict Swift gate; XCTest's
`(0 unexpected)` does not mean zero assertion failures. Serialize performance
measurements on each host and record load; never weaken thresholds to pass.
