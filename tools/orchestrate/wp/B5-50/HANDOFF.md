# B5-50 — Library and Develop accessibility audit

Branch: `wp/B5-50`. Local commits only.

## Result

Library and Develop controls now expose stable `library.<area>.<control>` /
`develop.<area>.<control>` identifiers and accessible names. The window-rooted audit
checks interactive roles and actual modern press overrides regardless of role, reports
the scenario and full offending AX path, rejects duplicate sibling identifiers, and
requires key controls and report groups to be reachable.

The committed [RED.txt](RED.txt) recorded **468 failures** across 28 audit scenarios and
557 interactive observations. Of those, **446 metadata/duplicate checks and 9 reachability
checks** represented defects addressed by the changes. The remaining **13** were inherited
AppKit compatibility press methods on splitters, slider thumbs and collection section
proxies. The final audit excludes those inherited bridge implementations, just as it
excludes inherited NSView/NSCell defaults; real overrides on nonstandard roles remain
covered by regression fixtures. It does not invoke deprecated action-name APIs.

The final focused run passes **4 tests**, including **35 audited hosted scenarios** and
**566 interactive control observations**. The map contains **325 distinct production
identifiers**: **98 Library** and **227 Develop**, excluding the two test-only press
fixtures. See [IDENTIFIERS.md](IDENTIFIERS.md) for the observed names and dynamic conventions.

## Changes

- Named Library shell/grid controls, workspace actions, filter/search controls, selection
  chips, inspector panels, editable metadata/keywords, sidebar rows and thumbnail cells.
- Named Develop panel disclosures, Basic/tone/HSL/colour-grading/detail/effects controls,
  Transform/Upright, Lens Blur, crop, HDR/proofing, presets, snapshots, history, masks and
  the loupe/mask toolbars. Native slider owners receive names immediately, including when
  their initial value is zero. Segments have individual IDs; mask/component/history and
  saved-record IDs use model keys or documented positional keys.
- Applied Library identifiers to import actions, mapping/fidelity controls and report
  content, including **Not fully supported** and **Approximate translations**. Existing
  B5-36 content-value checks remain intact under the Library namespace. Its hosted AX
  activation now uses the same modern-selector bridge as B5-42.
- Sidebar native row proxies are matched through `accessibilityRows()` and value identity,
  then represented by their realized row views. Native selection and disclosure operations
  are forwarded and regression-tested. The sidebar toggle uses the existing document
  toolbar pattern and unchanged native responder-chain action; Document AX identifiers
  and its existing toolbar item identity are preserved.
- Extracted the existing four report-publication assignments into `publishReport`, called
  by normal import completion, so the real sheet can be hosted with synthetic completed
  and cancelled reports. Import navigation/callback behavior is unchanged.

## Hosted coverage and boundaries

The audit walks **from each NSWindow**, including the real Library/Develop shell toolbar.
It covers an empty library; a generated JPEG library; every expanded Develop panel;
all HSL properties and colour-grading ranges; point/parametric curves; active crop; a
selected two-component mask and brush toolbar; a saved preset and history/snapshot;
all five import steps with synthetic folder/mark/fidelity data; and the actual import
sheet with completed and cancelled synthetic reports. Separate assertions verify native
sidebar row selection and album-group disclosure.

`LayoutProbeHarness` prohibits activation, and every hosted scenario asserts that its
window is non-key and the application inactive. No app GUI was launched in the foreground,
no desktop capture was performed for this audit, and no real user catalog was opened.
Generated photo/import data and preset storage are disposable; the preset store path is
verified before writing. Panel preferences and enhanced AX activation are restored.

This is an in-process hosted AX audit, not external VoiceOver acceptance. Standard macOS
window chrome and scrollbar internals are outside the app identifier namespace. Menu
launchers and the specified window scenarios are audited; transient system menus, panels
and popovers are not exhaustively opened. Repeated observations of the same control across
scenarios are not distinct source defects or distinct identifiers. Accessible names accept
AXLabel or the standard AXTitle, never values/help/placeholders alone.

## Verification

See [GATES.txt](GATES.txt) and [GREEN.txt](GREEN.txt).

- Required serial `build-ffi.sh` → `swift-gate.sh`: **SWIFT GATE OK**, exit 0.
  **923 XCTest tests**, 3 skipped, **0 failures**; **5 Swift Testing tests** in 2 suites passed.
- Required strict release product build: **PASS**, exit 0, **0 warnings / 0 errors**.
- Combined Library/Develop, B5-36 import, B5-42 Document and B5-44 shortcut run:
  **17 tests, 0 failures**. Document: 7; Library/Develop: 4; import: 3; shortcuts: 3.
- Standalone shortcut source audit: **SHORTCUT AUDIT OK**.
- `git diff --check`: clean. Added AX calls are non-deprecated.
- `PATH="$HOME/.cargo/bin:$PATH"`; `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-50`.
  Existing LibRaw vendor warnings occurred in the FFI build; the strict Swift build was clean.

No Rust source, `Cargo.lock`, or `board.json` changes. Nothing was pushed or installed.

## Commits

1. `f60355e6` — `test(B5-50): audit hosted Library and Develop accessibility` (RED).
2. `e068ea9d` — `fix(B5-50): name Library and Develop accessibility controls`.
3. This documentation commit — identifier map, green evidence and verification handoff.

All three commits end with the requested Claude Opus 5.5 co-author trailer.
