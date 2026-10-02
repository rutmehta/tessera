# B5-50b — Machine A round-two corrections

Branch: `wp/B5-50`, on supplied `f554c7a8`. Binding inputs: A-ROUND2.md B5-50b and LR-RULINGS.md.
No Rust source, Cargo.lock, or board.json edits. Synthetic fixtures only. No foreground GUI launch,
focus claim, system setting change, or access to the protected personal library locations.

## Item-by-item review response

| Finding | Status / implementation | Regression evidence |
| --- | --- | --- |
| 1. Preserve established identifiers | Done. Audited all macOS sources against local `origin/main` (`486d069f`). Enumerated 98 removed declarations/families in RESTORED-IDENTIFIERS.md. Restored static names and established family prefixes, including import/report, toolbar, lens blur, sidebar, transform, workspace, assist and photo inspector. The TransformPanel Upright comment once again matches the implementation. ACCEPTANCE and B5-42 use established names. | `testEstablishedIdentifiersFromMainRemainPresent`: 541 baseline stems; exact identifier-expression extraction. `LightroomImportAccessibilityTests`, `DocumentAccessibilityTests`, and the hosted Library/Develop audit use restored IDs. |
| 2. Distinct cancellation actions | Done. Sheet dismissal is `library.import.cancel`; running import cancellation restores the established `lrimport-cancel`. | Baseline regression pins `lrimport-cancel`; `testImportStepsAndSyntheticReport` audits sheet steps and sibling uniqueness. |
| 3. No user data in identifiers | Done. Presets, snapshots, keyword chips/tree/suggestions, import root resets and mark mappings use row indexes. Sidebar folder/basket rows use opaque per-row UUIDs. Accessible labels retain readable names. | `testKeywordIdentifierPrivacy`, `testSidebarFolderIdentifierPrivacy`, `testImportStepsAndSyntheticReport`, `testLibraryAndDevelopWindows`: distinctive synthetic preset/snapshot/keyword/root names are rejected in every traversed identifier, including percent-decoded values. |
| 4. Single-select sidebar | Done. AX selection passes `byExtendingSelection: false`. | `testSidebarNativeSelectionAndDisclosureArePreserved` selects another row first, never deselects before selecting the target, and asserts the exact singleton selection. Disclosure remains tested. |
| 5. Avoid redundant AX metadata writes | Done. Slider identifier/label setters compare their current values; thumbnail identifier and complete label do likewise. Thumbnail style or item changes still update the identifier. | `testMetadataUpdatesAreConditionalAndSidebarRemainsNative` pins setter guards; hosted window and panel audits verify metadata remains reachable. |
| 6. Restore native sidebar toggle | Done. Library/Develop retain the automatic SwiftUI toggle. Document preserves the pre-existing explicit toggle and its metadata. The audit exempts only the native sidebar toggle by its system-provided name. | `testMetadataUpdatesAreConditionalAndSidebarRemainsNative`, `testLibraryAndDevelopWindows`, `DocumentAccessibilityTests`. |
| 7. Document toolbar map | Done. B5-42 explicitly documents shared shell ownership and all four established `document.toolbar.*` controls. | `DocumentAccessibilityTests` plus source/map review. |
| Naming: Editingtarget | Done. New disclosure is `develop.panel.editingTarget`. | Hosted Develop window audit and updated identifier map. |
| Naming: library.assist.assist-* | Done. Restored established `assist-*` names rather than introducing another rename. Newly identified actions retain `library.assist.*`. | Baseline regression and restoration inventory. |

Ruling 3 necessarily changes the **payload**, not the prefix, of three pre-existing user-text
families: `keyword-suggestion-<index>`, `keyword-suggestion-reject-<index>`, and
`lrimport-mark-<index>`. ACCEPTANCE documents selecting the intended label at its current index.
Indexes follow visible model ordering; opaque sidebar IDs last for the row model lifetime.
`lrimport-locate` remains the established constant. Fixed built-in curve-preset labels are not user data.

## RED-first evidence

`ce5b27e8` — `test(B5-50b): pin established identifiers and privacy regressions`.
Before production changes: 8 tests, 7 failing tests, 110 expected regression assertions.
The compatibility test found 95 missing stems; privacy fixtures exposed preset, snapshot,
keyword and root strings; the sidebar selected two rows; metadata/native-toggle guards failed.
The press-action audit passed. Compilation issues were corrected before this RED run.

## Verification

Required serial `build-ffi.sh` → `swift-gate.sh`: **SWIFT GATE OK**.
927 XCTest tests: 3 skipped, 0 failures; 5 Swift Testing tests in 2 suites passed.
All 18 relevant accessibility tests passed in that gate (Library/Develop 8, import 3, Document 7).
The preliminary focused run and single-test retry hit the Document toolbar's 2-second layout
settling deadline. The unchanged test passed in 2.27 seconds in the full gate after the FFI rebuild;
no layout timeout or acceptance predicate was widened. Existing test-code warnings and LibRaw
vendor deprecations were observed. Strict release product build: **PASS**, complete concurrency checking and warnings-as-errors,
0 warnings / 0 errors (338.73 seconds).

Implementation commit: `d5504d60`. Final documentation is committed separately.
All requested findings are complete; external VoiceOver acceptance was outside this lane. All hosted windows stay non-key and the test application inactive.
The tests restore their app-scoped preferences and AX activation; no system preference is changed.
This is an in-process hosted accessibility audit, not an external VoiceOver certification.
Transient system menus/popovers are not exhaustively opened. No private image fixture is committed.

`cargo clean -p tessera-ffi --release` removed the lane's stale FFI release artifacts before final gates.
Environment: Cargo bin on PATH, CARGO_BUILD_JOBS=4, RAYON_NUM_THREADS=4,
CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-50.

`git diff --check` passes. No generated-binding drift, Rust source, Cargo.lock, or board.json delta
was introduced by B5-50b. The final documentation commit includes the current maps and gate records.
