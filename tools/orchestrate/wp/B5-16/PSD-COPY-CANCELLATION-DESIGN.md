# Rasterized PSD copy: exact operation contract (proposal)

Request 92b23b0f-1a4e-47c2-9aae-781b889439a1, 2026-09-27. Source reviewed at
B 755ac31 and A main b40c47b6. The inspected transform caller, FFI copy,
IO writer, compositor PSD adapter and PSD encoder have no diff between these
heads. Accepted receipt published before review. This document proposes an API;
no product implementation, generated bindings, tests or workload was run on B.
A reviews the contract before implementation and owns compilation/main merges.

## Current behavior and exact gaps

- `DocumentTransforms.swift:532` presents NSSavePanel, then enqueues a synchronous
  backend call on the singleton's serial userInteractive transform queue.
  `enqueue` captures the backend strongly, has no cancellation handle and reports
  completion through the current document, which can differ from the origin.
  Dismissing the save panel prevents enqueue; closing a document after enqueue
  does not cancel this copy. Swift Task cancellation cannot interrupt this call.
- `DocumentTransformsBackend.swift:105,221` exposes only
  `savePSDRasterizingTransforms(path:)`; the adapter invokes UniFFI session save.
- `document/transform.rs:1093` snapshots committed `st.doc.state()` under the
  state mutex after open validation, then releases it. It rasterizes enabled
  smart-object stacks sequentially in a private copy, preserving source path,
  dirty state and history. Preserve this committed-snapshot semantic; do not
  silently include scratch edits or convert this action into Save As.
- `filters.rs:rasterize_smart_stack` passes None to native_stack, creating a
  fresh request cancellation source without a host owner. Full RGBA conversion
  through `document.rs:raster_from_rgba` has no cancellation checks.
- `document.rs:shutdown` currently locks state to mark closed before stopping
  renderer/filters and joining the render worker. Copy work is neither registered
  nor joined. A retained backend/UniFFI Arc can outlive the controller. Drop alone
  cannot replace explicit close cancellation.
- `io.rs:348` calls `compositor::psd::to_psd`, then `PsdDocument::write`, then
  `write_atomic`. The adapter (`compositor/src/psd.rs:export_imported`) renders
  the merged composite with legacy `render_level_rgba`, a fresh uncancellable
  caller route, even after each smart layer was rasterized. It assembles full
  channel planes. The encoder (`psd/src/lib.rs:136`, `layers.rs:222`) builds
  section/channel/compressed vectors and a complete output Vec in memory.
- `io.rs:336` creates NamedTempFile beside the destination, writes all bytes,
  syncs the temporary file, then calls persist(path). There is no earlier direct
  truncation/write to the existing destination in this path. The irreversible
  replacement point is successful persist. No directory fsync is present: do
  not claim crash-durable replacement. Normal RAII drops an unpersisted temp;
  ensure PersistError's owned tempfile is dropped on failure too.

## Minimal exported API

Proposed Rust UniFFI surface (names are exact proposals, not existing symbols):

```rust
#[derive(uniffi::Object)]
pub struct RasterizedPsdCopyOperation { /* private state */ }
#[derive(uniffi::Enum)]
pub enum RasterizedPsdCopyOutcome { Saved, Cancelled }

// DocumentSession exported method; cheap registration, no snapshot/render/IO.
pub fn prepare_rasterized_psd_copy(&self)
    -> Result<Arc<RasterizedPsdCopyOperation>>;

// Operation exported methods.
pub fn cancel(&self) -> bool;
pub fn run(&self, path: String) -> Result<RasterizedPsdCopyOutcome>;
```

`cancel()` returns true when cancellation is accepted (also true if already
cancelled); false after commit admission or terminal success/failure. It never
locks Shared.state, waits for rendering, performs IO or waits for run().
`run()` is single-use; repeated/concurrent calls return a regular bridge Failure.
Cancellation is an outcome, not a string-matched BridgeError: today's bridge has
only Failure and erases EngineError variants. Real evaluation/IO failures remain
errors, including persist failure after commit admission. A pre-cancelled run
returns Cancelled before snapshot, layer evaluation or file creation. A malformed
path on an uncancelled run errors before evaluation.

Private ownership: operation retains Arc<Shared> and one Arc request source
(native CancellationToken plus live effect AtomicBool). Shared owns a dedicated
small copy registry, with a closed latch and Weak operation entries. No strong
cycle; registry prunes dead entries during prepare/close/completion. Every copy
gets a unique identity. Weak entry removal must compare identity, never blindly
clear an active slot. A terminal RAII guard unregisters on all run exits; dropped
unrun handles are pruned, and close cancels queued handles too. One simultaneous
copy per document is admitted; prepare returns Busy as a regular Failure for a
live nonterminal copy, so repeated clicks cannot accumulate snapshots. A fresh
copy after Cancelled/Saved/Failed is independent.

State machine: Prepared -> Running -> Committing -> Saved or Failed;
Prepared/Running -> Cancelled; precommit errors -> Failed. A separate single-use
run-claimed bit prevents a cancelled handle from being run repeatedly. The
phase transition and cancellation-source signal use the same small registry
mutex, never the document state mutex. Store phase in an operation-private cell
accessed only under that registry mutex; expose no mutable host state.

At shutdown entry, BEFORE Shared.state locking, acquire copy registry, latch
closed, cancel all Prepared/Running operations and signal both cancellation
channels; release registry, then do existing shutdown. Prepare and commit
admission use this same gate, rejecting closed sessions. Never nest registry
and state locks. After taking a document snapshot, recheck cancellation. Thus
close racing snapshot cannot allow an unadmitted copy to commit. A close that
arrives after Committing cannot revoke that admitted replacement; this exception
must be documented and tested. Close does not join the copy or promise all its
allocations have already been released. Existing renderer join latency is a
separate issue.

Under the registry gate, the final pre-persist check transitions Running to
Committing only if not cancelled/closed. Release the gate BEFORE persist IO.
Cancellation arriving later returns false, and run reports actual Saved or IO
failure, never Cancelled after replacement. This is a deliberate linearization
point immediately before rename, not a claim that rename itself is cancellable.

Keep legacy save_psd_rasterizing_transforms as a compatibility wrapper using
prepare/run; map Cancelled to the existing Failure only for legacy callers.
Migrate the actual UI to the typed operation result. Regenerate UniFFI Swift
bindings on A; validate generated object Sendable behavior before compiling the
host adapter. Do not introduce an unreviewed blanket unchecked Sendable wrapper.

## Cancellation plumbing and output transaction

1. Validate PSD/PSB kind and canvas size before expensive work. Snapshot on run,
   retaining current committed-state semantics. Check token before/after lock,
   tree traversal, each layer, native evaluation and raster conversion. Pass the
   SAME request source into native_stack; add a copy-specific cancellable raster
   conversion helper checking per tile/row, preserving existing callers.
2. Proposed FFI helper `save_psd_copy_with_operation(doc, path, operation)` keeps
   the regular save path unchanged. Check before/after to_psd and encoder calls.
   First bounded slice has conversion/encoder BOUNDARY cancellation only. It
   must explicitly retain the uncancellable merged-composite render gap below.
3. After encoding, check before creating a temp. Write encoded bytes in bounded
   chunks with checks between writes; check before/after sync_all. No lock held
   during write/sync. Neither a blocking write nor sync syscall is preemptible.
4. Gate commit admission as above; persist only admitted output. On cancellation
   or failure drop temp/bytes/private snapshot. Never delete/truncate destination
   to implement cancellation. Never check cancellation after successful persist
   and turn Saved into Cancelled. Source document remains untouched on every path.

A-owned follow-up needed for useful cancellation through PSD conversion:
add a reviewed cancellable PSD export adapter that passes caller token into the
merged-composite `render_level_rgba_with_cancel` and checks export_nodes/channel
assembly. Do not change PsdExport's public trait casually or assert the boundary
wrapper interrupts it. PSD encoder internals need their own token-neutral check
callback or reviewed API to check between layers/channels/compression blocks;
that is a separate design/implementation slice. Existing specialized filter
adapters may likewise only check at boundaries. No claim of bounded cancellation
latency until those gaps are closed.

## Swift ownership and visible behavior

Add a Sendable operation protocol in TesseraCore with synchronous
`cancel() -> Bool` and `run(path:) throws -> RasterizedPSDCopyOutcome`, plus a
backend `prepareRasterizedPSDCopy()` method. The adapter owns the generated
UniFFI object. Existing protocol mocks need explicit conformances, not a fallback
that silently runs the uncancellable method.

After save-panel OK, prepare and retain the operation on MainActor BEFORE
queue.async. Record origin document identity and an independent request UUID;
weakly capture the controller for completion. Add a real Cancel Copy action
associated with that operation. Cancel calls the handle immediately on MainActor,
never enqueues behind run(). Clear the UI owner only on matching UUID. Completion
must not report to whichever document became current; source close removes the
UI owner and backend close signals Rust. A stale completion still balances busy
but cannot clear/report into a newer request. Panel callback must reject a closed
origin; Rust prepare/open checks remain authoritative.

For this slice retain the existing serial queue (no new thread/polling timer),
but explicitly record that it blocks transform work and is userInteractive QoS.
A separate userInitiated output queue is a possible later scheduling change,
requiring resource admission; do not combine it with cancellation. Show “Cancelling
copy” while accepted cancellation is draining an opaque encoder, and report
“Saved” if cancellation was too late and persist succeeds. Switching tabs alone
need not cancel an intentional copy; closing its origin must request cancel.

## Tiny deterministic acceptance plan (UNIMPLEMENTED / UNRUN)

Use 2x2 or 3x2 fixtures, two smart layers and channel/barrier gates with watchdogs;
no sleeps, timing stress, large PSDs or GPU. A owns execution.

1. Prepare -> cancel -> run: Cancelled, zero evaluator/snapshot/write calls;
   second run rejected. Fresh handle succeeds and produces parseable tiny PSD.
2. Gate after first layer, cancel from another thread while test holds the
   backend state lock, release gate: cancellation returns without that lock,
   second layer never evaluated; no destination/temp/source state change.
3. Repeat gate with session close: observe close's copy signal before releasing
   backend lock (do not require whole close to return while lock held). Queued
   operation cancels too; post-close prepare rejected; no precommit publication.
4. Existing destination contains sentinel bytes; cancel at before-conversion,
   after-encoder and after-temp-sync gates: exact bytes preserved, temp removed.
   Inject write/sync/persist errors and verify cleanup and error outcome.
5. Cancel wins commit gate: no persist, Cancelled. Commit admission wins: cancel
   returns false, persist succeeds -> Saved even if close follows. Inject persist
   failure -> error. Test both schedules, not stochastic races.
6. Cancelled operation cleanup delayed until replacement owner is registered:
   old identity cannot unregister/cancel replacement. Retry succeeds and source
   path/history/dirty state are identical before/after copy.
7. Swift fake operation/serial queue gate: cancel before dequeue, origin close,
   tab switch, stale completion, matching cleanup and busy balance. Assert actual
   cancel invocation and outcome routing, not only hidden UI state.

A must also compile regenerated bindings and existing protocol conformers, then
run focused legacy PSD-save parity tests. No test is claimed passed here.

## Remaining resource risk and review decision

Full native RGBA, transformed rasters retained across layers, merged-composite
RGBA, channel planes, compression buffers and encoded output can coexist. Their
allocations are not made safe by a 512 MiB image-cache cap. Allocation itself,
legacy PSD conversion/composite rendering, encoder internals and blocking file
IO remain nonpreemptible in the first slice. Checked size arithmetic/peak memory
admission and streaming output remain separate work; cancellation is not a peak
memory budget. There are no irreversible destination writes before persist in
this inspected path, but post-admission replacement cannot be rolled back.

Requested A decision: approve handle/outcome/commit semantics and explicitly
choose boundary-only first slice versus requiring cancellable compositor PSD
conversion in the first implementation. B workload hold and paused heartbeat
remain in effect. No product edits are authorized by this design publication.
