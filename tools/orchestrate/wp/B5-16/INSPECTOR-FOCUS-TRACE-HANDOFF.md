# Actual-event focus trace implementation — UNRUN on B

Request7bd5595f-cd4b-4cf9-ac56-83e04e7d214f validated/accepted before source work. Separate codex/b5-16-inspector-focus-trace from qualified619524b0. Tests-first source history:80b1c709,6dc4e9e4,e9809db0,123abb9b; initial implementation6a2946b2; document-window ownership b97ef0cf; native-event test7c3f6caf then keyboard-safe metadata correction3af24b23. Failed hosted diagnostic remains on01242fa4/83172f16 and evidence; not copied into this branch. No production routing fix.

## Implementation

App/InspectorFocusTrace.swift plus narrow existing KeyRouter monitor integration. Default launch creates no collector/output or AX capture. Exactly one explicit --inspector-focus-trace <new-path> opts in. Missing/duplicate/invalid flag values disable tracing. File opens O_EXCL|O_NOFOLLOW|O_CLOEXEC,0600, no overwrite. It closes on cap, error or collector retirement. No retained responder/window/AX objects beyond synchronous capture; snapshots contain only values. No new monitor, observer, timer, actor hop or focus setter.

Capture is only document-mode keyDown Tab/ShiftTab/Space/Return/keypadEnter without Command/Control/Option, while event window equals key window AND current document viewport.window; sheet/panel/modal events excluded. Incomplete/unattached ownership produces no capture. Existing route handler executes exactly once with identical Bool; original monitor nil/event return remains. Nested events route normally but skip trace capture; output records the skipped count. Output uses sequence/timestamp/window/native identity; NSEvent.eventNumber was deliberately excluded after SDK NSEvent.h328–332 showed it is mouse-only.

Public application-focused accessibility element and firstResponder are sampled synchronously BEFORE routing the same event. Semantic objects need not be NSViews. AX window or bounded parent chain must match captured event window; conflicting/incomplete application focus cannot be upgraded by a convenient descendant. Optional fallback traverses public semantic children/navigation order, with identity/cycle detection, maximum64 unique nodes/queued nodes and depth8. Multiple candidates => ambiguous; truncation/cycle/conflict => unknown. Primary safe role/id/focus/membership also recorded when unknown. Native type is diagnostic only, never matched for routing.

Only allowlisted roles/static IDs are serialized, no AX labels/values/document names/paths/raw key characters. Native type/identity are length bounded. Maximum32 captured events and64KiB including final limit marker; no partial oversized record emitted intentionally. I/O failure closes trace without altering routing (OS partial writes can leave an incomplete last record; reject malformed evidence rather than infer acceptance). AX array materialization is framework controlled; our iteration/retention is bounded. AX getters can materialize state; disabled/enabled comparison remains required.

## Authored tests, not executed

InspectorFocusTraceTests has10 tests: default/malformed/excluded zero capture and zero factory calls; capture-before-handler/exact return/count; non-NSView semantic focus and exact window/parent membership; ambiguity/cycle/depth/node caps; conflicting primary versus fallback; arbitrary identifier/role redaction and node release;32event/64KiB closing; write failure/deinit; exclusive output no overwrite; reentrant pass-through; actual NSEvent excluded boundary. Some controls share test methods. No fake test proves actual SwiftUI focus.

B checks only source/SDK inspection and git diff --check. Compiler, all tests, apps, GPU and benchmarks UNRUN. No runtime acceptance claim.

## A-only gates and enablement

Review exact diff/hashes first; A composes this diagnostic branch with its pinned accepted source/FFI. Proposed focused command (replace scratch placeholder, preserve A's existing frozen FFI environment):

    swift test --package-path apps/mac --scratch-path <A-owned-scratch> -c release --jobs 2 --filter 'InspectorFocusTraceTests|KeyFocusTests|DocumentKeyRoutingTests|DocumentHistoryHeightControlTests|DocumentVectorVerifyFixesTests'

Run strict Release with A's established -warnings-as-errors invocation before packaging. Do not include the preserved known-failed hosted diagnostic as a passing aggregate. No command above was executed on B.

For the exact isolated signed owned candidate, first reproduce with trace disabled and record fresh AX state; quit/verify absence. Then explicitly enable on same source/profile with a NEW output path whose parent exists:

    open -n '/A-owned/Tessera Inspector Trace.app' --args --app-dir '/A-owned/isolated-profile' --inspector-focus-trace '/A-owned/evidence/focus-new.jsonl'

Use actual CUA Name -> one Tab -> observed Load focus -> one Tab; correlate trace pre-event primary semantic role/id/membership and native responder with external AX and handler result. Use ShiftTab separately. No Space/Return on Load; no chooser or user-file mutations. Output absent/empty/unknown/truncated or changed reproduction with tracing is NOT acceptance. Restore owned profile as needed; ordinary quit and verify artifact/fixture hashes. Trace is intentionally exhausted after32 captured events; new explicitly owned run/path needed for further capture, not silent reset. No global keyboard/TCC/VoiceOver changes.

A reports actual event evidence for subsequent narrow focus ownership correction. History user-reachable keyboard activation remains a separate GUI gate. A owns all runtime/main; B workload/heartbeat/writer hold unchanged.
