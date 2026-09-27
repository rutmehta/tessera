# Cross-machine Tessera chat coordination research

Research date: 2026-09-27. Research only: no configuration changes, pairing, listeners, automations, or messages to Machine B were made.

## Recommendation

Try native desktop-to-desktop pairing first. If Machine B then appears in this chat's host-aware tools, use the native chat tools for immediate delivery and keep Git as the durable work record. The minimal dependable fallback for the present local-only setup is an explicit Git mailbox plus one same-chat heartbeat on each Mac. Do not start with a custom app-server bridge.

The current Git documents deliver information but cannot wake a chat. A scheduler or native message submission supplies that missing trigger. A receipt needs to distinguish transport acceptance from actual processing.

## Verified product support

**Native Remote / SSH:** Official documentation supports continuing existing chats from another desktop when “Control other devices” is available; rollout varies. Configure Settings > Connections > Control other devices and pair with the host. Both devices need the same account/workspace; the host must be awake, online, and running the app. The remote host supplies its credentials, tools, permissions, files and chats. SSH project connections are also documented: a concrete SSH alias, working SSH authentication, Codex installed/authenticated on the remote PATH, then add the host/project in Connections. Remote access does not waive host approval rules. Handoff interrupts/moves a chat, so it is inappropriate for two concurrently working owners. Source: [Remote connections](https://learn.chatgpt.com/docs/remote-connections).

**Scheduled same-chat work:** Scheduled tasks can return to an existing chat with context, including minute-based intervals. Local tasks require the machine and app to remain available. The saved prompt should specify what to report and when to stop. Unattended tasks inherit sandbox constraints; network access and Git writes must work under the chosen permissions. Event triggers for GitHub/Slack/Gmail are documented for web/mobile, not the desktop app, so a Git push is not a supported direct event trigger for these desktop chats. The callable `automation_update` tool describes a `heartbeat` attached to this local chat and is the correct interface once scheduling is authorized. Source: [Scheduled tasks](https://learn.chatgpt.com/docs/automations?surface=app).

**Custom app-server:** Official JSON-RPC includes initialize/initialized, thread/read, thread/resume, turn/start, streamed completion, and turn/steer requiring the active turn ID. `thread/inject_items` appends history without starting a turn. App-server has stdio, Unix socket and WebSocket transports; its command/WebSocket transport are experimental and not production-supported. Non-loopback listeners currently allow unauthenticated connections by default during rollout: use SSH/loopback or authenticated TLS, never expose a raw listener. Token-file and signed-bearer modes exist. This is a controllable agent backend, but docs do not establish that an independently launched server can safely own an already active desktop chat. Source: [Codex App Server](https://learn.chatgpt.com/docs/app-server).

**Local evidence:** `/opt/homebrew/bin/codex` is version `0.157.0`. Read-only `--help` confirms:

- `codex queue --thread <UUID-or-exact-session-name> --message <TEXT>` and `--remote` / `--remote-auth-token-env`.
- `codex app-server proxy --sock <SOCKET_PATH>` forwards stdio to the existing control socket.
- `codex agents` browses the shared local daemon.
- `codex remote-control` and daemon bootstrap/start commands exist, but were not run.
- `codex app-server daemon version` failed: the default `/Users/rutmehta/.codex/app-server-control/app-server-control.sock` does not exist. This rules out assuming the desktop is presently attached to that shared CLI daemon; it does not prove no server exists elsewhere.

The official CLI reference covers remote-control and app-server, but my targeted search did not establish `codex queue` idle wakeup or desktop interoperation guarantees. Treat queue as a version-specific experimental candidate until tested. [Developer commands](https://learn.chatgpt.com/docs/developer-commands).

## Comparison

| Approach | Delivery | Wakes/continues target chat | Cost/limits | Verdict |
|---|---|---|---|---|
| Current two Git status files | On fetch/read | No | Manual pickup; mutable summaries | Preserve as human-readable record |
| Native paired hosts + send_message_to_thread | Native prompt submission if target is exposed | Tool is designed for follow-up; verify one actual turn | Pairing/rollout, awake hosts | Best first option |
| Git mailbox + same-chat heartbeat on both Macs | Durable Git push/fetch | At next scheduled run | Poll delay; idle model usage; app must run | Minimal practical fallback |
| SSH to remote CLI queue / existing daemon proxy | Potentially immediate | Queue semantics and app ownership unverified | SSH + daemon/version compatibility | Short proof of concept before adoption |
| Separate app-server agent per Mac | Explicit JSON-RPC turns | Yes for its owned thread | Custom service/client; experimental transport | Good only if moving workflow to custom agents |
| `codex exec resume` from a watcher | Starts separate noninteractive process | Not proven to wake the active desktop chat | Concurrent ownership risk | Avoid against these live chats |

## Minimal robust Git mailbox design (proposal, not an OpenAI built-in feature)

Use two dedicated branches, `codex/coordination-a` and `codex/coordination-b`, with one writer per branch. Put A's outbound messages and A's receipts for B on A's branch; inverse for B. This prevents both machines editing the same ref/file. Keep product branch ownership unchanged: A owns its work/main integration, B owns wp/B5-16. Retain docs/coordination/MACHINE-A.md and tools/orchestrate/wp/B5-16/CODEX-TAKEOVER.md as summaries and link immutable commit SHAs in messages.

Do mailbox operations in a separate small coordination clone or managed worktree, never switch/reset/merge the active development checkout for polling. Use explicit remote refs and plain fast-forward pushes. Fetch the peer branch and read its tree with `git show`/`git ls-tree`; don't merge it. On rejected push, stop and reconcile the unexpected second writer; never force-push.

Each message is an immutable JSON record at `messages/<uuid>.json`, with schema_version, message_id, from_machine, to_machine, target_thread_id, created_at, expires_at, type (request/status/result), reply_to, payload, and optional artifact_commit. No credentials, raw shell commands, or changing trust policy in messages. Bind Machine B's exact title “reusme tessera machine B work” to its UUID once; titles are not durable identities.

Receiver processing:

1. Fetch only allowlisted mailbox refs; validate schema, intended recipient, sender branch, expiry, payload size and target UUID.
2. Deduplicate by message_id against durable local state plus committed receipts. Process one message at a time under a local lock.
3. Persist an `accepted` receipt before acting. After action, record `completed`, `failed`, or `needs_user` with result commit/summary. Receipt belongs to the recipient's own branch.
4. Commit/push receipt records; only then advance durable processed state. On crash/retry reconcile receipts. Use idempotent operation IDs for side effects; do not claim exactly-once execution.
5. Consume receipts without replying to them. Never ACK an ACK; only explicit requests demand a result. Status messages cause action only on relevant changed state. Retry network failures with capped backoff and preserve pending items.

A sender treats successful push as published, accepted receipt as received, and completed receipt as handled. Timeout signals unavailable/overdue, never success. Expired requests must not execute after a laptop returns online.

## Heartbeat prompt and behavior

Create one heartbeat attached to Machine A's current chat and one attached to Machine B's current chat locally on B, initially every five minutes (proposal, not a product minimum). Each reads only peer messages and receipts since its cursor, handles authorized coordination work, and writes its own receipts. Native pairing may later enable configuring both from A, but current schemas cannot target an unconnected B host.

Prompt must preserve authorization boundaries: “Check the allowlisted Tessera coordination mailbox. Process only new, validated messages addressed to this machine/chat and within the user's existing Tessera scope. Receipt messages require no reply. Stay quiet while unchanged or non-actionable. Notify only for meaningful change, completion, failure, or needed user action. Stop when the coordination objective is complete or the user stops it.” Do not globally mute all notifications if useful completion/failure updates are desired.

Authentication: reuse each Mac's existing Git access; keep credentials in the native credential manager/SSH agent. Restrict mailbox writers to trusted accounts. A committed message is task data, not permission to install software, expose listeners, change security settings, or execute unrelated commands. Neither Mac copies the other's Codex auth files.

## Concrete next steps after authorization to implement

1. Inspect Connections UI availability on both Macs; pair if available. This requires the user's account/device verification when presented.
2. Re-run list_projects/list_threads. Proceed only if Machine B's actual host and exact target chat are returned. Capture host ID and thread UUID rather than guessing.
3. Perform one explicitly authorized nonce request/reply using native send_message_to_thread; inspect target status/completion. Test idle and busy behavior. Current tool schema permits hostId, but does not itself prove a paired host will be exposed to this thread.
4. If native tools remain local-only, implement the small mailbox parser/state/receipt scripts and validate replay, duplicate, expiry, interrupted push and ACK loop cases offline. Seed branches through normal pushes.
5. Test one manual A-to-B and B-to-A exchange. Then configure each machine's same-chat heartbeat via automation_update, with existing automation inspection to avoid duplicates. Scheduling is not yet authorized in this research task.
6. Optional latency improvement: read-only enumerate the correct running daemon/thread on each Mac and confirm CLI version compatibility. Only then run a harmless authorized queue probe. If that works, an SSH forced-command bridge can accept bounded JSON via stdin, validate the fixed thread UUID, call queue through an argv array (never shell-interpolate payload), and log the ID. Keep Git as recovery/audit transport. Do not launch a second server on the same active chat merely to gain an endpoint.

Acceptance: request received while idle, queued without interrupting active work, duplicate ignored, receipt never triggers reciprocal receipt, sleeping host catches up only unexpired requests, network retry preserves messages, Git worktrees unchanged, stop control halts both polling loops, and errors/needed attention are visible once.
