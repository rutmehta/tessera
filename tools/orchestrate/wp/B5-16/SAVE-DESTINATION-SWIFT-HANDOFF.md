# Checked Save As Swift source handoff — 2026-09-28

Request8638c362-1aef-4fda-9117-c5af79e50e92. Exact baseline
81cc08ebe24176bbca5e052f5ac83e1c0fe620d9 from origin/codex/document-save-destination-intent.
Branch codex/save-destination-swift. **SOURCE ONLY / UNRUN on B.**

- Tests first: 7b7a5dcb and8976f2fe.
- Product: 3d4b34f4.
- A owns compiler, runtime/GUI gates and main; this is not acceptance or behavioral RED/green.
- Generated API was read directly from this checkpoint: saveAsChecked(path:intent:),
  DocumentSaveDestinationIntent.createIfAbsent/replaceConfirmed and
  DocumentSaveAsResult.saved/destinationExists. No binding regeneration or archive transfer on B.
  A's provided archive checkpoint is f451870f53a740c17f179ecc81477f7c09b16aa0008bc32ef4754016691ddf80.

## Product changes

DocumentBackend.swift adds Sendable DocSaveDestinationIntent/DocSaveAsResult and a required checked
overload. No default forwarding to legacy saveAs. EngineDocumentBackend.swift maps generated cases
explicitly through the checked FFI API. Ordinary errors still use bridged; conflict is never parsed
from text. Legacy path-only overload remains.

DocumentWorkspace.swift latches createIfAbsent at absent form submission. It sets replaceConfirmed
only when handling the matching affirmative Replace invocation after native completion/membership
drain. Missing intent for a submitted form fails closed instead of falling through to legacy writer.
The checked writer seam and real backend both return DocSaveAsResult. DestinationExists settles
exactly once as DocumentSaveOutcome.destinationConflict(URL). Only the latest request publishes a
visible conflict; no Saved status, folder advance, success reload, then callback or continuation on
conflict. Admitted-write cancellation retains the actual conflict or actual saved result. Existing
legacy headless/ordinary Save keeps the Void writer route and replacing semantics.

StubDocumentBackend.swift uses a dedicated save gate before reading ordinary Save's current path,
then captures state AND history head under model lock. I/O releases model lock while save gate stays
held. Successful publication records capturedHead, so later edits remain dirty. Conflict leaves
path/title/savedHead/engine path map unchanged; pending edit may still be committed into local history.
Legacy save/saveAs share ordering and unique staging, with replacement semantics unchanged. PSD/PSB
remains unsupported on stub. Per-call internal saveForTesting hooks supply barriers without globals.

New DocumentSaveDestinationCommit.swift reserves same-directory unique stage using mkstemp, writes
all bytes, fsyncs and retains the descriptor through publication/cleanup to pin the inode. Create uses
renamex_np(RENAME_EXCL); only ENOSYS/EINVAL falls back to link. EEXIST maps to destinationExists;
other errors throw. Replace uses replacing rename. There is no check-then-replace or replacing fallback.
The SDK declaration/RENAME_EXCL constant was read, not compiled on B.

Stage-name ownership is relinquished after successful rename or the single unlink attempt. Cleanup
compares recorded device/inode before removal and rejects an observed reused/replaced name. It never
retries old names after success or cleanup failure. Failed cleanup or descriptor close after publication
is logged, while actual publication remains Saved. Conflict/error retains its primary outcome even
when cleanup also fails. A must inspect logs/retained stages; no universal stage-cleanup claim.
This does not establish atomic conditional unlink against an actively hostile parent-directory writer
between identity read and unlink; randomized owned staging and ordinary destination races are the
scope. Do not reinterpret these checks as filesystem-wide identity-CAS or a sandbox boundary.

## Tests and gates

All new/modified tests UNRUN on B; test commits require the new API to compile and are not claimed
baseline behavioral RED. A should establish the regression oracle with its permitted minimal seam.

- EngineDocumentBackendTests: real tiny session, saved/conflict mapping, confirmed Replace and
  missing-parent error; no localization/string matching.
- DocumentSaveDestinationCommitTests: sentinel bytes/inode and markers, new-name retry, legacy and
  confirmed replacement, PSD unsupported, late appearance barrier, existing directory/dangling link,
  two independent same-name writers with distinct live stages and exactly one winner, valid reopen,
  captured-head later edit, ordinary Save path resolved after waiting for Save As, forced unsupported
  exclusive rename plus link publication/cleanup failure, observed staging-name replacement rejection,
  successful rename followed by foreign old-name reuse, external destination edit after confirmed Replace.
- DocumentSaveSettlementTests: existing presenter/outcome tests migrate to checked writer seam, while
  headless legacy test stays on Void writer. Added late appearance without intent upgrade, native join
  before write, actual Replace response+detach grants replacement, conflict after admitted cancellation,
  no Saved/folder/path/title/dirty advance, no auto-Replace, no legacy then callback, newer-request status.
- Source checks: git diff --check passed. Opening/load/status/activation region and legacy direct
  write/export/close region compared byte-identical with81cc08eb. Explicit diff confirms native presenter,
  DocumentSheets, ContentView, AppModel, Rust and generated sources untouched.

A required next: compile against exact generated/archive checkpoint, establish meaningful regression
checks, run focused backend/stub/settlement and adjacent/full gate, then actual GUI collision and
confirmed Replace in isolated disposable output. Verify actual macOS directory/symlink errno and
unsupported-exclusive-rename fallback behavior; fake errno coverage is not native filesystem proof.
Retain any compilation/runtime failures. No broad performance, leak, arbitrary-filesystem, directory
fsync/power-loss durability, global Quit or external identity-CAS claim. B workload/heartbeat hold intact.
