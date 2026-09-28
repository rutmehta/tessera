# Checked Save As reconciliation onto accepted Smart Preview main

Request fc30ff89-f16d-4d3a-9de7-087f2284b47a; B source-only, 2026-09-28.

Baseline: `29a4cefd` (immutable). Branch: `codex/save-destination-current-main`.
Combined source checkpoint: `af49e4ef`. **UNRUN and not compile-qualified.**
A owns all regeneration, compiler/runtime gates and main integration. No B builds,
tests, apps, GPU, heartbeat restart or writer changes occurred.

## Commit mapping

| Reviewed input | Reconciled commit | Scope |
| --- | --- | --- |
| 75a1b9a66f74e2857ada49140a2cb8aa2cd24bac | 23c5bd0f | Native prerequisite, using merge parent 2 (11b31be6) as mainline; only three native source/test files |
| 2a22145bdde1035559ad906fa511ff4fb41b66d5 | aef5e5c8 | Native directory-collision assertions |
| 7b7a5dcb | 15ae6321 | Swift checked-save tests |
| 3d4b34f4 | eab89438 | Swift checked intent, atomic stub publication and settlement |
| 8976f2fe | fe3c19ca | Confirmed Replace/staging reuse tests |
| 3ca3e9c9 | af49e4ef | Fallback-conflict/dirty native session tests |

Original published branches/candidates remain intact, including
`codex/save-destination-swift` at 3ca3e9c9 and
`codex/smart-preview-thumbnails` at d40354e4. Reused the clean completed B5-16
checkout after process inspection found no build/runtime using it. B5-16a and
its preserved resource snapshot were not touched.

## Source reconciliation and limits

No unresolved source conflicts. All three native files are byte-identical to
2a22145b. Seven Swift files are byte-identical to 3ca3e9c9. The eighth,
StubDocumentBackend.swift, differs only by main's preserved immutable
`let step = sampledStep` concurrent-render capture fix. Native checked save API
and typed Swift mapping agree by source inspection. Only the two existing
DocumentBackend conformers require the new checked method; both implement it.

The diff touches exactly these eleven product/test files:

- `apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift`
- `apps/mac/Sources/TesseraCore/Document/DocumentBackend.swift`
- `apps/mac/Sources/TesseraCore/Document/DocumentSaveDestinationCommit.swift`
- `apps/mac/Sources/TesseraCore/Document/EngineDocumentBackend.swift`
- `apps/mac/Sources/TesseraCore/Document/StubDocumentBackend.swift`
- `apps/mac/Tests/TesseraCoreTests/DocumentSaveDestinationCommitTests.swift`
- `apps/mac/Tests/TesseraCoreTests/DocumentSaveSettlementTests.swift`
- `apps/mac/Tests/TesseraCoreTests/EngineDocumentBackendTests.swift`
- `crates/tessera-ffi/src/document.rs`
- `crates/tessera-ffi/src/document/io.rs`
- `crates/tessera-ffi/tests/document_save_destination.rs`

All other tracked files match pinned main, including Smart Preview APIs,
controls, tests, journal code, AppModel and generated output. No old 81cc08eb
bindings/archive were copied. The retained current-main bindings lack the new
checked Save As surface; A MUST regenerate from the combined native source and
build the matching archive before Swift compilation. Existing ignored build
artifacts were neither regenerated nor qualified.

Reviewed publication paths retain create-if-absent collision protection and
explicit confirmed replacement. Collision returns typed non-success without
success reload, path/title/folder advance or continuation. The stub save gate
covers snapshot through publication; captured history head preserves later edits
as dirty. Native regular Save, legacy Save As and flat-export replacement remain
explicit. No new semantics beyond the reviewed candidates were introduced.

Source validation: combined `git diff --check 29a4cefd` passed; exact candidate
byte comparisons and scoped file inventory above. Source hashes are in
SAVE-DESTINATION-CURRENT-MAIN-SOURCE.sha256. These are inspection results, not
executed tests. Historical native gates remain historical; new combined tests
are ALL UNRUN. No new behavioral RED/green or runtime acceptance is claimed.

## Required A gates

Use docs/coordination/SAVE-DESTINATION-INTEGRATION-REVIEW.md on pinned main as
the detailed gate contract. In A's assigned compiler lane:

1. Freeze combined Git inputs; regenerate current bindings and archive together
   through apps/mac/build-ffi.sh using the currently assigned target, jobs 2 and
   MACOSX_DEPLOYMENT_TARGET=15.0. Verify Smart Preview APIs survive regeneration.
2. Requalify native destination_commit_tests, document_save_destination and
   native/PSD/PSB round-trip/ordinary-save tests; run applicable strict/fmt checks.
3. Run focused Release Swift DocumentSaveDestinationCommitTests,
   DocumentSaveSettlementTests and EngineDocumentBackendTests, then adjacent
   presenter/attachment/load/status/history/close tests. Preserve opt-in probe
   status and establish meaningful behavioral regressions, not compile-failure RED.
4. Run full Release Swift and strict-concurrency/warnings-as-errors product
   build against the coherent archive. Retain failures and direct exit evidence.
5. In a disposable isolated GUI session, verify late destination collision
   preserves sentinel bytes/inode, retry with a new name, affirmative Replace,
   reopen actual saved content, and dirty/path/title/status/folder/stage outcomes.
6. Freeze post-gate hashes and report actual counts/exits before A integrates main.

No broad filesystem, hostile-directory identity-CAS, guaranteed cleanup,
directory power-loss durability, performance or full-app acceptance is claimed.
