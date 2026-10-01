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

## B5-42b — Strict release compatibility (2026-10-01)

Fix commit: `46262454` (`fix(B5-42b): replace deprecated document accessibility APIs`), on top of `3f71ddc4`; no rebase.

- Replaced the deprecated role/index attribute queries in `LayersOutlineView` with modern AX methods. AppKit's `NSOutlineRow` proxies do not expose modern role/index getters or conform to `NSAccessibilityRow`, and each query creates new proxies. The modern `accessibilityRows()` collection is therefore bridged as `NSArray` and matched by value equality to recover native row order. Existing realized row views, metadata, selection/disclosure actions, fallback behavior, and non-row children are retained.
- Audited the complete `git diff origin/main...HEAD -- apps/mac`, including tests. The hosted audit now uses modern role/identifier/label/title/children getters and its explicit interactive-role set rather than deprecated attribute/action-name APIs. Cell metadata still comes from its owning control. A scan of added lines against the installed SDK's deprecated AX selectors finds **zero matches**.
- Hosted SwiftUI requires enhanced accessibility activation. The test-only `HostedAccessibilityApplication` bridge calls AppKit's existing `isAccessibilityEnhancedUserInterface` / `setAccessibilityEnhancedUserInterface:` selectors and restores the previous state. These selectors are not declared in Apple's public protocol headers; the test fails explicitly if the getter is unavailable. They are confined to the test target.
- Focused audit: **5 tests, 0 failures**, **70 scenarios**, **1,199 interactive control observations**. Every scenario's count matches the original `GREEN.txt` exactly. Native row selection/disclosure and all existing control-reachability assertions pass.

### Strict release build

Executed from `apps/mac` against the final source:

```sh
swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

Exit status **0**; complete captured output contains **0 warnings and 0 errors**:

```text
[0/1] Planning build
Building for production...
[0/3] Write swift-version--58304C5D6DBC2206.txt
[2/4] Compiling TesseraFFI TesseraFFI.swift
[3/5] Compiling TesseraCore AppDefaultsIsolation.swift
[4/6] Compiling Tessera AISettingsView.swift
[4/6] Write Objects.LinkFileList
[5/6] Linking Tessera
Build of product 'Tessera' complete! (279.72s)
```

### Required serial gate

Executed from the worktree root:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-42
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

Exit status **0**:

```text
Build complete! (12.61s)
Executed 905 tests, with 3 tests skipped and 0 failures (0 unexpected) in 353.323 (353.406) seconds
✔ Test run with 5 tests in 2 suites passed after 0.029 seconds.
SWIFT GATE OK
```

The FFI step reports existing vendor LibRaw warnings; the strict Swift release build above is warning-free. An earlier focused run hit the existing two-second toolbar layout-settle limit and passed on rerun. An earlier full gate had one unrelated `PreviewEventsTests.testConcurrentDeliveriesPublishBeforeCallbackAndEvictOlderImages` failure; that test passed both its focused rerun and the final full gate. No unrelated test or timeout changes were made.

`git diff --check` is clean. Both commits remain local on `wp/B5-42` with the requested co-author trailer. No Rust source, `Cargo.lock`, or `board.json` changes. No GUI launch or interaction; validation used the required builds and background hosted tests.
