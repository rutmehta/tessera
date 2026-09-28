# AX collection bridge repair — source only, UNRUN

Request58e629b8-cc38-48fb-9ee3-f14f023ce3d6 validated/accepted before edits. Separate codex/b5-16-focus-ax-bridge-repair from036749da. Tests-firstf304727a; product4485d4c3ce5a65c67ddfde976816faf8f2ccce72.

## Preserved failure and narrow diagnosis

Main33b95e39 tools/orchestrate/wp/B5-16/evidence/2026-09-28/focus-trace-c5f8bca7-gui-failed contains GUI-REPORT.md, selected and full crash and raw JSONL. Read both crash forms: PID12227 main thread EXC_BREAKPOINT/SIGTRAP, _ArrayBuffer._typeCheckSlowPath -> Collection.prefix -> AppKitFocusNode.children(limit:)285 -> snapshot229 -> routeEvent174. The typed navigation-order array accessor was being iterated. This is diagnostic observer failure, not proof of second-event routing.

Samebuild disabled Name->Tab focused Load->secondTab hid panels. Enabled trace persisted only first Name event: matched AXTextField, handlerfalse. Second event/native-semantic/router result remains UNKNOWN. Replacement process12310 had no arguments; default-profile startup effects remain unknown as A recorded. No old evidence or acceptance claims are erased.42focused/strict passes did not qualify this bridge.

## Source change

Only AppKitFocusNode changes within InspectorFocusTrace.swift. Remove accessibilityChildrenInNavigationOrder entirely. Native children now use the public accessibilityChildren Objective-C selector, obtained through #selector(NSAccessibilityProtocol.accessibilityChildren), and NSObject.perform with takeUnretainedValue for the getter's +0 object result. The local result holds it during capture. No selector-name guessing, private API, KVC, method swizzle or typed Swift array/protocol-array construction.

Check returned object as NSArray. Read at most min(requestedBudget,64) objects by indexed object(at:), then conditionally test each against public NSAccessibilityProtocol before wrapping. Invalid entries are not silently ignored: batch incomplete propagates to snapshotunknown. Nil/non-array result, missing getter/non-NSObject source, zero/negative budget all return unavailable/incomplete without unsafe iteration; explicit empty NSArray is an empty complete batch. The trace never interprets an unknown result as viewport ownership.

AppKitFocusNode is internal instead of private solely to test its actual adapter with @testable import. No new production entry call or stored reference. Primary capture, membership rules,64node/8depth graph limit, scalar allowlist, file32events/64KiB/exclusive creation and teardown remain unchanged. KeyRouter is byte-identical to036749da. Existing exactly-once/result/reentrancy/lifetime tests are byte-identical.

This addresses the observed Swift protocol-array bridge trap. It is not a guarantee against arbitrary misbehaving Objective-C getters: getter materialization remains framework-controlled and no global exception interception is introduced. Actual SwiftUI retry is mandatory. Conservative unavailable children can yieldunknown more often; do not weaken that state merely to obtain a focused result.

## Authored native-boundary tests (5), all UNRUN

InspectorFocusAXBridgeTests:
- Real NSAccessibilityElement parent/child through AppKit setter/getter and actual adapter; valid semantic child need not be NSView.
- NSObject @objc public getter returns heterogeneous NSArray of semantic elements, NSNull, NSString and nonconforming NSObject; safe entries checked individually, batch incomplete. A separate navigation-order getter tracks calls and must remain at0.
- Nil, wrong return type and missing getter fail closed; explicitly empty NSArray distinction.
- Zero/negative budget invokes no getter;2 and oversized requested budgets enforce2/64 child caps and incomplete status.
- Actual heterogeneous AppKit children through snapshot retainunknown, and value-only snapshot releases native semantic objects.

These are not InspectorFocusTraceNode-only mocks. They exercise Objective-C accessor/NSArray and native AppKit semantics. All compiler/test/app execution remains UNRUN on B. B performed source/SDK/crash review and git diff --check only.

## A-only gates and retry

Compose exact source and verify companion INSPECTOR-FOCUS-AX-BRIDGE-SOURCE.sha256 (includes unchanged KeyRouter/previous tests). Proposed focused command with A's pinned FFI environment:

    swift test --package-path apps/mac --scratch-path <A-owned-scratch> -c release --jobs 2 --filter 'InspectorFocusAXBridgeTests|InspectorFocusTraceTests|KeyFocusTests|DocumentKeyRoutingTests|DocumentHistoryHeightControlTests|DocumentVectorVerifyFixesTests'

Then A's strict Release gate. Preserve original42/strict results as prior evidence, not current acceptance. On reviewed exact package, repeat disabled then enabled actual Name->Tab Load->secondTab, same isolated owned profile/fixtures, NEW exclusive trace path with --inspector-focus-trace. No Load activation/chooser or user-file mutation; no global settings. Compare actual second-event capture/handler with external AX; unknown remainsunknown, missing event remainsmissing, a failure disappearing under tracing is notfixed. Watch for process death/replacement before another CUA observation; follow A's owned runtime protocol to avoid unintentional profileless relaunch. A owns review/runtime/main. B workload/heartbeat/writer hold unchanged.
