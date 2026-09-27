# Typed legacy effect route — d20e6e9b

Source-only implementation after validating exact B target/expiry and publishing
accepted receipt. Product scope: document/filters.rs helpers/evaluator/tests only.
RequestCancellation struct/accessor unchanged; A owns its concurrent token wiring.

Private FilterEvalError distinguishes Cancelled from Failed(BridgeError), with
explicit EngineError variant conversion and native/legacy exit conversions.
Spec::run, run_effect (including block closure/channel) and eval_stack now retain
the type. NativeFilterEvaluator uses into_engine; cached-prefix compatibility
uses into_bridge. Existing exported BridgeError/API remains unchanged.

The same borrowed effect AtomicBool reaches effect.apply; the same request's
native token remains the compositor channel. Added typed checkpoints before
crop/raster/output clones and after successful work. Genuine returned errors
propagate before later checkpoints. Empty input succeeds when not cancelled;
pre-cancelled empty input now returns typed Cancelled before cloning.

Production collect_effect_results consumes all channel messages, preserving the
first received genuine failure over cancellation in either arrival order.
Successful payloads after an error are discarded. Earlier successful writes are
only private scratch; any final error prevents returning/publishing that image.
Scoped worker join and scheduling are unchanged; no new scheduling/global memory
promise. Already-running kernels can still take time to drain.

Six tiny tests added, ALL UNRUN:
1. Real 2x2 Gaussian pre-cancel/fresh success and empty-input contract.
2. Native evaluator with only effect flag set proves typed legacy cancellation
   reaches EngineError::Cancelled rather than stopping at native entry check.
3. Production collector drains both error orders, preserves first real failure,
   discards late successes and returns error instead of partial output.
4. Channel-gated genuine engine error remains failure after live dual-source
   cancel, through production collector/native conversion.
5. Real malformed mask failure stays failure after later cancellation.
6. Compatibility conversion preserves bridge failures even if text is
   'cancelled'; typed engine cancellation stays typed; invalid Gaussian radius
   failure is not reclassified after flag changes.

Tests use tiny images/synthetic results and five-second channel watchdogs.
Collector ordering test drives the same production helper with an instrumented
iterator; it is not a stress/timing guarantee for every OS schedule.
No compositor/PSD/IO/document/shared shell/Swift/generated edits. Existing
request_cancellation_tests, image_cache_tests cached-prefix parity and
document_filters integration remain A regression gates.

Installed rustfmt and git diff --check passed. No builds/tests/bindings/apps/
benchmarks or heartbeat restart on B. A compiles, reviews and merges. This closes
the proposed source-level type-erasure route; acceptance and end-to-end PSD
latency/memory behavior remain unverified. Editing-readiness smoke stays
independent on A.
