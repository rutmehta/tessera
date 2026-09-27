# Document CPU cancellation route — source-only plan

Request 7e842a6c-fb9e-4f68-b326-28674465fa6b. Reviewed compositor bridge
main0d627023 and kernel505c4c29; fetched main11fc339. The three inspected FFI
files (`document/filters.rs`, `render.rs`, `transform.rs`) have no diff between
B63a06a2 and fetched main. No caller implementation changes or B execution.

## Existing routes and gaps

| Route | Existing cancellation owner | Break in propagation |
| --- | --- | --- |
| Filter preview | `filters::Inner.running: Option<Arc<AtomicBool>>`; worker creates flag per preview; replacement/stop/cancel_filter sets it | `filtered` checks before/after `native_stack`; native_stack does not receive the flag. It prewarms with uncancellable direct render_tile, then calls legacy render_level_rgba. Expensive native transforms finish before stale result is discarded. |
| Destructive filter apply | `FilterState.apply_cancel` holds replaceable Arc<AtomicBool>; cancel_filter sets it | Some effect paths poll it, but this is not a compositor CancellationToken. Synchronous native paths need the same live cancellation source, not a copied initial bool. |
| Smart-stack bake | `bake` creates a local AtomicBool(false) | No external owner can set that local flag. Preview cancellation does not make this bake cancellable. Requires a registered per-bake request source. |
| Document viewport render | Signal has pending/busy/stop booleans; request coalesces flags; frame publishes only if surface generation still matches | No per-frame CancellationToken is installed. cpu_present uses direct render_tile. Generation rejection prevents stale publication, not computation. stop only wakes the worker; it cannot interrupt inner CPU work. |
| CPU readback | Renderer::read_level CPU and GPU-fallback call legacy render_level_rgba | Legacy wrapper creates a fresh token. No caller handle currently accepted. |
| Advanced transform Cancel | cancel_advanced_transform validates a numeric edit-session token and clears preview state | This numeric token identifies an edit session; it is not a cooperative cancellation token for active rendering. |
| Rasterized PSD copy (incident path) | Swift saveRasterizedPSD enqueues synchronous savePSDRasterizingTransforms; FFI snapshots layers, rasterizes each, saves copy | No per-operation cancellable handle in the Swift backend protocol/UniFFI call. Cancelling Swift work/result delivery cannot stop the synchronous Rust call. rasterize_smart_stack -> native_stack also takes no cancellation input. |

`render::Pressure.token` only owns the scheduler's pressure-hold job. Cancelling
it ends that bookkeeping hold, not document computation. Do not reuse it as if
it were the render request's token.

A's new `render_level_rgba_with_cancel` propagates a caller CancellationToken
through full-level CPU rendering and pixel assembly. Its legacy wrapper still
constructs a new token intentionally. `SmartFilterEvaluator::evaluate_with_cancel`
default checks before/after the adapter; adapters with long inner loops still
need a cooperative override. GPU submission/preemption is not established here.

## Proposed patches, in dependency order (review required before edits)

1. **Native preview/bake route and obsolete prewarm (B FFI, A review).** Introduce
   a request-owned cancellation source with a clonable compositor token and the
   existing effect AtomicBool. A single `cancel()` method updates both live
   channels; replace every direct flag store in the migrated route. All readers
   retain the same source for the request lifetime. Do not snapshot a bool into
   a fresh token, and do not add a polling helper thread. Alternatively A may
   extend engine-api to accept a shared external flag, but that is A-owned API
   work, not an implicit B change.

   Thread the token through `filtered -> native_stack ->
   render_level_rgba_with_cancel`. Remove the old direct-tile prewarm only on a
   base containing RES-01's serial first-touch/pass reuse; its old lock comment
   is obsolete there. Retaining an uncancellable prewarm defeats the new bridge.
   Register an independent active bake source and cancel it on supersession or
   shutdown. Keep generation/key checks at publication even with cancellation.
   Clearing old request state must compare identity so a late completion cannot
   unregister/cancel the next request. Preserve cancellation as a distinct
   outcome, rather than poisoning the current preview with a stale error.

2. **Viewport/readback request ownership (B document FFI, A compositor API).**
   Signal stores the active frame token; a new render request or stop cancels
   that token before the next frame starts. Create/replace it under the signal
   lock, release locks before expensive work, and pass its clone through CPU
   fallback/readback. cpu_present's region/direct-tile path currently lacks a
   public caller-token entry point; agree a cancellable region/tile API with A
   instead of falling back to a full-image render just to get cancellation.
   Do not cancel a frame for a layers-only notification without a render change.
   Keep surface generation checks and cancellation checks before publication.

3. **Rasterized-copy operation handle (B Swift/document FFI, separate API slice).**
   Add a per-operation handle allocated before synchronous work is enqueued, with
   a cancel method callable independently of the blocked worker and document
   render lock. Register it in the session with identity-safe cleanup; use a
   session-close parent token plus a child per operation. Thread through every
   layer rasterization and native stack, check between layers and before output
   publication. Swift exposes the handle to a real Cancel control; wrapping the
   same blocking call in Task.detached is not cancellation. Regenerate UniFFI
   bindings when the interface changes. Preserve source document state and the
   existing destination on cancellation: inspect the writer's temp-file/rename
   boundary before claiming output atomicity. Encoder-internal cancellation is
   a separate capability if the writer has no checkpoints.

Keep each route in a separate reviewable commit. No main merge, B workload or
heartbeat restart is authorized by this plan. Memory admission remains separate:
cancellation limits wasted work after a request, not peak memory of a live one.

## Deterministic bounded regressions required on A

- Already-cancelled token reaches native_stack before any evaluator/prewarm work;
  tiny two-tile smart object, zero attempted stages and Cancelled outcome.
- Gate a tiny evaluator at a known point, cancel via the real request handle,
  release the gate, assert no subsequent stage/publication and active counts
  return to zero. Use barriers/channels with watchdogs, no timing sleeps.
- Preview A superseded by B: A is cancelled; delayed A cleanup cannot remove B's
  handle or publish A's result/error. B completes. Repeat with session close.
- Bake supersession and shutdown use the registered bake source; they do not
  depend on a local false AtomicBool or preview generation alone.
- CPU region/render fallback receives caller token; cancellation after first
  tile stops subsequent tile work and skips presentation. Test direct-tile API
  separately when A exposes it; retain nested Rayon deadlock controls.
- Tiny rasterized PSD copy cancelled between two layers: no next-layer evaluation,
  no document mutation, existing destination bytes unchanged, no temporary-file
  residue after cleanup. Repeat success to show it was not simply disabled.
- Cancel handle allocated before enqueue works even if cancelled before the
  worker starts. New operation after cancelled operation is unaffected.

All tests above are requirements, not implemented or executed evidence. Run on
A in its available bounded slot, two workers, small fixtures and outer watchdog;
retain original failures. B resource hold remains.
