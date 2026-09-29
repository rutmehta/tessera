# Remaining inspector keyboard ownership — source review

Request `d4a3357f-2636-4a8a-8837-04c47ddb3a14`, accepted after exact B target/expiry validation. Frozen source `527410c7`; A evidence `28da5782`. This extends INSPECTOR-KEYBOARD-FOLLOWUP-AUDIT.md with an explicit inventory. No product/router/trace/test changes. B compiler/tests/apps/GPU UNRUN; A owns runtime and integration.

## Inventory and key behavior

Paths below are under `apps/mac/Sources/Tessera/`. “Owned” means the actual first responder implements `KeyOwningControl`, not that every child or semantic descendant inherits that protection.

| Controls / source | Explicit ownership | Local key behavior / acceptance contract |
| --- | --- | --- |
| Load LUT / Reset: Document/DocumentInspectorActionButton.swift | Native NSButton + marker | Tab/Shift-Tab delegate to native traversal. Space/Return/keypad Enter activate once for non-repeat; Cmd/Ctrl/Option excluded. Disabled Reset skipped. A bounded GUI now passed; not a whole-inspector result. |
| History − / + / reset: Document/DocumentHistoryHeightControl.swift:105 | Native NSButton + marker | Same activation keys; Tab/Shift-Tab native. Actions edit shared persisted requested height and clamp via budget. Reachability, clamp skipping, focus after collapse/disable still need actual traversal. |
| Document sliders: Document/DocumentControls.swift:7 → Inspector/ValueSlider.swift:83 | Native NSControl + marker | Arrows adjust, Shift ×10 / Option ×0.1; Home/End bounds; Return/keypad Enter/Escape commit and relinquish focus. Other keys native. No assumption that Escape rolls back. |
| Curve editor: Inspector/CurveEditorView.swift:342 | Native NSView + marker | Enabled point mode: arrows nudge, Shift larger; Delete variants remove selected knot; Tab cycles knots; Escape clears selection/releases focus. Source handles Shift-Tab through the same next-knot case, not reverse traversal. Review this local contract separately; do not apply button traversal assertions to it. |
| Layers list: Document/LayersOutline.swift:558 | Native NSOutlineView + marker | Unmodified Delete/backspace or forward Delete removes selected layers; other keys delegated to outline. Text editing must remain text-owned. Child eye/chain NSButtons are not themselves marked; list ownership does not prove child-first-responder protection. |
| Type tool: Document/Text/TextInputView.swift:13; native text fields/editors | Marker for TextInputView; router recognizes NSText/NSTextField | Text/IME and editing keys belong to text; tool letters must not switch tools during typing. This is a negative control against broad ownership changes. |
| Dither and common checkboxes: Document/AdjustmentEditors.swift:58, Colorize/Monochrome: Document/PropertiesPanel.swift:213/251 | SwiftUI Toggle; actual native/semantic responder unresolved | Expected control interaction: Tab/Shift-Tab traverse without panel toggle; Space toggles once and commits correct parameter; no viewport pan. Do not impose Return-as-toggle without confirming platform behavior. Preserve synchronous actor-inherited setter. |
| Popups and actions: AdjustmentEditors.swift:78/87; PropertiesPanel.swift:188 | SwiftUI Menu/Button; unresolved responder | Menu owns navigation/selection/dismissal; action button owns activation and traversal. Check menu-open arrows/Return/Escape and closed traversal separately; no global document action should replace focused control intent. |
| Inspector tabs/group mode/curve channels/mixer/gradient segments: App/Components.swift:190 and PropertiesPanel.swift | SwiftUI Buttons in SegmentedPicker, not native NSSegmentedControl | Reachability and activation select the intended segment; keyboard behavior cannot be inferred from native segmented-control conventions. Observe focus settlement on subtree replacement. |
| History collapse header and other inspector icon/gradient actions: Document/DocumentView.swift:247, AdjustmentEditors.swift | SwiftUI Buttons; unresolved responder | Activate intended action and traverse normally. Collapse must not leave focus/routing stranded on removed children. |
| Fill/gradient color wells: Document/DocumentControls.swift:47 | Native NSColorWell, NO marker | Native implementation alone is not router ownership. Check focused closed well and color-panel input separately; panel protection is a separate router guard. No demonstrated failure. |

`App/KeyRouter.swift:63` protects explicit marker/text first responders and panel/sheet/modal windows. It does not establish arbitrary ancestor or semantic ownership. Its tool-letter route deliberately runs over non-text marked controls first: preserve this existing behavior for native LUT/History/sliders/list, while text remains excluded. Modifier and menu shortcuts need their own routing path; the marker is not an all-key suppression policy.

## Highest-value A GUI sequence

Use the current isolated fixture/exact candidate and existing keyboard policy; no global setting changes or forced focus. Capture original PID, external focused element, panel state and (if enabled) same-event responder/handler result. Compare disabled/enabled trace for a reproduction, keeping UNKNOWN semantics unknown.

1. Identity LUT: Name → Tab Load → Tab Dither (disabled Reset skipped, already observed). **Now originate Tab from Dither**, then Shift-Tab back; inspect destination and verify panels remain visible. Separately Space on Dither: one visible parameter/history change, no pan; Undo restores it. Record Return behavior without presuming checkbox activation. This is an unresolved boundary, not a proven failure.
2. History: reach controls through real traversal, activate decrease/increase/reset and verify value/height/history preference; walk both directions at a disabled clamp/default reset. Collapse after child focus and confirm a valid remaining destination. Distinguish pointer/AX-action success from traversal success.
3. Representative nearby controls: one adjustment popup (open, arrows, Return selection, Escape dismissal), one ordinary Analyze Again/Reset button, one tab/group selector; verify Tab and Space/Return where appropriate. Avoid converting the entire control family based on one example.
4. Native-unmarked controls: a closed color well and a Layers child action, if reachable through normal traversal. Record actual first responder rather than assuming the parent marker protects them.
5. Negative controls: viewport Tab still toggles panels; text keeps tool letters; non-text marked controls retain documented tool-letter selection; slider Return/Escape settlement and curve knot Tab retain their distinct contracts. An owned chooser must keep input.

## Proposal boundary

No further implementation is justified by the current evidence. If Dither-origin Tab/Space is intercepted, first preserve the exact event and intended control identity. A narrow candidate for subsequent review would be a Dither-only native checkbox using the existing explicit marker, preserving binding/AX label/identifier/disabled behavior and callback isolation. Do not implement or generalize it until the reproduction identifies the boundary. No unknown-focus bypass, private proxy class exemption, global native conversion or trace/router rewrite is proposed.

## Evidence reconciliation

`28da5782` records d0191c41: disabled/enabled Name→Load→Reset and reverse passed; Reset Space/Return/keypad Enter had observable edit/Undo behavior; Load Return chooser/Escape cancellation passed; disabled Reset traversal reached Dither; viewport Tab and compact/light/dark checks passed. Exact callback counts are separate unit evidence. Dither-origin behavior, full History/inspector, held repeats and full VoiceOver remain unaccepted. Source-only findings above do not enlarge that scope.
