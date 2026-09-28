# Checked Save As Swift final source review

Reviewed source-only on 2026-09-28. No checkout changes, build, GUI, test execution, or merge performed.

Candidate: origin/codex/save-destination-swift at 3ca3e9c9a832ac01ba8965d573898d40e544a718. Baseline81cc08ebe24176bbca5e052f5ac83e1c0fe620d9. Product3d4b34f46cb907f7ff3b270a40d9cd03987326dd; tests7b7a5dcb/8976f2fe/3ca3e9c9. Read B5-16 handoff via git show. Main inspected at171d5509ce4949bf1d5045b95a872562c5d14c9a.

## Findings

No actionable correctness blocker found in the reviewed Swift change within its stated ordinary destination-race scope. This is source approval, not runtime acceptance.

One concrete integration prerequisite: native/generated baseline81cc08eb is not in current main. Merge base is11b31be6b09ba4988e461846187c9dab98b2f890. Main lacks save_as_checked / DocumentSaveDestinationIntent and generated saveAsChecked. Applying only the Swift commits cannot compile. Integrate the separately qualified native destination-intent lineage first, then regenerate bindings and archive from the combined current sources. Do not overwrite current Smart Preview bindings with the older81cc08eb generated files/archive. This is a dependency blocker, not a new defect in B implementation.

## Reviewed behavior

- Workspace latches createIfAbsent on form submission. Only the matching affirmative replacement presentation after native drain grants replaceConfirmed. Missing submitted-form intent fails closed. Legacy headless/ordinary paths remain separately explicit.
- Checked backend protocol requirement has no default legacy fallback. Real Engine adapter maps typed native result and intent; no error-message parsing.
- Destination conflict settles once without success reload, Saved status, folder/path/title advance, or continuation. Latest-request status guard and remove/latch-before-callback ordering preserve reentrant ownership. Cancellation of an admitted writer retains its actual result.
- Stub save gate precedes path resolution/snapshot and stays held through publication, while model lock is released for I/O. Captured history head marks the bytes that were actually written; later edits remain dirty. Model and history identifiers are immutable/monotonic enough for the tested capture semantics.
- mkstemp reserves unique same-directory stage; write/fsync loops handle interruption; open descriptor pins inode. Exclusive rename or unsupported-only hard-link fallback cannot replace an ordinary late destination. Explicit replacement uses rename. Successful publication stays saved if cleanup/close diagnostics follow.
- Stage identity comparison rejects an already-observed replaced name. Name ownership is relinquished once after rename or cleanup attempt, preventing later cleanup from deleting a reused old name. No atomic hostile-directory unlink/CAS or power-loss directory-fsync guarantee is inferred.
- Tests cover real destination byte/inode preservation, independent writers, stage replacement/reuse, fallback collision and cleanup failure, captured-head edits, save path serialization, typed Engine mapping, and presenter settlement/cancellation. Tests remain unrun on this candidate.

## Main integration overlap

Among the five changed handwritten source files, only StubDocumentBackend.swift has changed on main since B baseline. Main's change freezes sampledStep into let step for concurrent render capture around line849; B's save gate/output hunks are separate. Preserve that strict-concurrency repair. DocumentWorkspace/DocumentBackend/EngineDocumentBackend and reviewed test files have no same-file main delta from81cc08eb. New commit helper/test files are new. Native/generated integration has overlapping current Smart Preview additions and requires normal coherent regeneration, not stale-file replacement.

Candidate diff --check against81cc08eb passed during review. No claim about future combined generated whitespace.

## Exact next qualification gates

1. At an explicitly assigned compiler boundary, integrate native destination-intent prerequisite and B source/test commits into the feature checkout; inspect combined diff and preserve current Smart Preview and sampledStep changes. Freeze all Git inputs plus ignored generated bindings/archive. Regenerate via apps/mac/build-ffi.sh with the shared BetterSSD target, MACOSX_DEPLOYMENT_TARGET=15.0 and jobs2. Review delta/checksums and record direct exits.
2. Establish meaningful regression evidence using the authorized minimal checked-writer seam; these new tests reference new API and are not a compilable baseline behavioral RED by themselves. Never label baseline compile failure a behavioral regression.
3. Run release focused Swift tests filtered DocumentSaveDestinationCommitTests|DocumentSaveSettlementTests|EngineDocumentBackendTests against the coherent archive. This includes real Darwin directory/dangling-symlink outcomes and real link fallback under injected unsupported rename errno. Preserve any compile/runtime failures and retained-stage diagnostics.
4. Run adjacent DocumentSavePresenterTests|DocumentSaveSheetAttachmentTests|DocumentLoadSettlementTests|DocumentLoadStatusTests and applicable document history/close suites. Respect existing opt-in GUI/probe conditions; do not claim skipped probes executed. Then full Release Swift tests and strict product build with -strict-concurrency=complete -warnings-as-errors. Revalidate native focused document_save_destination plus relevant document I/O/FFI gates if native sources are newly combined or changed.
5. In a separately authorized isolated disposable GUI session, exercise an actual late destination collision through Save As, retry with a new name, and actual affirmative Replace. Verify sentinel bytes/inode for conflict, actual reopened saved content for success, dirty/path/title/status/folder behavior, and stage cleanup/diagnostics. No real user destinations, protected-dialog bypass, or global Quit expansion.
6. Freeze post-gate manifests and archive hashes, report exact counts/direct exits, retain failures, then request root integration. No speed, arbitrary-filesystem, universal cleanup, hostile-directory identity-CAS, or directory durability claim.

## Preserved A native prerequisite evidence located

Main already contains text evidence at tools/orchestrate/wp/B5-16/evidence/2026-09-28-save-destination-native/README.md; external originals are under /Volumes/betterSSD/tessera-validation/native-save-destination. Native product source candidate75a1b9a66f74e2857ada49140a2cb8aa2cd24bac, test-only final2a22145bdde1035559ad906fa511ff4fb41b66d5, generated checkpoint81cc08ebe24176bbca5e052f5ac83e1c0fe620d9. Source maps and11 direct-exit files accompany logs. Historical target was /Volumes/betterSSD/tessera-cache/target/main with jobs2/Rayon2; do not relabel this old evidence as current shared target. New qualification must use the currently assigned target.

Historical recorded gates:01 private commit4;02 real session5 including native/PSD/PSB decode and independent-writer races;03 native roundtrip1;04 PSD/PSB save-back1;05/06 fmt/strict;07 directory diagnostic;08 final directory assertion;09/10 final fmt/strict;11 archive/bindings generation. All recorded direct exits0. No full app or Swift integration acceptance was claimed. Recorded archiveSHA256 f451870f53a740c17f179ecc81477f7c09b16aa0008bc32ef4754016691ddf80 is provenance only, not suitable to replace the current combined archive.

Exact historical commands from preserved command files:

- 01-private: `cargo test -p tessera-ffi --release --lib destination_commit_tests -- --test-threads=1`
- 02-session: `cargo test -p tessera-ffi --release --test document_save_destination -- --test-threads=1`
- 03-native-roundtrip: `cargo test -p tessera-ffi --release --test document tessera_doc_round_trip_and_same_path_same_session -- --test-threads=1`
- 04-psd-psb: `cargo test -p tessera-ffi --release --test document psd_opens_with_names_and_modes_and_saves_back_unknown_keys -- --test-threads=1`
- 10-final-strict-clippy: `cargo clippy -p tessera-ffi --release --all-targets -- -D warnings`
- 11-build-ffi: `bash apps/mac/build-ffi.sh
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main
CARGO_BUILD_JOBS=2
RAYON_NUM_THREADS=2`
