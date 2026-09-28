# Save As current-main reconciliation — independent source review

Reviewed source-only: origin/codex/save-destination-current-main cfb511e59c8fd8fdcf8aa7292eb40c7d6f8ba411; combined product/test checkpoint af49e4ef0e8962ff2013ba686923b53171e09746; pinned base29a4cefd. Main observed7199f9e2 has no apps/crates delta from that pinned base. Read branch handoff/SOURCE.sha256 and main docs/coordination/SAVE-DESTINATION-INTEGRATION-REVIEW.md. No checkout edits, builds, tests, launch or GPU workload.

Disposition: no new semantic integration blocker identified. The reconciliation faithfully preserves reviewed candidates and current Smart Preview code. This is source approval, not compilation or runtime qualification.

## Provenance verified

All11 product/test SHA256 entries independently recomputed from cfb511e5 Git blobs match SOURCE.sha256. All3 native files are byte-identical to2a22145b;7Swift files are byte-identical to3ca3e9c9. The remaining StubDocumentBackend.swift differs from3ca3e9c9 only by current main's immutable sampledStep/let step concurrent-render repair. Exact whole diff from29a4cefd contains these11paths plus only the handoff and hash manifest. Thus Smart Preview APIs, journal, UI, AppModel, tests and generated outputs outside these paths remain exactly pinned-main bytes. git diff --check29a4cefd..cfb511e5 passed.

Scope:
- apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift
- apps/mac/Sources/TesseraCore/Document/DocumentBackend.swift
- apps/mac/Sources/TesseraCore/Document/DocumentSaveDestinationCommit.swift
- apps/mac/Sources/TesseraCore/Document/EngineDocumentBackend.swift
- apps/mac/Sources/TesseraCore/Document/StubDocumentBackend.swift
- apps/mac/Tests/TesseraCoreTests/DocumentSaveDestinationCommitTests.swift
- apps/mac/Tests/TesseraCoreTests/DocumentSaveSettlementTests.swift
- apps/mac/Tests/TesseraCoreTests/EngineDocumentBackendTests.swift
- crates/tessera-ffi/src/document.rs
- crates/tessera-ffi/src/document/io.rs
- crates/tessera-ffi/tests/document_save_destination.rs

## Behavior checked

Form submission latches create-if-absent when no replacement was requested; only affirmative matching replacement settlement grants replaceConfirmed. Checked protocol requirement has no permissive default; engine intent/results map typed UniFFI enums. Conflict exits before saved-node/path/title update and before Swift success reload/continuation. Ordinary Save and legacy Save As preserve explicit replacing publication.

Native save serialization covers snapshot/publication/state advancement. Stub saveGate is acquired before current-path lookup and model snapshot; captured history head is marked saved rather than a later edit head. Swift staging keeps descriptor ownership through publication/cleanup, retries interrupted writes/fsync, uses exclusive rename with unsupported-only hard-link fallback, and does not convert post-publication diagnostics into a false failed save. Stage-name identity checks and one-shot cleanup preserve the documented ordinary-race scope; they do not imply hostile-directory atomic identity CAS or directory-power-loss guarantees.

## Required integration dependency

Current retained generated bindings intentionally lack saveAsChecked and its enums. Regenerate bindings AND matching native archive from combined current source before Swift compilation, retaining Smart Preview APIs. Do not transplant historical81cc08eb generated files/archive. All combined new tests remain unrun. Follow the detailed native/Swift/GUI gates in main's integration review; historical component passes are not combined-head acceptance.

## Existing render-resource-bounds worktree inspection

Path /Users/rutmehta/.codex/worktrees/render-resource-bounds/tessera; HEAD29e5ba8fbd760413354d86eb5c80c1fd83dd60e4, branch codex/document-save-destination-intent. Tracked status clean, no nonignored untracked files, no working diff. Ignored content exists: .superpowers/, apps/mac/build/, fixtures/raw. Preserve it; old generated/build artifacts are not current combined qualification inputs. Historical branch/evidence commits remain valuable and should be retained when selecting a new branch.

Read-only ps inspection found no cargo/swift/xcodebuild or active render-resource-bounds runtime command. A second lsof cwd inspection from outside that checkout found no process rooted there (the first inspection saw only its own temporary inspection commands). Therefore it appears suitable for root to reuse at this moment after its normal ownership check, without deleting or resetting ignored material. No checkout/branch/files were changed here; process observations are point-in-time, not a lease.
