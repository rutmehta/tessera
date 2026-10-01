# B5-42 — Document accessibility audit

Document controls now expose `document.<area>.<control>` identifiers and accessible names. The audit traverses hosted SwiftUI/AppKit accessibility children, reports each offender's scenario and full AX path, and rejects missing names, malformed IDs, duplicate sibling IDs, and empty trees. `AXTitle` is accepted as the accessible name of standard buttons; values, help, and placeholders are not accepted as names.

## Changes

- Added explicit metadata to inspector controls, tool options, toolbar items, Layers row actions, Channels, History, text/shape properties, and transform options.
- Shared segmented controls accept an optional document prefix and expose separate identifiers and names for each segment. Document callers supply the prefix.
- Native cell-backed controls keep their native roles and actions, with metadata installed on the owning control. The hosted audit reads owner metadata for native cells because direct legacy cell getters omit it. Layers vends its existing realized row views with row metadata/native children and forwards AX selection/disclosure to the native outline operations. A regression test checks selection and expand/collapse. Explicit reachability checks require the layer visibility, mask-link, smart-filter blending and effect-visibility controls.
- The document sidebar toolbar item has an explicit ID and retains the native `NSSplitViewController.toggleSidebar` responder-chain action and sidebar symbol. SwiftUI's automatic item offers no supported identifier modifier. Library modes retain the automatic item. The removal modifier is on the sidebar column, as shown in [Apple’s toolbar API documentation](https://developer.apple.com/documentation/swiftui/view/toolbar(removing:)).
- Decorative menu chevrons are excluded from AX; containment prevents parent IDs from overwriting child IDs.

## Coverage and boundaries

The tests use a disposable 64×64 engine document and `LayoutProbeHarness`, including the real shell toolbar. They cover every tool enum case, every adjustment and fill kind, all inspector tabs, a text editing session, shape properties, free transform and editable advanced transforms, Layers with masks/smart filters/effects, Channels rename, and History snapshots. No window becomes key/front. Test panel preferences are restored at teardown.

This is an in-process, background hosted AX audit, not a VoiceOver or external AX-client acceptance run. Pop-up launchers are audited; transient system menus, color panels and sheets are not exhaustively opened. AppKit scrollbar internals are excluded. Native toolbar wrappers may share an identifier with their descendant representing the same control; duplicate sibling IDs fail. No GUI launch, desktop capture, real Lightroom catalog access, or installed-app validation was performed.

## Validation

- Focused release audit: **5 tests, 0 failures**, **70 hosted scenarios**, **1,199 interactive control observations**, **373 distinct identifiers**. [GREEN.txt](GREEN.txt) records per-scenario counts.
- Required serial `build-ffi.sh` then `swift-gate.sh`: **SWIFT GATE OK**. **896 XCTest tests, 3 skipped, 0 failures** in 205.701 seconds; **5 Swift Testing tests in 2 suites passed**. The debug build also completed successfully. No locked-screen capture exception was needed.
- Environment: `PATH="$HOME/.cargo/bin:$PATH"`, `CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-42"`.
- `git diff --check`: clean.

## Commits

- RED: `daa310c8` — `test(B5-42): audit hosted document controls for AX identifiers and labels`. The committed [RED.txt](RED.txt) records 512 failures across 69 scenarios and 1,208 interactive observations (4 tests).
- FIX: `c5b3ab56` — `fix(B5-42): expose document control identifiers and accessible names`.
- DOCS: this commit.

All commits are local on `wp/B5-42` with the requested co-author trailer. No Rust changes; Rust crate gates are not applicable. `Cargo.lock` and `board.json` are unchanged.

## Identifier map

See [IDENTIFIERS.md](IDENTIFIERS.md) for the observed control/name map and dynamic identifier conventions. To refresh the observed map:

```sh
cd apps/mac
TESSERA_AX_MAP=1 swift test -c release -Xswiftc -enable-testing --filter DocumentAccessibilityTests
```
