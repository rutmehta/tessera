# B5-16a / B5-16 current reconciliation

Read-only source review. No builds, applications, GPU work, checkout/branch changes, B messages, or main merge. Runtime remains with the storage reviewer. Read current TASK-BOARD.md/MACHINE-A.md, original B5-16-REVIEW.md, and the full chronological CODEX-TAKEOVER from origin/wp/B5-16. Historical paragraphs are not current acceptance.

## Pinned refs

- main: `1496ebc9b204ca50fe3b37a0d898513c9d51b9c3`
- b516: `d572d0953a76129b2dd2d5ef3f821cb556c60864`
- save: `1e45baaead70b1bcd208afba800bcacd92213ee8`
- b516a: `a6e63615ed446655787563eabebf08284dc485f2`
- b516_layout: `5d456ad9bc89130c13289e0ada3a272ed4e7abab`
- b516_review_fix: `8184da175d30c6b8d39de19baf363c4ee84dbfe9`

Machine-readable exact Git blob comparisons and ancestry: `/tmp/tessera-b516-current-comparison.json`. Remote ref is the locally available origin/wp/B5-16 snapshot; no fetch or claim of unseen B work.

## Already integrated — do not repeat

- **B5-16a adjustment JSON/model round-trip:** source a6e63615 is an ancestor of main (via ae6055ae). More importantly, current main, current B5-16 tip and Save As candidate have identical Git blobs for AdjustmentAnalysis.swift, DocumentAdjustmentModels.swift, DocumentAdjustments.swift and StubCompositor.swift. The equivalent B commit132cba3e is not an ancestor, yet its four model/analysis blobs also match main. This is not outstanding work.
- **Later B resource fixes:** integration checkpoints261a585f (timers),5fa0faea (outline),1c0f36b8 (viewport ownership),1fb7e983 (preview/bake cancellation),78492f7f (frame cancellation/cache) are ancestors of current main. Preserve the dated gates and limitations; do not redispatch those completed slices because the old B5-16 tip has different ancestry.
- **Owned Save As presentation:** main includes11b31be6 and current DocumentSavePresenter. The original B5-16 tip lacks this file; replacing from that branch would discard later work. Checked destination intent is a distinct current candidate, described below.
- **Document strict/key/checkbox fixes:** integrated through4c07e5eb. Current AdjustmentEditors has the reviewed MainActor Sendable synchronous inline Binding setter. The B5-16 tip retains an older unisolated setter; preserve current main when reconciling persisted editors.
- **Smart Preview desktop/native/GPU:**89b78881 and8e100b80 are main ancestors. These completed current APIs/routing are absent or older in the B5-16 branch. No repeat implementation or old generated-binding replacement is appropriate.

## Genuinely outstanding B5-16 source integration

The branch being nonancestor is NOT the evidence by itself. Direct source comparison establishes:

- Main and Save As candidate still use the stacked Properties/Layers/Channels/History DocumentInspector. B5-16 has segmented Stack/Properties/Channels, budgeted panes and resizable persisted History. DocumentInspectorLayout.swift and DocumentInspectorTabsTests.swift exist only in B5-16, absent from both current main and Save As candidate. B's layout/scroll/sheet/tool-palette and long-document tab changes therefore remain a source reconciliation task.
- Main's Auto editor still uses session autoClip; B uses separate persisted shadowClip/highlightClip. Main's Color Lookup editor still ignores persisted filename/dither in dispatch and uses session lookupFile; B edits both persisted values. The round-trip models being present does not mean these UI controls shipped.
- B8184da17 fixes the original review's modern frozen-stat/legacy Neutralize editing contract through matchColorSettingNeutralize, with explicit missing-source explanation and tests. Main retains the earlier compatibility accessor and source reanalysis path, not that revised B helper/editor/test implementation. Do not ask B to re-fix the historical finding from scratch; carry its already-authored regression and implementation into a current-base reconciliation, preserving current strict checkbox code.
- The portable B self-test runner/failure-accounting files and tests are absent from main/Save As candidate. B's authored repaired runner should be reconciled if used for the next acceptance; do not use the historical shell runner or a zero-failure summary as a sufficient oracle. Preserve resource-hold handling and process ownership.

A wholesale B5-16 merge/file overwrite is unsuitable: its tip also lacks later recovery, Smart Preview, and Save As code, and differs across shared AppModel/DocumentWorkspace/FFI files. Request a minimal current-base source branch rather than treating the entire branch diff as pending feature work.

## Current checked Save As candidate is separate and already being qualified

Candidate1e45baae contains the current-main reconciliation from cfb511e5, matching regenerated bindings, and the reviewed same-directory test oracle repair. Main-versus-candidate product/test diff is exactly13 files: two generated bindings; DocumentWorkspace; DocumentBackend; DocumentSaveDestinationCommit; EngineDocumentBackend; StubDocumentBackend; three Swift test files; native document.rs, document/io.rs, and document_save_destination.rs. DocumentView/AdjustmentEditors and absent inspector-layout files are unchanged by this candidate.

Current board records native01–08, generation09 and focused36 success; adjacent/full/strict/GUI belong to the active lane and are not promoted by this review. It is not yet a main ancestor. Do not reassign this source implementation to B or restart existing qualification. Earlier accepted presentation fixes do not imply destination-collision acceptance; conversely ongoing Save As qualification does not complete B5-16 layout.

## Interactive and broader gates remain distinct

Original B5-16 recovery records six app self-test passes; repaired Channels/Text exit behavior passed; Transform was interrupted under resource hold. Those dated results do not establish a fresh current resolved-tree Transform or full B5-16 acceptance. History drag min/max and double-click reset, reopen persistence, scrolling and footer reachability, compact eight-document overflow, selected slider/tab continuity, first-responder shortcuts and VoiceOver remain B5-16 acceptance items unless a later exact-candidate result is supplied. Current general Swift suites do not contain the absent B inspector tests and cannot substitute for these interactions.

B5-11/12/13 uncovered-canvas drag checks, B5-12b Transform completion, B5-13 preserved liquify/move/extend source reconciliation, B5-14 post-scheduling performance rerun, and B5-15 main-thread/layout/P19/memory acceptance remain separate historical queue items. No new current source or runtime proof for those was found in the requested handoff/board; retain as pending rather than infer from Smart Preview or Save As successes. B's resource hold/paused heartbeat remains the governing handoff constraint; this review authorizes no B workloads.

## Smallest coordinator follow-up

After the current Save As candidate is accepted or its exact base frozen, send ONE source-only request to B's existing writer: reconcile only remaining B5-16 tabbed inspector/layout and persisted adjustment editor changes plus their already-authored Neutralize/runner regressions onto the new pinned main. Preserve current DocumentWorkspace/presenter/checked-save APIs, strict checkbox/key fixes, resource ownership and Smart Preview bindings; exclude already-integrated B5-16a and completed resource slices. Return a bounded diff, file hashes, tests and explicit UNRUN interaction matrix. A reviews/resolves/tests this tree in the existing serialized lane before scheduling isolated GUI acceptance. Do not restart broad B5-12/13/14/15 workloads or dispatch duplicate writers. No message was sent by this reviewer.
