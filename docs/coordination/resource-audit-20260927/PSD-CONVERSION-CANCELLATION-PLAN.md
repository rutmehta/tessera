# Cancellable PSD conversion companion (approved scope, unimplemented)

Machine A coordinator, 2026-09-27. B owns the separate operation/Swift handle
slice requested as `547433be`. This A-owned compositor slice must not modify
B's Document FFI or host files. No build or implementation is claimed here.

Add `compositor::psd::to_psd_with_cancel(document: &Document,
cancel: &CancellationToken) -> EngineResult<PsdDocument>` for the actual document
copy caller. Preserve existing generic `to_psd` and `PsdExport` source contracts;
no required method added to external trait implementations. Both paths should
share conversion internals, with fresh tokens only for compatibility callers.

Pass the live caller token into merged-composite
`render_level_rgba_with_cancel`, and through node traversal, raster/mask/channel
assembly. Check before large allocations or opaque copies, between layers and
channels, at row boundaries and bounded intervals within long rows. Check before
returning success. Cancellation returns EngineError::Cancelled and no partial
PsdDocument; original Document and retained opaque metadata stay unchanged.
Avoid a fresh token inside the cancellable path. Check size multiplication and
output capacity arithmetic before allocating channel/composite buffers.

This scope does not make allocation, retained PSD metadata cloning, compressor
internals or the encoded-output writer preemptible. Existing source/result/cache
budgets are not a bound on total export memory. The separate encoder and peak
working-memory admission work remain explicit; no latency or process-memory
acceptance follows from this API alone.

Use tiny native/imported document fixtures: uncancelled output parity with legacy
conversion for supported depths and retained metadata; pre-cancel before any
conversion; deterministic cancellation during node/row/channel work with no later
stage, followed by unaffected successful retry. Prefer narrow injected checkpoint
helpers in tests over sleeps or global mutable hooks. Include checked arithmetic
and existing PSD round-trip regressions. Freeze the exact source, preserve RED
and failures, run with two workers/watchdogs only when A's compiler slot is free.

B operation design approval includes one correction: accepting cancellation of a
running copy must retain per-document admission until that worker unwinds. An
opaque encoder must not overlap a newly admitted retry. Commit admission occurs
under the operation gate before unlocked persist; late cancellation reports the
actual save outcome and must never claim Cancelled after destination replacement.
