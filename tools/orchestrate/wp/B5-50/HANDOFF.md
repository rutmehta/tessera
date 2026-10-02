# B5-50c — Machine A follow-up corrections

Branch `wp/B5-50`, commits on supplied `058a7e9e`.

| Review item | Status and evidence |
| --- | --- |
| 1. Four indirect identifiers | Restored `detail-ai-denoise-model`, `lensblur-model`, `transform-upright-reset`, and `transform-reset`. The first two also restore their `-retry` actions: **4 helper inputs / 6 concrete IDs**. Pin extraction now captures literal `id:` and `identifier:` arguments; 545 baseline stems are pinned. `testEstablishedIdentifiersFromMainRemainPresent` and `testModelProgressRetryIdentifiers` pass. |
| 2. Identifier format | Restored the `^(library|develop)\.` rule with explicit main compatibility: exact static identifiers and pinned dynamic stems. Separator-ended stems and main's separator-free `facet` family accept payloads; static `grid` does not accept `gridUnexpected`. `testIdentifierFormatAllowsOnlyNamespacesOrPinnedStems` covers both accepted and rejected values; hosted audits apply the predicate to every interactive node. |
| 3. Original RED citation | Original **468-failure RED is at `f554c7a8:tools/orchestrate/wp/B5-50/RED.txt`**. B5-50b overwrote the working-tree RED/GREEN records. B5-50c evidence has separate filenames. |
| 4a. Folder identity | Opaque folder/basket keys are retained in an in-memory, main-actor-isolated process-session registry, surviving row rebuilds without exposing paths. `testSidebarFolderIdentifierSurvivesRebuild` verifies stable, distinct, private IDs. |
| 4b. Value identity | Restored value/model identity in the seven enumerated lists changed by this lane: applied keywords, keyword tree, both snapshot menus, suggestions, import roots and mark mappings. Indexes remain only in accessibility identifiers. Unrelated pre-existing offset-based lists are unchanged. |
| 4c. History label | History toggle uses `item.label` without the action-dependent “Enable” prefix. Updated the lane identifier map. |
| Additional indirect-ID audit | Compared changed sources with `git show origin/main:<file>` at `270f0169a36ada716ab2e6930082f81e0c0edf20`, covering helper arguments, assigned identifiers, concatenation and interpolation prefixes. **0 additional renamed inputs** beyond the four reported; see [B5-50c-AUDIT.md](B5-50c-AUDIT.md). Intentional private payload substitutions retain the established three prefixes. |

RED-first commit: `1c9321af`; 3 focused tests, 5 assertions failed (four missing IDs plus unstable folder identity). Production changes followed this run. The first full gate exposed six pre-existing `facet<title>` identifiers rejected by the new format predicate; main's explicit dynamic stem was then admitted without relaxing static identifiers. No production baseline IDs were renamed to satisfy the format check.

Implementation commit: `3391d9b5`.

Verification: required `build-ffi.sh` → `swift-gate.sh`: **SWIFT GATE OK**.
930 XCTest tests, 3 skipped, 0 failures; 5 Swift Testing tests in 2 suites passed.
All 11 Library/Develop accessibility tests passed. Strict release product build:
**PASS**, `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors`,
273.97 seconds, zero warnings/errors. `git diff --check` passes. After committing these
lane evidence files, `git status --porcelain` is empty; neither gate introduced generated drift.

No foreground GUI launch or system-setting change. Hosted probes remain non-key/inactive.
No Rust, Cargo.lock, board.json, or generated-binding changes from `058a7e9e`.
Environment: Cargo bin on PATH; `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-50`,
`CARGO_BUILD_JOBS=4`, `RAYON_NUM_THREADS=4`.

---

Historical B5-50b record (superseded where corrected above):

# B5-50b — Machine A round-two corrections

Branch: `wp/B5-50`, on supplied `f554c7a8`. Binding inputs: A-ROUND2.md B5-50b and LR-RULINGS.md.
No Rust source, Cargo.lock, or board.json edits. Synthetic fixtures only. No foreground GUI launch,
focus claim, system setting change, or access to the protected personal library locations.

## Item-by-item review response

| Finding | Status / implementation | Regression evidence |
| --- | --- | --- |
| 1. Preserve established identifiers | Done. Audited all macOS sources against local `origin/main` (`486d069f`). Enumerated 98 removed declarations/families in RESTORED-IDENTIFIERS.md. Restored static names and established family prefixes, including import/report, toolbar, lens blur, sidebar, transform, workspace, assist and photo inspector. The TransformPanel Upright comment once again matches the implementation. ACCEPTANCE and B5-42 use established names. | `testEstablishedIdentifiersFromMainRemainPresent`: 541 baseline stems; exact identifier-expression extraction. `LightroomImportAccessibilityTests`, `DocumentAccessibilityTests`, and the hosted Library/Develop audit use restored IDs. |
| 2. Distinct cancellation actions | Done. Sheet dismissal is `library.import.cancel`; running import cancellation restores the established `lrimport-cancel`. | Baseline regression pins `lrimport-cancel`; `testImportStepsAndSyntheticReport` audits sheet steps and sibling uniqueness. |
| 3. No user data in identifiers | Done. Presets, snapshots, keyword chips/tree/suggestions, import root resets and mark mappings use row indexes. Sidebar folder/basket rows use opaque session-stable UUIDs. Accessible labels retain readable names. | `testKeywordIdentifierPrivacy`, `testSidebarFolderIdentifierPrivacy`, `testImportStepsAndSyntheticReport`, `testLibraryAndDevelopWindows`: distinctive synthetic preset/snapshot/keyword/root names are rejected in every traversed identifier, including percent-decoded values. |
| 4. Single-select sidebar | Done. AX selection passes `byExtendingSelection: false`. | `testSidebarNativeSelectionAndDisclosureArePreserved` selects another row first, never deselects before selecting the target, and asserts the exact singleton selection. Disclosure remains tested. |
| 5. Avoid redundant AX metadata writes | Done. Slider identifier/label setters compare their current values; thumbnail identifier and complete label do likewise. Thumbnail style or item changes still update the identifier. | `testMetadataUpdatesAreConditionalAndSidebarRemainsNative` pins setter guards; hosted window and panel audits verify metadata remains reachable. |
| 6. Restore native sidebar toggle | Done. Library/Develop retain the automatic SwiftUI toggle. Document preserves the pre-existing explicit toggle and its metadata. The audit exempts only the native sidebar toggle by its system-provided name. | `testMetadataUpdatesAreConditionalAndSidebarRemainsNative`, `testLibraryAndDevelopWindows`, `DocumentAccessibilityTests`. |
| 7. Document toolbar map | Done. B5-42 explicitly documents shared shell ownership and all four established `document.toolbar.*` controls. | `DocumentAccessibilityTests` plus source/map review. |
| Naming: Editingtarget | Done. New disclosure is `develop.panel.editingTarget`. | Hosted Develop window audit and updated identifier map. |
| Naming: library.assist.assist-* | Done. Restored established `assist-*` names rather than introducing another rename. Newly identified actions retain `library.assist.*`. | Baseline regression and restoration inventory. |

Ruling 3 necessarily changes the **payload**, not the prefix, of three pre-existing user-text
families: `keyword-suggestion-<index>`, `keyword-suggestion-reject-<index>`, and
`lrimport-mark-<index>`. ACCEPTANCE documents selecting the intended label at its current index.
Indexes follow visible model ordering; opaque sidebar IDs now last for the process session (B5-50c).
`lrimport-locate` remains the established constant. Fixed built-in curve-preset labels are not user data.

## RED-first evidence

Original B5-50 RED: **`f554c7a8`**, `tools/orchestrate/wp/B5-50/RED.txt`: 3 tests,
468 failures. Recover with `git show f554c7a8:tools/orchestrate/wp/B5-50/RED.txt`.
The working-tree RED.txt/GREEN.txt were overwritten by B5-50b reruns and do not
represent the original run. B5-50c records use separate filenames.

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
