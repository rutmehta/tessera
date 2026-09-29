# Color Lookup native keyboard ownership — source-only candidate

Request5b27b9df-43bd-45d8-8ee4-5cf79bba101b validated/accepted. Separate codex/b5-16-lut-native-key-owner from a6e28f4e. Initial tests55d583a1/1f658c25/7caa4e54 precede product71a6e3c9; focus maskf3f0966a; test refinementsbd6c194b/31c18bb1/45a9860f. All tests UNRUN on B. A owns compiler/runtime/main.

## Evidence and bounded correction

Read d18369b3 GUI-REPORT.md, ROOT-REVIEW.md and raw focus-01.jsonl for ax-lifetime-4685efd5-gui. Disabled and enabled observed Name -> Tab external AX Load focus -> secondTab panel hiding. Second event native SwiftUI.KeyViewProxy, matched event/current document window, handledtrue, but INTERNAL semantic statusunknown/incomplete and membershipunproven. This evidence remains unchanged and is not semantic proof or permission to exempt a private class.

Existing native controls implement KeyOwningControl; KeyRouter protects such an actual first responder before document Tab/Space dispatch. Source-supported correction gives ONLY Color Lookup Load and Reset actions an explicit NSButton subclass conforming to that contract. No KeyRouter/trace changes, no AX-derived shortcut decision, private class matching, unknown-focus bypass, focus setter, nextKeyView override, monitor, global preference or timer. The remaining ExtendedAdjustmentEditor button helper and other controls are untouched.

New DocumentInspectorActionButton NSViewRepresentable updates title/label/identifier/help/enabled/action on reuse; inherits SwiftUI disabled environment including Reset.disabled(identity), and clears action on dismantle. Native momentary button target/action retains AX press and native tracking. acceptsFirstResponder respects enabled AND super's native policy. Tab/ShiftTab go to super; Space/Return/keypadEnter activate once for nonrepeat events without Command/Control/Option, matching existing History control convention. No default/global key equivalent. Native focus acquisition still requires actual A qualification; source conformance alone does not prove SwiftUI hosting will place focus on this native responder.

Appearance uses the same bordered-small ThemeButtonBody tokens: caption font, small height, horizontal Space.s, control radius, raised/hover/pressed fills, strong hairline border, primary text and disabled opacity. Intrinsic width follows text+padding and can compress/truncate to proposed width. Native focus-ring mask uses opaque theme ink; no ad hoc palette/font or lint waiver. This aims to preserve existing style, not a pixel-equivalence claim; actual light/dark/layout/hover/pressed/focus comparison remains mandatory.

## Six authored tests, all UNRUN

- Explicit native KeyOwningControl contract and unchanged label/id/help/font/height/key-equivalent metadata.
- Direct native keyDown Space/Return/keypad action once, repeats suppressed; fixture closure only, no chooser.
- Action reuse replaces stale callback; disabled and dismantled actions cannot invoke.
- Activation key/modifier partition leaves Tab/ShiftTab/other keys to native path.
- Action context release after dismantle with explicit pool boundaries while button remains alive.
- Actual PropertiesPanel Color Lookup host contains exactly Load and Reset native adapters with correct IDs and identity Reset disabled. Explicit288x848 host geometry, no DocumentView shared-owner attachment, activation policy restored. One Task.yield for initial view transaction; no timed sleep. This is installation/disabled-state coverage ONLY, not fake focus proof.

Existing viewport Tab/Space, text/modal guards and slider/layers tool-letter tests remain unchanged. Existing diagnostic and all previous failures preserved. Only the two LUT actions are in scope: no claim that Dither, every other SwiftUI inspector control or History traversal is fixed.

## A-only qualification

Review exact diff/hashes, compose pinned accepted source and FFI, then proposed focused command in A-owned scratch:

    swift test --package-path apps/mac --scratch-path <A-owned-scratch> -c release --jobs 2 --filter 'DocumentInspectorActionButtonTests|InspectorFocusAXBridgeTests|InspectorFocusTraceTests|DocumentHistoryHeightControlTests|DocumentKeyRoutingTests|KeyFocusTests|DocumentVectorVerifyFixesTests|DocumentInspectorLayoutTests|ThemeLintTests'

Run strict/full as A requires only after focused success. B executed none of these; only source review and git diff --check.

A actual GUI with owned copied document/profile:
1. Trace disabled, disclosed unchanged keyboard policy: Name -> oneTab -> Load -> nextTab must advance focus with panels visible; ShiftTab must reverse. Observe actual focused responder/AX; do not force focus. Repeat with Reset enabled and identity/disabled, confirming disabled Reset skipped by native policy.
2. Trace enabled with fresh exclusive path after ordinary quit/liveness checks: capture actual native first responder and handledfalse at these control events. Unknown internal AX remains unknown; explicit native KeyOwningControl responder can now provide independent ownership evidence. A must verify real event path, not infer from class name alone.
3. Verify Space/Return/keypad once on owned safe Reset scenario with Undo restoration. Load activation opens a chooser; only within A's explicit GUI scope, verify chooser opens then cancel without selecting/modifying files. Otherwise mark Load activation pending, not passed.
4. Verify visible keyboard focus, AX labels/help/IDs/enabled state and exact layout/hover/press/disabled appearance in dark/light and compact288-column bounds. Retain footer/scroll budget checks.
5. Viewport plainTab still toggles panels, Space pans; name field retains typing, tool-letter-over-slider/layers remains; inspect subsequent Dither/other controls as separate findings rather than claiming this bounded slice covers them. History user-path keyboard reachability remains separate.

No actual GUI acceptance claimed; A alone reviews/runs/merges. B compiler/apps/GPU/heartbeat/writer hold remains.
