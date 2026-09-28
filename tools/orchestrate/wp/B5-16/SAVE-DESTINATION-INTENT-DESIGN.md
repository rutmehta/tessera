# Additive Save As destination intent — design only

Request636b1603-fdb3-4e38-a002-4c69476be539. Main plan read from origin/main737dcd09,
`docs/coordination/SAVE-DESTINATION-RACE-PLAN.md`; UI source93244f8f read separately.
No product changes, compiler, tests, apps, benchmarks or heartbeat restart on B.
This is independent output-safety work; native presenter93244f8f remains unaccepted under A review.

## Contract and minimum API

Keep both legacy methods unchanged in signature and replacement behavior:
`DocumentSession.save_as(path: String) -> Result<()>`, `DocumentBackend.saveAs(path: String) throws`.
Ordinary `save()` also keeps its established-path replacing semantics. Do not infer checked intent
inside the backend, add identity-CAS to Replace, or disable explicitly confirmed replacement.

Proposed additive Rust/UniFFI surface in crates/tessera-ffi/src/document.rs:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DocumentSaveDestinationIntent { CreateIfAbsent, ReplaceConfirmed }
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DocumentSaveAsResult { Saved, DestinationExists }
// Exported alongside existing save_as; existing BridgeError remains unchanged.
pub fn save_as_checked(&self, path: String, intent: DocumentSaveDestinationIntent)
    -> Result<DocumentSaveAsResult>;
```

A destination collision is a typed non-success return, not BridgeError.Failure text. Other errors
remain existing Result/BridgeError failures. This avoids adding a global BridgeError case that would
force unrelated exhaustive switches to change. UniFFI generated names must be verified by A's
normal generator, not hand-guessed or edited. Both enums are immutable values across FFI.

Proposed Swift model/API in apps/mac/Sources/TesseraCore/Document/DocumentBackend.swift:

```swift
public enum DocSaveDestinationIntent: Equatable, Sendable {
    case createIfAbsent, replaceConfirmed
}
public enum DocSaveAsResult: Equatable, Sendable {
    case saved, destinationExists
}
// New protocol requirement; legacy saveAs(path:) remains unchanged.
func saveAs(path: String, intent: DocSaveDestinationIntent) throws -> DocSaveAsResult
```

EngineDocumentBackend explicitly maps the two enums around `session.saveAsChecked`; `bridged`
continues mapping only actual BridgeError failures. Do not discard DestinationExists by returning
Void. Stub implements the checked contract, including an actual atomic create-if-absent commit;
there must be no default protocol implementation that forwards checked intent to legacy saveAs.
Current concrete conformers are EngineDocumentBackend and StubDocumentBackend; A must compile all
test conformers after adding the requirement. A separate optional capability protocol would allow
unsupported backends, but adds dispatch complexity and is unnecessary for these two known backends.

## UI intent capture and real completion

At93244f8f DocumentWorkspace.swift:487–496, finishSaveAs still decides whether a Replace prompt is
needed. This check is UI policy only, not overwrite protection. Store the requested destination and
an optional intent on the matching DocumentSaveOperation:

1. If the path appears absent when the user submits Save, latch `.createIfAbsent` for that request.
   It remains CreateIfAbsent through form native drain and all encoding; never upgrade it based on
   a later exists check. Broken symlink may look absent to fileExists; atomic commit still rejects it.
2. If it exists, intent remains unset during form drain and while Replace is pending.
3. Only the actual matching Replace invocation's affirmative completion can latch `.replaceConfirmed`.
   Admission still waits for completion AND exact parent membership clearance. Cancel, stale response,
   duplicate response, wrong request/controller and mere file existence grant no Replace intent.
4. ReplaceConfirmed authorizes replacement of that destination path, including if another process
   changed the file or removed/recreated it after confirmation. No identity token/CAS requirement.
5. A CreateIfAbsent collision never automatically opens Replace or upgrades intent. It ends this save
   with a visible conflict; the user can invoke Save As again and explicitly choose Replace or a new name.

Preserve native presentation state machine/tokens; no DocumentSavePresenter or DocumentSheets change
is necessary for this output slice. Workspace's actual write call carries `intent`, not an inferred
Boolean. Existing legacy headless Save As, direct `write(doc,to:)`, self-tests, and PSD-copy operation
retain legacy entry points until separately migrated by their callers.

For typed completion, add internal `DocumentSaveWriteResult { saved, destinationExists(URL) }` and a
checked writer seam receiving `(document, url, intent, completion: Result<DocSaveAsResult, Error>)`.
Keep the existing Void writer seam for ordinary/legacy tests; checked UI writes MUST use the new
seam/real checked backend and cannot fall back to the legacy seam. Convert ordinary Void success to
internal `.saved` in the common settlement helper. This makes the test boundary match real production.

Add `DocumentSaveOutcome.destinationConflict(URL)` to the preparation-facing terminal result. On
checked DestinationExists, settle that typed result exactly once; latest request alone publishes
“Save: A file appeared at this destination. Choose another name or confirm Replace.” Do not publish
Saved, update lastSaveFolder, reload model as if saved, advance a path/title/saved marker, execute legacy
`then`, or authorize close continuation. A late conflict still remains conflict if cancellation marked
a running writer; never translate it into `.saved(continuationCancelled: true)` or a synthetic cancel.
Actual successful admitted writes keep the existing continuationCancelled semantics.

Known workspace wrappers use `if case .saved(...false)` and remain fail-closed. Search all current A
preparation/recovery callers for exhaustive DocumentSaveOutcome handling before integration; if A has
new callers, A owns their explicit conflict->hold/retry mapping. No AppModel/global Quit changes in B's
scope and no string matching to recover the conflict type.

## Native path and marker discipline

Current main paths:
- document.rs:2112 save_as holds Shared::saving through snapshot/write/path update; :2174
  save_snapshot commits pending edits then snapshots `(doc,node)`, io::save, then saved_node.
- document/io.rs:338 write_atomic uses unique NamedTempFile and replacing persist; :351 save dispatch.
- compositor/src/format.rs:395 to_bytes is public; :420 save uses fixed `*.tessera-doc.tmp`.

Introduce private `save_snapshot_with_mode(path, mode) -> Result<DocumentSaveAsResult>`. Hold the same
Shared::saving lock for checked and legacy entry points. Snapshot/encoding occurs outside model lock
as today. Checked conflict returns BEFORE saved_node, path, title and Engine.documents.by_key update.
On Saved only, record the captured node (not a later history head), then update path/title/registry by
existing save_as sequence. Committing a pending edit into history for the snapshot may still occur
before a conflict; do not claim conflict rolls back edits/history or restores earlier rendering epochs.
It must not falsely mark the document saved. Existing post-commit model-lock poison/close failures are
not destination conflicts and must not be mislabeled; do not widen this slice into session-close redesign.

Use io.rs internal `CommitMode { Replace, CreateIfAbsent }`. Legacy save/io::save maps to Replace;
checked ReplaceConfirmed maps to Replace; checked CreateIfAbsent maps to CreateIfAbsent. The private
Replace mode does not imply a user confirmation for legacy callers. Unify document-session output:
- Native: `compositor::format::to_bytes(doc.state())`, then the shared staged commit helper.
- PSD/PSB: existing conversion/version checks/psd.write, then the SAME helper.
This avoids the native fixed temporary name for all DocumentSession saves while preserving ordinary
save/legacy path replacement. compositor::format::save's unrelated callers remain outside this slice;
its public serializer is reused without changing the file format or copying encoder internals.
Do not change io.rs save_psd_copy_checked/write_copy_atomic or its cancellation/commit semantics.

## Atomic commit and explicit cleanup policy

All modes create a unique NamedTempFile in the destination parent, write complete bytes and sync the
staged file before publication. No deterministic `.tessera-doc.tmp`, pre-emptive destination deletion,
exists-then-rename, or fallback from no-clobber to replacement. Directory fsync/power-loss durability,
hostile parent-directory rename attacks and arbitrary remote filesystem behavior are not claimed.

Main plan proposes persist_noclobber. Pinned Cargo.lock tempfile3.27.0 and its Unix implementation
(src/file/imp/unix.rs:94–140) were read: it tries NOREPLACE rename; unsupported forms fall back to
hard_link then unlink, and that unlink error is explicitly ignored. Therefore “persist_noclobber
succeeded” alone cannot justify “the staging filename was removed.” This matters for truthful cleanup
acceptance, not destination no-clobber correctness.

Concrete narrow option recommended for A review: retain the NamedTempFile owner and use
`std::fs::hard_link(staged.path(), destination)` as the CreateIfAbsent publication point on supported
local macOS filesystems. Destination creation is a single no-clobber filesystem operation; a preexisting
entry (including dangling symlink) must not be followed/replaced. On link success, call staged.close()
to explicitly unlink the staging name and observe cleanup errors; on link failure close the stage and
preserve the primary error. Unsupported hard links fail closed, never fall back to overwriting rename.
Replace uses NamedTempFile.persist, which atomically replaces the path using its existing semantics.
This avoids depending on tempfile's silent-success unlink fallback and requires no new dependency.

Native owner A should approve this hard-link-only admission choice or retain persist_noclobber with an
equally explicit cleanup policy before implementation. Do not silently promise both cleanup certainty
and tempfile's current ignored-unlink behavior. Success with failed post-publication staging unlink is
still a committed save, with an explicit cleanup diagnostic (structured native log/test hook containing
stage path and error); never report conflict/failure implying destination was untouched. This is not a
rollback opportunity. Session markers must reflect the committed write. A must decide user-visible
cleanup warning handling if needed; the minimum API can keep it diagnostic rather than adding another
UI failure case. Tests must preserve and expose any retained stage, never count cleanup as complete.

On no-clobber commit failure, map the OS destination-exists error to DestinationExists before any
BridgeError string conversion. Other I/O errors remain errors. Validate actual errno classification for
regular file, symlink and directory on A; do not use a later exists check or error text to invent conflict.
Pre-commit cleanup failure must not replace a primary typed destination conflict; report both through
the conflict plus cleanup diagnostic. Exact temp ownership is required; never remove a destination
or arbitrary matching filename during cleanup.

## Stub parity

StubDocumentBackend.swift:912 currently encodes JSON with Data.write(.atomic), which replaces, then
sets savedHead to the current head after I/O. The new checked method needs a real unique staged commit
helper, proposed `apps/mac/Sources/TesseraCore/Document/DocumentSaveDestinationCommit.swift`.
Use Darwin mkstemp in destination parent (exclusive unique file), robust write/fsync/close, atomic
link-to-destination for CreateIfAbsent, and rename for ReplaceConfirmed, with the same cleanup/result
policy. Existing Foundation `.atomic` alone does not implement no-clobber. No dummy in-memory exists
flag in the production stub. PSD/PSB stays unsupported on stub; do not claim engine-format parity.

Serialize stub saves with a dedicated save lock, separate from its model lock. Share this ordering
with legacy save/saveAs without changing their overwrite policy. Snapshot `(state, capturedHead)` under
model lock after pending edit commit, release for encoding/I/O, and update savedHead only to that
capturedHead after successful publication. This avoids a later concurrent edit being incorrectly marked
saved. On conflict keep path/title/savedHead/engine map unchanged. Check lock ordering: never take the
save lock while holding the model lock; legacy save resolves path then enters the common save path,
without recursively locking. This fixes snapshot consistency within the touched save path only.

## Files and ownership proposal

| Files | Planned change | Owner / gate |
| --- | --- | --- |
| crates/tessera-ffi/src/document.rs | Add enums/exported checked method; shared save lock/snapshot mode; markers only after Saved | A native owner, review before edits |
| crates/tessera-ffi/src/document/io.rs | Shared native+PSD staging, typed commit outcome, no-clobber/replace mode and test-only commit barriers | A native owner |
| crates/compositor/src/format.rs | Read/reuse public to_bytes; no change needed in minimum slice | Remains A/shared |
| apps/mac/Sources/TesseraFFI/TesseraFFI.swift and CTesseraFFI/CTesseraFFI.h, module.modulemap | A regenerates exact checked API bindings/archive with apps/mac/build-ffi.sh; verify actual tracked generator paths | A compiler owner; no manual bindings |
| apps/mac/Sources/TesseraCore/Document/DocumentBackend.swift | Add intent/result types and checked overload | B source after authorization |
| apps/mac/Sources/TesseraCore/Document/EngineDocumentBackend.swift | Explicit typed mappings, no message parsing | B source after generated API checkpoint |
| apps/mac/Sources/TesseraCore/Document/StubDocumentBackend.swift and proposed DocumentSaveDestinationCommit.swift | Real staged no-clobber stub, save ordering/captured head | B source after contract approval |
| apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift | Optional intent until actual confirmation; checked completion seam, typed terminal conflict/status | B; wait for presenter acceptance and use exact accepted baseline |
| crates/tessera-ffi/tests/document.rs plus private io.rs unit tests | Session markers/roundtrip/commit races; tiny barriers | A runs |
| apps/mac/Tests/TesseraCoreTests/{EngineDocumentBackendTests,DocumentModeTests,DocumentSaveSettlementTests}.swift; new DocumentSaveDestinationCommitTests.swift | Mapping, stub markers/commit and UI outcome tests | B source; A runs |

Verify generated Swift filename against build script at implementation time; do not add a second copied
binding. AppModel, DocumentSheets, ContentView, native presenter and PSD-copy APIs are excluded. If A
needs preparation caller wiring beyond Workspace, coordinate it as an A-owned dependency.

## Tiny acceptance sequence (proposed, all UNRUN)

1. Native helper RED: private per-call commit barrier after bytes/sync, never a global mutable hook.
   Insert sentinel while held, then resume CreateIfAbsent for native/PSD/PSB. Typed conflict, unchanged
   sentinel bytes and file identity, no marker/path/title/map advance, no owned stage remaining on normal
   cleanup. Reopen a subsequent successful new-name save. Keep dirty document nontrivial but tiny (e.g.4x4).
2. Two independent sessions held at the same commit boundary target one initially absent filename.
   Release both without sleeps: exactly one Saved, one DestinationExists, distinct staging identities,
   valid winner reopens; repeat native, PSD and PSB. Shared session lock is not mistaken for cross-session
   exclusion. A may use a separate-process sentinel writer for the race; no big image/GPU needed.
3. Existing regular file, dangling/regular symlink and directory: CreateIfAbsent leaves entry/referent
   untouched. Permission/encoding/write/sync failure stays an ordinary error with no Saved marker.
   Inject link failure and cleanup failure separately; validate no false pre-commit/committed claim.
4. ReplaceConfirmed control: after affirmative intent, externally alter destination before commit;
   replacement STILL succeeds by path convention. Also succeeds if target disappears before commit.
   Legacy saveAs/save keep replacing; no identity-CAS or disabled Replace regression.
5. Stub checked real-file parity, cleanup and captured-head regression: hold I/O, make a later edit,
   complete save; saved snapshot is recorded and later edit remains dirty. Stub PSD unsupported unchanged.
6. Swift mapping tests distinguish Saved, typed DestinationExists, generic BridgeError; no localized
   string matching. UI form absent->checked CreateIfAbsent; existing->no intent until matching actual
   Replace approval and native drain. Stale/cancelled Replace does not write. After late destination
   conflict: exactly-once typed outcome, visible conflict for latest request, no Saved/folder advance,
   no continuation; retry new name then real Replace succeed. Held writer cancellation reports actual
   conflict or actual Saved(continuationCancelled) as appropriate, never optimistic settlement.
7. A regenerates FFI and compiles focused/adjacent/full before isolated real GUI absent-name race and
   confirmed Replace. Existing presenter tests/GUI gates stay separate and remain mandatory.

Implementation requires A design approval and native ownership/API checkpoint; this document does not
claim code, tests, native contract probe or Save As product acceptance. B resource hold remains.
