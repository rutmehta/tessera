# Preserved RED and bounded diagnostics

Frozen HEAD1d361fa383d7d8b0fb291fbfbfbd97a19c07ae54. Gate01 direct exit1:65 tests,22 assertion failures,179.01s. All source/head/FFI/fixture freezes true. No shared edits and no downstream gates attempted.

Four of five new History tests fail (14 assertions); direct keyboard activation test passes. Eight assertions in later DocumentKeyRoutingTests fail. Four test compile warnings flag redundant String?→String casts at21,24,45,126.

`diagnostic-keyboard-alone-01` executes the existing binary with Swift --skip-build and only DocumentKeyRoutingTests|KeyFocusTests.15 pass,0 failures,exit0; entire before/after snapshot including outputs equal. This proves an order-dependent combined-run failure rather than an unconditional keyboard-source regression. Source points to the new hosted inspector test: DocumentView.onAppear attaches DocumentTools.shared.workspace, whereas new teardown removes the hosting content without restoring that singleton. KeyRouter.handleDocument consults DocumentTools.shared before local key handling, allowing retained old workspace state to consume keys/change the wrong document. Exact lifetime mechanism remains a source diagnosis, not instrumented proof. B should isolate/restore singleton ownership in its test and qualify combined order.

`native-ax-probe-02.swift` is a standalone minimal plain NSButton/NSTextField AppKit comparison, prohibited activation, with no ordered/visible window and no app package launch. It reproduces both unattached and hosted:

- plain native NSButton.accessibilityRole() = AXUnknown;
- accessibilityPerformPress() = false while target callback count increments exactly once;
- NSTextField.accessibilityValue() = "168 pt" despite setAccessibilityValue("168 points").

Therefore direct role/press-return assertions in this prohibited harness cannot distinguish the new subclass from ordinary native AppKit behavior. Do not label them a proven missing external AX action, and do not solve merely by assigning synthetic passing roles. Retain real callback/state/persistence assertions for every press and qualify actual external AX button/action/current-value with the isolated GUI afterward. If contract requires literal spoken "points", use a supported readout implementation/override with a numerical value check, rather than expecting NSTextField's setter to supersede its text getter. Test/source expectations should explicitly agree. Native button value semantics also need actual GUI observation. Removing redundant casts is mechanical.

Probe01 failed to compile because the standalone script initially lacked a MainActor scope; both its source/log/exit and corrected probe02 source/log/exit are retained. No product-source or XCTest binary change occurred. This diagnostic is not GUI accessibility or VoiceOver qualification.

Remaining compiler/runtime lane work paused pending root-coordinated B source corrections. Prior GUI checkpoint/isolation exception remains unchanged. No cleanup of unknown files, user apps or profiles.
