# Dither native ownership source handoff

Request936e2c6e-31a1-4ea2-8f22-a5bbdc4348a9 validated/accepted before work. Separate branch `codex/b5-16-dither-native-key-owner` from frozen527410c7; earlier branches unchanged. Tests-first `4b240cfc` plus test isolation/import correction `ad0755ff`; product `a80f4c53c7b65d8d31d4d7ab96d27edd8fcb1bc2`. Six new tests ALL UNRUN on B. Source diff/check and hashes only; A owns compiler/runtime/main.

## Observed trigger and bounded correction

Main1d7023c5 preserves actual Dither-origin Tab hiding panels after Name→Load→Reset→Dither on d0191c41. External AX focused checkbox, but internal trace remained unknown/incomplete/unproven; handled=true. That unknown attribution is not relabeled. Shift-Tab, Space and History were unrun in the failed session. Earlier bounded LUT GUI and full-suite evidence remain separate.

Only Color Lookup's Dither view changes. `DocumentDitherCheckbox` uses a synchronous SwiftUI Binding with the same model setter, preserving LUT size/data/sourceFilename and final=true commit. Common checkbox helper and every other control are unchanged. Native NSButton switch supplies checkbox role/value, label/ID/help, standard drawing/focus/disabled state with caption theme font and small native control size. No custom checkbox painting, global policy or new key equivalent. The existing explicit KeyOwningControl marker protects actual responder ownership. Native policy determines acceptsFirstResponder; no forced focus or nextKeyView wiring.

Space performs one native click/toggle; key repeats suppressed. Tab/Shift-Tab, Return/keypad Enter and all other keys delegate to native checkbox behavior. Existing tool-letter routing unchanged. Updates refresh state/enabled/callback without publishing; dismantle clears callback. No timers/observers/global registry. No router or trace changes.

## Authored tests and limits

DocumentDitherCheckboxTests (six): native role/value/metadata/focus-policy comparison; synchronous Space toggles and repeat suppression; latest callback, no update publication, disabled and teardown behavior; activation-key partition excludes Return and Tab; explicit autoreleasepool callback lifetime; actual Properties installation and Dither edit preserving LUT fields, JSON round-trip and one Undo using explicit stub backend.

The hosting test does not force focus or prove traversal. JSON round-trip is not file save/reopen. Direct keyDown tests do not prove app-level pan suppression. Tests are uncompiled/unrun here; A must establish results, including native AX bridge assertions and hosted fixture behavior. No passing claim or RED result inferred from authored tests.

## A validation sequence

1. Review/cherry-pick bounded source/test commits onto current integration; run new six tests plus prior native-action, focus bridge/trace, key routing, History and theme/layout tests, strict and required full suite. Baseline installation assertion must distinguish old SwiftUI Dither from the native adapter; retain actual failed GUI evidence independently.
2. On copied isolated LUT document, record Name→Load→Reset→Dither then Dither-origin Tab and reverse Shift-Tab with trace disabled and enabled as appropriate. Verify panels remain visible and actual native checkbox responder; do not assume a specific next element without inspecting native order. Repeat disabled-Reset path.
3. Space from focused Dither toggles once with one edit; repeated held Space does not repeatedly toggle; no document pan. Undo restores original Dither and filename/table; redo then save/reopen copied document verifies persistence. Preserve original fixtures and user photos.
4. Return/keypad Enter retain native checkbox convention; do not impose action-button activation. Verify native AX press/value, enabled/disabled behavior, real focus ring and checkbox label in light/dark/compact width. No global preferences changed. Native style fidelity remains an actual GUI gate.
5. Viewport Tab still toggles panels; text retains input; non-text marked tool-letter behavior and open chooser/menu handling preserved. Recheck accepted LUT actions and distinguish remaining History/other controls from this slice.

Hashes are in DITHER-NATIVE-KEY-OWNER-SOURCE.sha256; KeyRouter and InspectorFocusTrace hashes match frozen527410c7. Product limited to new adapter plus five-line Dither call-site diff.
