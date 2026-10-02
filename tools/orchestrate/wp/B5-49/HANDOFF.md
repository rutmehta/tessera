# B5-49 — background keyboard checklist

Branch: `wp/B5-49`. Original B5-49 record; superseded by [B5-49b review corrections](../B5-49b/HANDOFF.md), including the explicitly authorized push. Base main: `486d069f`; merged harness tip: `fa8caea7`; merge commit: `30dd3d03`. Keyboard restoration fix: `68e61fcb`; checklist implementation: `bace9874`. The B5-25 harness branch was merged with `--no-ff` onto this worktree's current main without conflicts. Main's accessibility identifiers and subsequent shortcut/harness changes remain intact.

## What changed

`DocumentKeyboardChecklistTests.testCombinedKeyboardChecklist` runs the combined checklist through a real hosted document/inspector and background responder chain. It uses `LayoutProbeHarness` (isolated `--app-dir` preferences, prohibited activation, order-back-only windows), a two-JPEG library, and documents with at least three layers. Controls are resolved by B5-42 `document.*` identifiers. The library collection is resolved by its native type because the B5-42 namespace covers document controls.

Keys go directly through `KeyRouter`, then `NSWindow.sendEvent` only when unhandled. Real mouse events exercise marquee and pan gestures. Native commands without an application menu scene use their actual action methods, explicitly identified in the results. No global key posting, app activation, key/main-window promotion, FKA toggling, or system setting writes are used. Every delivered key asserts that the fixture window is neither key nor main and the test application is inactive.

With `TESSERA_REGENERATE_KEYBOARD_RESULTS=1`, the runner writes `RESULTS.md`, its archival alias `GUI-RESULTS.md`, and `focus-hosted.jsonl`. The latter contains actual `InspectorFocusTrace.snapshot` values and `KeyRouter` handled results, with global sequence, step, key code, repeat/up flags, document ownership, focused identifier after delivery, and layer count before every key (including every Delete). Production `routeEvent` excludes non-key windows; the test therefore labels its trace `background-hosted-direct-delivery` rather than inventing owned-key-window eligibility. It also records Delete/tool/up events beyond the production logger's restricted keys and 32-event limit. N/A means no acceptance claim; missing required hosted controls or failed checks produce FAIL and fail XCTest.

The combined test found one merge-related safety gap: after removing a focused inspector and restoring it with Tab, `NSWindow` could retain first responder. A subsequent Delete could then remove a layer with no visible canvas owner. B5-49b moves the fix into `setPanelsHidden(false)`, reusing B5-25's `claimKeyboardIfStray` in the visible viewport's own window for Tab, Show Panels, screen-mode restoration and document exit. It never makes the window key. Both the original panel restoration regression and the new checklist require visible canvas ownership before the next Delete.

The B5-44 generated `docs/shortcuts.md` reference was regenerated with `python3 tools/orchestrate/shortcut-audit.py --write-doc` after the merge/fix. The audit enumerates 63 menu bindings, 11 routing sources, and 16 reserved chords; it reports `SHORTCUT AUDIT OK`. No shortcut integrity rule was relaxed.

## Acceptance boundaries

These results test responder ownership and behavior, not visible focus-ring rendering or physical keyboard delivery. Space repeat is 30 injected repeat events with timestamps spanning one simulated second. Undo invokes the document Undo action directly; native menu key-equivalent integrity remains covered by main's existing tests.

Step 12's “selection clears” follows main's existing Clear semantics: Delete clears **selected pixels**, creates one `Clear` history operation, preserves layer count, and retains the marquee. The runner uses a real 96×64 engine document because `StubDocumentBackend.deleteSelection` deliberately throws “needs engine.” It does not change main to interpret Delete as Deselect. Panel entry for this subcheck is explicit first-responder assignment; canvas Tab itself hides panels, as required by steps 1–3.

The background layer-name field editor proved usable: real character Delete, typed Space, Escape cancellation, and native Tab are automated. Escape is checked before Tab (Tab commits/end-editing), then rename is reopened for the Tab check.

## Per-step handoff

The detailed sequence/nativeType/handled table is in [RESULTS.md](RESULTS.md). Substeps keep partial automation separate from remaining native acceptance.

| Checklist step | Automated | Remaining N/A / reason |
| --- | --- | --- |
| 1 | 1a: two-JPEG Library responder, injected ⌘E, confirmation action, automatic viewport focus, Tab hide/restore; repeat via New Document action | 1b: native ⌘N application menu dispatch and visible sheet interaction require the application command scene |
| 2 | Switch existing documents without canvas click; Tab hide/restore | None |
| 3 | Canvas ⇧Tab unhandled; panels unchanged | None |
| 4 | Stack/History expanded; identified + action enables −/+/↺; record H0 | None |
| 5 | List Tab is traced; panel state checked | FKA off: real eye reports `canBecomeKeyView=false`; native list Tab skips it for History −. No eye ring claimed |
| 6 | 6b: actual History − → + → ↺ in two Tabs; ⇧Tab returns to +; every event unhandled | 6a: with FKA off, forcing the eye responder does not make it a native key view; Tab does not progress in the combined host. Eye → History ring traversal remains manual |
| 7 | Space on + moves one row, Return zero/one row, Space reset returns default; no pan | None |
| 8 | History traversal handled in 6b | Native toolbar/titlebar focus-ring traversal needs application/key-window FKA integration |
| 9 | Both Delete codes on identified eye preserve L and responder; handled=true | None |
| 10 | Eye Space toggles once, 30 repeats do nothing, release does nothing, second press toggles back; no pan | None |
| 11 | Existing B5-25 unit tests still cover plain-button and generic-proxy routing, but these are not counted as acceptance | Real sidebar action uses `NSApp.sendAction` with no explicit target; real SwiftUI.KeyViewProxy activation needs the key-window focus ring |
| 12 | Real engine marquee drag; eye Delete preserves selection/count; canvas Delete makes exactly one pixel Clear operation | None, with the Clear-versus-Deselect interpretation above |
| 13 | B/V select Brush/Move while eye focus and panels stay | None |
| 14 | Actual Layers outline Delete removes one selected leaf; Undo restores; L recorded | None |
| 15 | Canvas without pixel selection deletes one layer; Undo restores; real Space-drag changes viewport center; release ends pan | None |
| 16 | 16a: real layer-name field editor Delete, Space, Escape, native Tab; no layer deletion/pan | 16b: FKA off; Properties Name → Load LUT → Dither native traversal |
| 17 | 17a: directly focused Dither activation, repeat/release suppression, one history entry and Undo | 17b: native focus-ring traversal needs FKA/key window |
| 18 | Hide Panels action from eye focus; bounded Tab restoration, visible owner, safe post-restore Delete | None |
| 19 | 19a: injected F twice/restoration and F back to standard; safe post-restore Delete | 19b: macOS full-screen/Space transition requires an application window and could take focus |
| 20 | Canvas Tab hides panels; direct click at former inspector coordinates; canvas Tab restores | None |
| 21 | 21a: real Library action; grid Right arrow advances image with document=false; injected ⌘E/confirmation and focus/Tab reentry | 21b: physical toolbar Library click and visible sheet UI require application scene |
| 22 | 22a: TEARDOWN only, excluded from PASS counts; hosted window disposes cleanly | 22b: app Quit/save-prompt lifecycle cannot be substituted with XCTest process exit |

Machine A's remaining manual pass is **exactly 1b, 5, 6a, 8, 11, 16b, 17b, 19b, 21b, 22b**. Use the requested FKA-on environment for the ring checks; this runner never changes that setting. The observed machine has `AppleKeyboardUIMode` unset (`defaults read -g AppleKeyboardUIMode` exits 1) and `NSApp.isFullKeyboardAccessEnabled == false`.

## Reproduce

From this worktree:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-49"
cd apps/mac
./build-ffi.sh
swift test -c release -Xswiftc -enable-testing --filter DocumentKeyboardChecklistTests
cd ../..
tools/orchestrate/swift-gate.sh
cd apps/mac
swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

The runner only rewrites the result/trace artifacts with `TESSERA_REGENERATE_KEYBOARD_RESULTS=1`; routine tests never write them. Addresses in JSONL are normalized to `<address>`. Setup failures are explicit FAIL rows. On FKA-enabled machines, 5/6a are attempted normally; they are not hard-coded skips. Full UI-only rows remain N/A because no app command scene/key window is created.

## Original B5-49 validation (superseded by B5-49b)

- Merge: no conflicts; no Rust, `board.json`, or `Cargo.lock` changes.
- Targeted original keyboard/History suites plus runner: 34 tests, zero failures.
- Combined checklist: 18 automated PASS substeps, 10 explicit N/A, zero FAIL; 77 consecutive trace records, all `keyWindow=false`.
- Full Swift gate: **SWIFT GATE OK** — 943 XCTest tests, 3 skipped, zero failures; 5 Swift Testing tests passed.
- Strict release product build: **PASS**, `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` (exit 0).

See [swift-gate.log](swift-gate.log) and [release-build.log](release-build.log) for final gate evidence.
