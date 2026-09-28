# Swift integration checkpoint — not final acceptance

Integrated base567f5e92. Generated bindings commit2fb3b1f0; mechanical Swift import/test fixes b6cea8f2. Checkpoint clean; compiler lane released to FFI owner. Full/strict acceptance deliberately remains pending native mixed-agent fix and owner-prepared Document warning repairs.

## Actual commands and outcomes

01 build-ffi.sh passed270.5s with explicit shared CARGO_TARGET_DIR/jobs2; script sets MACOSX_DEPLOYMENT_TARGET15.0. Rebuilt archive and ran uniffi-bindgen from same native outputs. No generated file hand edits. Only expected generated Swift/header copies+static archive changed; generation-changes.json records difference. Modulemaps unchanged. Target archive/dylib/bindgen hashes in native-output-hashes.json; ignored app archive and generated build copies included in every source manifest. Static archive remains private build output, not Git.

02 focused full-package compile failed: missing selective import for FolderHandle in AppModel.rescan. Fixed by import struct TesseraFFI.FolderHandle. Also exposed pre-existing Document warnings; sent to root/B owner without edits or suppressed flags.
03 focused27 ran after import:20 SmartPreview tests pass;6 routing cases pass; alias case7 assertions fail. Expected fixture Foundation resolvingSymlinksInPath normalized /private/var to /var while native canonical index correctly retains /private/var. No runtime UniFFI checksum failure.
04 full initial suite failed:672 XCTest cases,1 skipped,10 assertion failures in3 tests; separate5 SwiftTesting cases pass. Failures: AgentReviewLayoutTests mixed missing-original batch(2 assertions), alias fixture(7), WorkspaceReadyPhotoTests old workspace title without new Original suffix(1). Exact failures/logs preserved. No other failures reported.
05 focused27 after independent ONLINE POSIX realpath expected identity:27 passed. Requested symlink remains distinct; offline assertions unchanged and no offline resolution introduced.
06 combined SmartPreviewUITests|OfflineLibraryRoutingTests|WorkspaceReadyPhotoTests:28 passed,0 failures (5.488s tests,34.6s total), after updating only expected title to Editing1photo·RAW·Original. Real RAW readiness/layout/mask assertions retained.

All non-generation commands had identical before/after source/archive hash manifests. Checkpoint-manifest.json proves committed b6cea8f2 inputs exactly equal passing28 gate. No test flags relaxed. No app bundle launched or protected-dialog interaction; full package includes its existing background-window-server layout tests.

## Remaining work delegated by root

Native OriginalWriteReservation currently rejects a mixed AgentRunRequest before any result when one original is missing; existing UI acceptance requires successful originals plus a failed missing-photo review entry. FFI owner preparing batch-specific guard correction preserving dirty-journal and active-source protections. That failure must stay open, not weaken the test.

B owner preparing pre-existing strict fixes: immutable sampling-step capture in StubDocumentBackend, Sendable checkbox setter closure in AdjustmentEditors, intended modifier gating in DocumentTransforms, unused id in TransformSelfTest. No Document edits made by this implementer. Strict concurrency/warnings-as-errors command has NOT yet run; full final suite and strict gate deferred by root until fixes integrated. Regenerate native archive/bindings again after guard fix and freeze updated inputs before final acceptance.
