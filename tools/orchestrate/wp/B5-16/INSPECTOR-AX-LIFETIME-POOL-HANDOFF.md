# Native AX explicit pool lifetime control — UNRUN

Request8102f329-cc08-49ee-a897-6d2400fed0a2 validated/accepted. Separate codex/b5-16-ax-lifetime-pool from frozenfd2b7d5f. Test-only commit832d095bbd14680d0f3e932031bc39a2214941c6. No trace/routing/product changes.

Read origin/main docs/coordination/INSPECTOR-AX-LIFETIME-DIAGNOSIS.md and preserved cc482a66 evidence under evidence/2026-09-29/ax-bridge-114273ea-red. Exact114273ea ran47 cases:46 passed, one case failed both weaknil assertions; snapshotunknown/incomplete assertions passed. Strict/GUI were unrun. This proves neither an observer leak nor harmless autorelease. All previous failures including c5f8 GUI crash remain preserved.

Updated observed test uses synchronous autoreleasepool around all root/child/native array construction and snapshot capture. The value-only Snapshot is returned into an outer let, never cleared. Both original post-boundary weaknil assertions and unknown/incomplete requirements remain. withExtendedLifetime((window,snapshot)) explicitly keeps both alive during post-drain checks; the outer window's weak connection to child remains unchanged.

New same-native-graph/no-observer control repeats exact root/child/properties/heterogeneous copied children array inside a separate autoreleasepool. It omits only AppKitFocusNode wrapping and snapshot capture, keeps outer window alive, and asserts both weak references nil after drain. No manual child/parent/window clearing, sleeps, event-loop pumping, timing allowance or changed product ownership.

Interpret A outcomes conservatively:
- Both release after drain: no observer retention survives that declared boundary; exact prior internal owner still unproven.
- Control releases but capture does not: investigate capture-specific retention.
- Both remain: native setup/AppKit ownership is not isolated.
Unknown/incomplete snapshot assertions remain required in every observed capture case. No result predicted or asserted here.

All tests UNRUN on B. Source review/git diff --check only. Companion manifest pins test and unchanged product hashes. A proposed gate: same original focused set plus new control (expected48 cases if previous47-case filter unchanged), then strict only if passing, then independently reviewed actual isolated disabled/enabled Tab trace retry. Example A-only command with pinned FFI and scratch:

    swift test --package-path apps/mac --scratch-path <A-owned-scratch> -c release --jobs 2 --filter 'InspectorFocusAXBridgeTests|InspectorFocusTraceTests|KeyFocusTests|DocumentKeyRoutingTests|DocumentHistoryHeightControlTests|DocumentVectorVerifyFixesTests'

Native lifetime success cannot qualify actual SwiftUI focus or routing. A owns all compiler/runtime/GUI/main. B workload/heartbeat/writer hold unchanged.
