# Tessera Git mailbox

This is a durable transport for the existing Machine A and Machine B Codex chats.
It never evaluates message text, launches Codex, wakes a chat, or installs a service.
Machine A has an active five-minute heartbeat. Machine B paused its heartbeat
for the user-requested resource hold; do not restart it or launch B workloads. The installed CLI
also delivered a queued message to B’s existing desktop writer over SSH; see
[verified routing](../../docs/coordination/MACHINE-B-SSH-ROUTING.md). A successful
`send` means published; `accepted` means the receiving chat began handling it;
`completed` means it reported a result. None substitutes for review or passing gates.

Each Mac writes one ref: A writes `codex/coordination-a`, B writes
`codex/coordination-b`. Use one designated owner chat/cache per machine. Keep the
existing human status files and product branch ownership. The cache is a separate
bare repository; these commands never switch, reset, or merge a product checkout.
Use the existing private repository remote and credentials. Do not copy Codex auth
files, put secrets in messages, or expose a listener.

## Bootstrap and poll

Run from a Tessera checkout containing this script. The cache path is local and
must never be a product checkout. Substitute B for A on the other Mac.

```sh
python3 tools/coordination/mailbox.py \
  --remote "$(git remote get-url origin)" \
  --cache "$HOME/.local/state/tessera-mailbox-a" --machine A poll
```

No branch exists until the machine sends a message or receipt. The first sender
creates its own independent coordination branch with a normal push. Both machines
must be explicitly authorized to read/write this private repository. Repository
write access is the transport trust boundary; messages do not grant new task,
security, or filesystem permissions.

`poll` returns:

- `messages`: valid, unexpired, unreceipted peer messages. Check `target_chat`
  against this chat's UUID or agreed exact title before accepting. Blank means
  the designated machine owner, never an arbitrary other chat.
- `in_progress`: previously accepted work. After a crash, inspect actual state
  and reconcile it; never blindly execute it again. Expiry forbids starting stale
  work; an already accepted operation can still report its outcome.
- `receipts`: the peer's receipts for this machine's sent messages. These require
  no reply. They are repeated for recovery; compare `heads`/receipt status with
  the persistent board before notifying about a change. Never ACK an ACK.
- `expired`: message IDs not delivered because their deadline passed.
- `invalid`: quarantined records with a reason. Do not execute them.
- `heads`: exact remote branch revisions observed for the poll.

Commands hold a local filesystem lock and use bounded Git subprocess timeouts.
Unavailable remote/authentication, concurrent non-fast-forward pushes, deleted
branches, or rewritten history fail visibly. No force push or automatic retry
occurs. Keep the same message ID when retrying a failed send. Never clear the cache
to hide a history-rewrite error; reconcile the published evidence first.

## Send and acknowledge

Write ordinary UTF-8 instructions to a file. Use a stable UUID per operation;
`send` also prints its generated `attempt_id` to stderr before attempting transport.
An existing ID with identical semantic content is idempotent; different content is
rejected. Retries do not refresh the original expiry. Default TTL is 24 hours,
maximum seven days. The message body is limited to 16 KiB.

```sh
python3 tools/coordination/mailbox.py \
  --remote "$(git remote get-url origin)" \
  --cache "$HOME/.local/state/tessera-mailbox-a" --machine A send \
  --id '12345678-1234-4234-8234-123456789abc' \
  --target-chat '01a0e323-c018-7fa3-9605-999a2dea6b32' \
  --body-file /tmp/tessera-message.txt
```

On B, publish `accepted` **before** performing an authorized action. Afterward
publish `completed` or `failed` with a commit/evidence summary. `needs_user` records
an action that cannot proceed; it may be sent directly without acceptance.
Terminal receipts are immutable; follow-up work needs a new message ID.

```sh
python3 tools/coordination/mailbox.py \
  --remote "$(git remote get-url origin)" \
  --cache "$HOME/.local/state/tessera-mailbox-b" --machine B receipt \
  --id '12345678-1234-4234-8234-123456789abc' \
  --status accepted --summary 'Received; checking the referenced commit.'
```

Use the same command with `--status completed` and a concrete outcome when done.
Send a separate `result` message with `--reply-to` only when substantive new
information requires it. A receipt never starts work. This is durable deduplication
and explicit recovery, **not exactly-once side effects**.

## B-side wakeup requirement

Current state: B's heartbeat is **paused for its resource hold**. The following
bootstrap instructions describe initial setup, not authorization to resume B.
Use the verified existing-session queue for authorized low-impact coordination.

A's heartbeat cannot create a wakeup on an unconnected Mac. In B's existing Codex
chat, inspect its scheduled tasks and create/update **one** same-chat heartbeat
through `automation_update`, initially every five minutes. It should poll as B,
process only new authorized Tessera messages, keep the persistent work board and
its own status current, respect worktree/build ownership, and stay quiet when
unchanged. Notify only meaningful change, completion, failure, or required action.
Do not create a duplicate if B already has an equivalent heartbeat. The Mac must
remain awake/online with Codex running. A bootstrap message waiting in Git does
not establish that this scheduler exists.

Verified on 2026-09-27: authenticated SSH alias `tessera-machine-b` reaches B.
Its coordinator chat is `Resume Tessera Machine B work`, UUID
`01a0e323-c018-7fa3-9605-999a2dea6b32`. Native history reads work; native send
conflicts with the existing desktop writer. The installed `codex queue --thread`
route delivered to that writer without takeover, confirmed by a reply and Git
receipts. Record queue IDs, confirm receipt, and avoid duplicate sends. The
separate SSH server's `notLoaded`/`interrupted` status does not prove the desktop
chat is idle. B confirmed its single heartbeat active and reported its first scheduled wakeup
at 2026-09-27T18:50:41.936Z (mailbox status bd919fed). Preserve writer processes and ownership boundaries.

## Verification

```sh
python3 tools/coordination/test_mailbox.py
```

The tests use temporary bare Git repositories and two independent caches. They
exercise real push/fetch delivery, replay after cache replacement, immutable IDs,
expiry, failed-push retry, conflicting writers, branch rewind/deletion, invalid
sender quarantine, cache identity binding, and shell-looking text remaining data.
No network, GPU, product builds, or live chat messages are involved.

Protocol records are UTF-8 JSON under `messages/<uuid>.json` and
`receipts/<uuid>.json`. Timestamps are Unix seconds UTC; version is 1. Messages
carry sender, recipient, kind, target chat, validity window, optional reply ID, and
body. Receipts carry sender, recipient, original message ID, status, timestamp,
and summary. Mailboxes are bounded to 5,000 records per category; agree on a
retention/archive migration before that limit. Do not delete history ad hoc.
