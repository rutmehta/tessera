# Inspector keyboard follow-up audit

Source-only review authorized by Rut's “good, continue.” Reviewed frozen candidate `527410c7` and A status/evidence publication `c7127247`. No product or test changes; all B runtime checks UNRUN. A retains compiler, GUI and main integration. This review does not establish a new runtime defect or acceptance of the active LUT candidate.

## Confirmed source coverage and remaining gates

| Area | Source fact | Evidence still needed on A |
| --- | --- | --- |
| Color Lookup Load/Reset | `Document/AdjustmentEditors.swift` uses `DocumentInspectorActionButton` for these two actions only. Native `DocumentInspectorNativeActionButton` conforms to `KeyOwningControl`. | Real Name → Tab → Load → Tab and reverse traversal; identity Reset disabled skipping; enabled Reset and Load chooser activation exactly once. Compare diagnostic disabled/enabled. |
| Dither and other adjustment controls | Checkbox helper at `AdjustmentEditors.swift:58` remains SwiftUI Toggle; menus at78 and ordinary button helper at87 remain SwiftUI. Auto/Equalize/HDR Analyze Again, Black & White defaults, gradient actions are outside the native adapter. | After Load traversal passes, inspect the next Dither/menu/action focus boundaries. Do not extrapolate the two-button fix to all inspector controls. |
| Inspector tabs | `App/Components.swift:190` SegmentedPicker is a group of ordinary SwiftUI Buttons. `DocumentView.swift` swaps tab subtrees. | Keyboard reachability and focus settlement when tab/selected editor changes. Keep normal viewport Tab panel shortcut as a negative control. |
| History | `DocumentHistoryHeightControl.swift` has three native `KeyOwningControl` buttons. `HistoryHeightButton.acceptsFirstResponder` returns `isEnabled`; LUT adapter also consults `super.acceptsFirstResponder`. | Native traversal into/out of the group, disabled skipping at clamps, collapse while a child owns focus, key activation and persisted height. Policy difference warrants observation, not automatic code normalization. |
| Hidden tab shortcuts | `DocumentView.swift` shortcut buttons have zero frame, zero opacity, hit testing disabled and AX hidden. | Inspect actual traversal if unexpected focus stops occur. These source modifiers alone are not proof of key-loop exclusion or a proven interception defect. |

## Router constraints

`App/KeyRouter.swift:63` treats text responders and explicit `KeyOwningControl` responders as busy, alongside sheet/panel/modal boundaries. Ordinary SwiftUI action ownership is not established by this guard. The previously observed private proxy with UNKNOWN/incomplete semantics must stay unknown; no generic private-class exemption or broad unknown-focus bypass follows from this audit.

`handleToolLetterOverKeyOwner` intentionally handles tool letters over non-text key-owning controls before the busy guard. Adding the native action buttons therefore participates in the existing tool-letter policy. The A acceptance check should verify a tool letter over Load/Reset still selects the intended tool, while text entry keeps letters. Do not broaden button ownership into suppression of all document keyboard behavior.

## Native adapter lifetime and layout review

At `527410c7`, configuration replaces the action closure, identifier, label, help and enabled state on every update; dismantle clears the callback. The adapter adds no global registry, observer or timer. Its single tracking area is replaced in `updateTrackingAreas`. This source inspection found no concrete lifetime blocker; it is not leak or teardown runtime acceptance.

Space, Return and keypad Enter are explicitly handled once for non-repeat events; Tab/Shift-Tab go to native behavior. Tests invoking `keyDown` directly establish action dispatch, not the actual host's key-view path. The hosting test establishes native installation/disabled metadata only. Native `super.acceptsFirstResponder`, custom focus drawing, disabled appearance, truncation and 20-point height require actual UI checks at compact width. Focused control removal/disable deserves a real event check because `acceptsFirstResponder` does not itself prove where current focus settles.

## Bounded next validation sequence

1. Finish A's existing exact candidate GUI gate without modifying it. Record actual responder plus external focused element, handler result and panel state for both Tab directions. Preserve earlier failed diagnostic/GUI evidence.
2. On the same isolated fixture, check adjacent Dither and one menu/action control. A successful Load transition is insufficient evidence for them. If a failure appears, capture the exact event before proposing a scoped change.
3. Check History decrease/increase/reset from genuine keyboard traversal (no forced `makeFirstResponder`). Verify each clamp, default request, compact footer budget, collapse/removal and reopen persistence. Existing direct action and forced-focus tests are not substitutes.
4. Negative controls: viewport Tab still toggles panels; text accepts letters; tool letters over native non-text controls preserve documented behavior; an open chooser owns input.

Avoid expanding to broad UI redesign or a global focus bypass. Remaining work is bounded acceptance and evidence-driven follow-up, not an assertion that this source review has fixed the other controls.

## Reconciled evidence

A's `c7127247` publication records `d0191c41` portable automated evidence with 54 focused and three theme/layout tests plus strict checks, independently verified on A. Actual GUI was still A-owned/in progress at this snapshot. B ran no compiler, test, application or benchmark. Frozen product branch remains `codex/b5-16-lut-native-key-owner` at `527410c7`.
