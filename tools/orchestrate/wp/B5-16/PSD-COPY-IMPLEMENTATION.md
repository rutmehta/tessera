# PSD operation / host source candidate — request 547433be

2026-09-27. Exact B-target request validated against main4090aeb7 and local
resource hold; accepted Git receipt before source edits. Builds, generated
bindings, tests, apps, benchmarks and heartbeat remain prohibited on B.

## Implemented source

- New document/psd_copy.rs exports RasterizedPsdCopyOperation prepare/cancel/run
  and RasterizedPsdCopyOutcome Saved/Cancelled. Committed snapshot taken on run;
  single-use handle, private request source, weak per-document admission registry.
- Cancellation-requested is independent from Prepared/Running/Committing/Finished.
  Running cancellation keeps admission until run_inner returns and drops its
  snapshot/encoder/output locals. Prepared cancellation permits replacement but
  old run is permanently pre-cancelled. RAII cleanup compares pointer identity.
- Shutdown closes the copy registry before state locking or render-worker join.
  No lifecycle/registry locks span state locking, evaluation, copying, IO or host
  callbacks. Lock order is registry then lifecycle where both are needed; finish
  releases lifecycle before taking registry. Cancel takes only lifecycle and
  signals the live native/effect request pair.
- Each smart layer receives that request pair through native_stack. Raster
  conversion checks per tile/row. Tree traversal/layer loop/snapshot boundaries
  also check cancellation. Compatibility save_psd_rasterizing_transforms prepares
  and runs a handle, mapping Cancelled to legacy Failure.
- Copy-only IO helper checks before/after PSD conversion/encoding, writes a
  same-directory temp in 64 KiB chunks, syncs, then admits commit. Persist runs
  unlocked. Cancellation after admission is rejected; Saved/error reports actual
  replacement outcome. Ordinary save/export paths remain unchanged.
- Swift protocol/adapter retains the generated operation. MainActor slot remains
  occupied while cancel drains. Save-panel callback prepares before enqueue,
  completion checks request UUID and original document, and generic enqueue
  suppresses its current-document error routing for this path. Document close
  cancels/removes host ownership; late callbacks still balance busy but cannot
  report to a closed/new document. A narrow File-menu Cancel Rasterized PSD Copy
  action calls cancel immediately. No queue/thread/scheduling change.

## Mandatory A follow-up and remaining limitations

UniFFI bindings were deliberately NOT generated. The checked-in generated Swift
cannot yet contain the new API; A must regenerate before compilation. Existing
bindings declare generated object protocols Sendable and implementations unchecked
Sendable; the new Swift adapter relies on that generator contract without adding
an unchecked production wrapper. Verify the actual newly generated class.

Narrow compositor handoff: io::save_psd_copy_checked currently calls
compositor::psd::to_psd(doc) with boundary checks. A should add its cancellable
conversion entry point here and thread the SAME state.cancel native token from
psd_copy::run_inner. RequestCancellation.native is currently private to filters;
add a narrow borrowed-token accessor when wiring the new conversion API. Do not
create another fresh token or replace live cancellation with a bool snapshot.
No compositor, engine-api or render.rs changes in this candidate.

Legacy PSD conversion still includes uncancellable merged-composite rendering
and channel assembly. PsdDocument::write still allocates/encodes opaque sections
and full output. Boundary cancellation is not encoder interruption, bounded
latency or a peak-memory limit. Specialized native adapters may retain their own
boundary-only checks. Close signals immediately but does not join the copy and
may still wait for the existing render worker. Commit-admitted output can finish
replacing a file after close. No directory fsync/crash-durability claim.

The queue remains the existing serial userInteractive transform queue; the copy
can still delay unrelated transform commands until it drains. Admission is per
document; the host queue serializes copies, but external callers on different
sessions are not globally memory-admitted. Shared-shell touch is restricted to
the two copy menu actions in AppCommands; A must preserve its current shell work
when applying this separate candidate. Outline/viewport/telemetry files untouched.

## Tests added — ALL UNRUN

Rust private psd_copy::tests (4): cancellation in first-layer callback skips next
layer and preserves original Arc tree; pre-cancel/single-use/late cleanup and
replacement; channel-gated running-cancel rejects second prepare until RAII unwind;
close-versus-commit ordering. Fixtures 2x2, no GPU.

Rust io::copy_transaction_tests (3): cancelled commit preserves sentinel/removes
temp then retry writes; cancellation between two chunks removes temp/preserves
sentinel; no check after commit and persist failure remains failure/cleans temp.
Largest synthetic buffer 64 KiB + 1 byte.

Rust integration document_transform_ui::rasterized_copy_handle_precancel_retry_and_close
(1): real session 2x2, cancelled handle keeps sentinel, fresh handle writes PSD
signature, path/dirty/history unchanged, queued close cancellation preserves saved
bytes, post-close prepare rejected. This existing integration harness initializes
Engine and may initialize GPU on A; it is not part of the pure CPU unit subset.

Swift RasterizedPSDCopyTests (4): pre-worker cancel retains slot, stale close
completion cannot clear replacement, separate document slots/too-late cancellation,
and actual host queue gated fake close suppresses callback and balances busy.
The fake worker has a five-second watchdog. No host window/app launch needed.

These are source assertions, not passed acceptance. The layer-loop test injects
its raster result/cancellation; it does not exercise a real encoder interruption.
Direct close cancellation while holding the actual backend mutex, exhaustive
filesystem failures, fresh generated Swift compile, mocks, whole save parity and
end-to-end GUI validation remain A gates. Proposed commands should use filters
above in A's bounded compiler slot, not broad B execution.

## Source checks performed

Existing rustfmt binary formatted only changed Rust files (document.rs used
skip_children). git diff --check passed. Manual review checked lock ordering,
identity cleanup, retained running admission, commit outcome ordering and host
origin routing. No compiler, bindings generator or tests were invoked on B.
