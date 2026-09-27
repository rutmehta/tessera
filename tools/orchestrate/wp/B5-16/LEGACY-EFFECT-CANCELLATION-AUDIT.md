# Legacy effect cancellation audit — ab6dceee

2026-09-27. Exact B target/expiry validated; accepted receipt before review.
Source: B0df02c4, current A status858fc594. Proposal only: no implementation,
bindings, tests or workloads. A owns preceding correction integration/compilation.

## Findings

- NativeFilterEvaluator::evaluate calls eval_stack -> Spec::run -> run_effect
  in crates/tessera-ffi/src/document/filters.rs.
- Effects already distinguish cancellation: filters::Filter::apply returns
  EngineResult<Raster> (crates/filters/src/lib.rs:149); checkpoint at158 returns
  EngineError::Cancelled. Effect::apply at317 checks cancellation, validates,
  reads, evaluates and writes. No public filters API change is needed.
- FFI run_effect at1014 declares filter_block as Result<Vec<f32>> (BridgeError).
  effect.apply(...)? therefore erases the variant through lib.rs:66. The scoped
  worker channel/receiver also carries BridgeError; the final cancellation check
  manufactures failure("cancelled"). Spec::run at224 similarly stringifies
  checkpoints and converts its adapter EngineResult. eval_stack at1170 carries
  the erased error back to NativeFilterEvaluator.
- Correction0df02c4 rightly preserves ambiguous legacy errors as failures. A later
  flag or error-string match cannot reliably recover the original type.
- Additional precedence gap: run_effect receiver uses msg? and stops at the
  first error. A cancellation result can hide a genuine failure from another
  already-running block. Scoped threads join, but later errors are not collected.

## Smallest proposed internal route

Keep implementation within FFI document/filters.rs. Add private FilterEvalError
with Cancelled and Failed(BridgeError), plus FilterEvalResult<T>. Do not couple
this general filter helper to the PSD CopyError module.

From<EngineError> matches Cancelled before conversion; all other engine errors
become Failed through today's bridge conversion. From<BridgeError> ALWAYS means
Failed. Provide explicit into_bridge and into_engine conversions:

- into_bridge maps Cancelled to existing failure("cancelled") and returns an
  original Failed payload unchanged, preserving legacy exported messages.
- into_engine maps Cancelled to EngineError::Cancelled and Failed to existing
  EngineError::invalid("smart filter", error.to_string()).

Change private run_effect, filter_block, worker channel/scoped closure, Spec::run
and eval_stack to the typed result. Spec::run has one caller (eval_stack);
run_effect has one caller (Spec::run). eval_stack has two production callers:
NativeFilterEvaluator uses into_engine, while filtered_with_cancel's cached
prefix loop uses into_bridge at that compatibility boundary. Exported BridgeError
and UniFFI APIs stay unchanged; no binding generation or engine/compositor change.

Explicit checkpoints emit typed Cancelled. Preserve the SAME borrowed AtomicBool
from RequestCancellation.effect through Spec::run/run_effect/effect.apply, paired
with the same RequestCancellation.native used by the compositor. No fresh token,
flag snapshot or polling. Genuine returned errors propagate before any subsequent
checkpoint. Pre-cancel may skip validation: errors never evaluated are not
observed failures. Add checks before expensive crop/raster/output clones, without
claiming allocation or opaque adapters are interruptible.

## Parallel result aggregation

Drain all messages and join scoped workers. Record first received genuine Failed
separately from saw_cancelled. After an error, discard successful payloads rather
than copying partial output. At completion prioritize observed Failure, then
Cancelled, then a final typed checkpoint, then success. Preserve first-arrival
selection among multiple genuine failures; do not claim deterministic error
selection between concurrently failing blocks.

Do not early-return on cancellation. An optional local stop-scheduling flag may
prevent new blocks but must not mutate the user's cancellation source or discard
results from already-running blocks. Initial bounded correction can retain
existing scheduling. Channel memory and global admission remain separate.

## Proposed tests / exact caller coverage — UNRUN

- New FFI private legacy_effect_cancellation_tests: 2x2 real Gaussian, pre-set live
  effect flag returns typed Cancelled; fresh request succeeds; assert variant
  through native evaluator, not just is_err.
- Gate a tiny effect callback after it has a genuine failure; cancel the same
  request before returning error. Failure survives typed/native conversion.
- Exercise production collector with Cancelled then Failed and reversed order;
  Failure wins both, all senders drain, success+cancel never publishes output.
- BridgeError containing literal "cancelled" remains Failed. Typed Cancelled
  converts to old BridgeError only at compatibility exit. Genuine payload stable.
- Malformed params/mask error already returned wins a racing cancellation.
- Existing FFI request_cancellation_tests (five) cover live dual source and
  supersession/close; image_cache_tests::source_and_prefix_cache_policy_preserves_tiny_filter_pixels
  covers cached-prefix compatibility; document_filters integration covers
  preview/bake/apply. Existing filters crate CPU/contracts tests supply kernel
  parity but do not substitute for these FFI variant assertions.
- Existing psd_copy::tests::only_typed_cancellation_maps_to_cancelled and
  genuine_failure_wins_cancel_between_work_and_finalization remain downstream
  gates. Use channels/watchdogs, no sleeps or large fixtures, A execution only.

Recommend one FFI-only typed-route/aggregation slice after A review. This is
error provenance/precedence, not whole-PSD interruption, peak memory containment
or incident closure. Mask assembly/preparation/adapter latency gaps remain.
A handles its two nested IO test-module path repairs, compositor token wiring,
compiler and main merges. B hold and paused heartbeat unchanged.
