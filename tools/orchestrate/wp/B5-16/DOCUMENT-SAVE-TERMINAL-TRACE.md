# Save As terminal lifecycle diagnostic

Request: 95510c13-77dc-4af0-8be4-aa4ebe0fa1f6. Diagnostic commit: cd967702 on codex/document-save-terminal-trace, based directly on failed GUI candidate e8ddabfa71fd6f4243b6bcaac0f529a15a1cf379. Document source is identical between that base and A evidence tip 4d92ddb3. The existing B source branch remains preserved at e38caf11, including independent statusPublication/activateDocument APIs; this diagnostic does not replace or revert those APIs in eventual integration.

## Enable on A only

Set TESSERA_TRACE_SAVE_AS=1 in the environment of the exact isolated diagnostic executable and capture stderr to the attempt's evidence directory. With the variable absent, tracing is off. Output has the SAVE-LIFECYCLE prefix, monotonic uptime and sequence, capped at 2,000 events and 3,000 characters per event with an explicit limit marker. IDs and native object identities are logged, not document names, paths, pixel data or error text. No timer, polling task, file creation, timeout, forced sheet ending, or lifecycle state change is added. Synchronous bounded stderr output can affect timing; this is diagnostic evidence, not a performance measurement.

Capture the previously observed sequence: new tiny document, first Save As, Cancel, then enabled Save As again. Preserve raw trace from process start through the successor attempts and normal close. Other sequences can follow A's bounded GUI plan. B has not launched anything.

## What the trace distinguishes

- content.claim / swift.claim: request and existing capture/claim identity.
- view.make/update/refresh/didMoveToWindow/dismantle/endOwnership: actual NSView identity, prior request, final window and teardown callback presence.
- view.captureCallback / probe.capture: weak owner survival, actual hosting window, captured parent and current attachments.
- probe.leaseBegin/leaseEnd: individual UUID and full remaining lease set before/after.
- swift.bindingSet / swift.onDismissCallback / swift.dismiss / swift.contentDisappear: whether callback arrives, whether its captured ID is nil/stale, and whether swiftDismissed is latched.
- native.track/didEnd/windowLost: parent/sheet snapshots at registration and received native boundaries.
- terminal.check / join.check / presentation.complete: flags, lease set and attachment snapshots before/after each gate; no new assumptions about ordering.
- operation.begin/settle/cancel / successor.request: logical settlement versus retained presentation identity, presented item, queued successor, active prompt, and phase.

Source permits several distinct blockers: no SwiftUI callback, wrong/lost captured ID, live probe lease after dismissal, nil/wrong native parent, or unobserved attachment. The GUI failure alone does not select one. The trace records the predicates needed to distinguish them before proposing another product change.

## Preserved evidence and limits

Read main ee8b46f0 tools/orchestrate/wp/B5-16/evidence/2026-09-28-document-save-terminal-probe. Initial 88de50ea did not compile: my source referenced nonexistent NSWindow.didBeginSheetNotification. A e8ddabfa removed that observer; willBegin is not equivalent post-attachment evidence. Repaired candidate passed 43 focused tests, but actual first Cancel stalled repeated enabled Save As commands, with no file written. Neither the earlier tests nor their simulated terminal signals establish native GUI success.

Source-only diff review and git diff --check completed. No B tests, build, app, benchmark, heartbeat or writer change. A compiles/runs GUI when its lane is available. Revert cd967702 after retaining trace evidence; no product correction or new acceptance is claimed.
