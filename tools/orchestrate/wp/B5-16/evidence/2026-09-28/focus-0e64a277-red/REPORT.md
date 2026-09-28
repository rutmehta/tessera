# Hosted focus diagnostic: incomplete host identification

Composed only B test c39697c43645c812c72e95dc871bb6a9615e1088 onto858147a3 as A0e64a277; source bytes match B. Prior858 preserved at codex/b516-qualified-858147a3 and its qualified executable/package/evidence remain intact. No product changes.

Release command completed direct exit1 after163.008s:34 XCTest cases,33 passed,1 failed,0 skipped. Zero Swift Testing cases selected. Filter includes new focus diagnostic, six History tests, DocumentKeyRouting, KeyFocus, and DocumentVectorVerifyFixes. Full command/env retained. Existing scratch rebuilt/relinked; it is not now the old858 output. Before/after output hashes are separately retained. Four existing FFI artifacts, all source inputs, HEAD and Sony fixture stayed unchanged.

Failure: DocumentInspectorFocusRoutingTests.swift:70 XCTUnwrap cannot find a native NSView with AX identifier document.properties.colorLookup.load. The test reached actual288×848 hosted Properties and native selectNextKeyView, but fails BEFORE focused Load ownership and KeyRouter Tab/Space/Return assertions. This is diagnostic incompleteness, not a new keyboard-router reproduction or a skip. Do not infer NSButton suppression is sufficient.

Exact stdout responder evidence:

- hosted: firstResponder=KeyViewProxy ObjectIdentifier(0x0000000c3a57aa00); fullKeyboardAccess=true.
- after-name-Tab: firstResponder=_SystemTextFieldFieldEditor ObjectIdentifier(0x0000000c3b7c6c00); fullKeyboardAccess=true.
- Native tree includes two KeyViewProxy instances, real AppKitTextField and Checkbox/FocusRingNSButton; all printed AX identifiers are empty, and all printed AXfocused values false. No Load LUT native host is identified. Class names are diagnostic evidence only, not proposed product-routing predicates.

Raw stdout/native AX tree preserved in run.log and extracted responder-native-AX.txt. Test source also attaches captures with keepAlways, but this report does not claim a separately exported XCTest attachment bundle. The process did not establish the earlier real GUI AX-focused Load path. No global keyboard policy changed, no GUI app launched, and no broader rerun occurred.

Compile warnings retained: new test line26 redundant nil-coalescing of a nonoptional String; existing weak-variable mutability warnings in three tests and non-Sendable self capture in EngineDocumentBackendTests. Compilation itself succeeded; no strict-warning acceptance claimed.

Next bounded B diagnostic decision: correlate the actual SwiftUI accessible Load element/semantic focus with responder using public APIs without requiring that element to be a native NSView, and establish genuine navigation before evaluating interception. Do not force button focus, match private class names in product, or suppress all hosting responders. Hosted-window focus policy/transaction behavior remains a diagnostic variable; the existing visible GUI failure remains independently preserved.

Runtime released to root after process completion. Anchored test executable pgrep returned1/no matches. No correction, extra gate or GUI launch is authorized by this report.
