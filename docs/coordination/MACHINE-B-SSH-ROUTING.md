# Machine B active-writer routing check — 2026-09-27

Target: `01a0e323-c018-7fa3-9605-999a2dea6b32`, “Resume Tessera Machine B work”. SSH-discovered host: `remote-ssh-discovered:tessera-machine-b`.

## Verified follow-up: existing-session queue succeeds

The initial read-only investigation below did not test `codex queue`. A later
bounded invocation of the installed CLI succeeded without writer takeover:

`/Users/rutmehta/.local/bin/codex queue --thread 01a0e323-c018-7fa3-9605-999a2dea6b32 --message TEXT`

Invoked over authenticated SSH, it returned exit0 and queue ID
`01a0e428-7e56-7850-8b37-b3bc4faac94e`. B’s existing chat replied in turn
`01a0e428-7ff7-7520-91dd-d2f67f451483`, then published an **accepted** bootstrap
receipt on `codex/coordination-b` at `94dd36bd58d4ee03da266caaad55acf9649fcd46`.
This establishes actual delivery for this installed CLI/version/setup. The CLI
help explicitly describes queuing to an existing session; no bypass, takeover,
extra session, restart or authentication copy was requested.

Use exact UUID addressing and a bounded command with safely quoted message text.
Record queue IDs and avoid duplicate enqueueing. Queue success alone is not a
receipt; confirm B’s reply or typed Git receipt. The separate SSH app-server still
reported `notLoaded`/`interrupted` while B published work, so that view does not
reliably describe the desktop writer’s live state. Git mailbox receipts and
explicit peer evidence remain authoritative for coordination outcomes.

## Initial read-only conclusion (superseded by the verified queue experiment)

No verified supported route from the presently exposed SSH app-server to the other process that owns this desktop chat. Do not equate `notLoaded` with “no writer anywhere”: official thread/read is a persisted read which does not load a thread; loaded status belongs to the contacted server. Parent's observed successful read plus active-writer rejection is consistent with a different server retaining ownership, but does not identify that writer conclusively.

The documented existing-desktop route is device Remote Control: pair the running B desktop via Settings > Connections > Control this Mac and add it through A's Control other devices (where available). That reaches the desktop host's existing chats. SSH-host discovery instead starts/manages a remote app-server and is not documented as attaching to another desktop process's stdin. No currently exposed tool provides a pairing or writer-routing operation.

## Read-only observations

- SSH login-shell `command -v codex` resolves `/Users/rutmehta/.local/bin/codex`, version `0.157.0`. Plain non-login SSH shell cannot find it; use `zsh -lc` for documented CLI inspection.
- `codex app-server proxy --help`: forwards stdin/stdout to an existing control socket, optionally `--sock PATH`.
- `codex queue --help`: existing-session queue command with `--thread`, `--message`, and optional `--remote` accepting ws/wss/unix endpoints. This command is present in the installed version; current official developer-command page does not document its cross-process writer forwarding or idle desktop wakeup semantics. It was NOT invoked.
- Process metadata and Unix socket names only, no auth/config/DB/lock contents: PID18176 is desktop-bundled app-server, no explicit listen argument and no named Unix endpoint in lsof. PID35613 is desktop-bundled `--listen stdio://`, also no named endpoint. PID39855 is the separate daemon `--listen unix://`, with a named `/private/tmp/codex-daemon-501/...` socket. PID39863 is another codex process without a listen option. Process existence alone does not identify the chat writer.
- None of the observed desktop server endpoints gives an attachable documented named control socket. The named daemon socket is not evidence that it owns this chat. No socket connection/resume/send attempted by this investigation.

## What can and cannot route safely

If a control socket belonging to the actual owner were already exposed and verified, an initialized client could use the documented loaded-thread lifecycle there. `turn/steer` requires the *current active* turn ID and fails without an active turn; a persisted completed turn from a different server is not suitable. `thread/inject_items` requires a loaded thread and does not start a user turn, so it is not a wakeup substitute. `thread/resume` on the separate server already fails ownership and should not be retried as a takeover.

Supported next product setup: desktop-to-desktop Remote Control pairing, if available, or the user interacting with the already-owning B desktop chat. Existing Git mailbox remains the durable fallback, requiring the B-owned chat to poll/read it. No claim of delivered/woken B chat.

No writer takeover, restart/kill, additional server, DB/lock change, auth read, service/config mutation, or GUI bypass performed. Gain-map worktree untouched.

## Sources actually fetched

- https://learn.chatgpt.com/docs/remote-connections — existing desktop chats, Control other devices, SSH starts remote app-server, pairing/security boundaries.
- https://learn.chatgpt.com/docs/app-server — transports, read versus resume, loaded/list, turn/steer, inject_items.
- https://learn.chatgpt.com/docs/developer-commands — app-server experimental status and remote-control daemon commands; no `codex queue` entry found.
