# M2-58 remaining presentation acceptance

Source review on 2026-09-27 against published product 5ff2679 and evidence
196005e. This is a measurement plan, not a new performance result.

## Existing evidence and instrumentation

The earlier nonactivating run recorded 121 inputs and callbacks, but zero actual
presentation timestamps. `LoupeRenderer.swift` correctly records
`drawable_presented` only when Metal supplies a positive `presentedTime`; GPU
completion and drawable submission are separate events. `tools/bench/app_timing.py`
requires at least 100 causally joined presentations without dropped records.
Do not replace that oracle with callback latency or rerun an occluded window
indefinitely expecting it to establish display latency.

The Detail preview has a different presentation path: `DetailPreviewView.show`
copies an IOSurface into a CGImage, assigns CALayer contents and commits a Core
Animation transaction. `DevelopTools` validates session/settings revision before
publishing the detail callback. Neither callback nor transaction commit provides
the actual display timestamp required by the current P11 acceptance wording.

## Next executable sequence

1. Preserve the existing input/session/generation identity and actual-residency
   join. Use a source-provenance-verified release and the same copied RAW fixture,
   viewport, display configuration and scripted gesture for both comparisons.
2. Obtain an authorized, non-occluded presentation surface while preserving the
   current rule against activating, uncovering or rearranging the user's app
   windows. If no such surface is available, mark display acceptance unavailable;
   an offscreen render remains correctness evidence only.
3. Before collecting benchmarks, confirm positive actual presented timestamps
   and enough causal matches in a short capability run. Fail the capability check
   explicitly on zero timestamps, lost identity or trace drops.
4. For P11, record separate final-input, exact-detail-ready and detail-publication
   events with session/revision/crop identity. These stages locate latency but do
   not independently prove visible presentation. A display-level oracle for the
   CALayer detail region is still needed; do not label a CATransaction completion
   handler as scanout. Keep the <=200 ms acceptance pending until that distinction
   is resolved with measured display evidence.
5. Run paired detail-closed/detail-open gestures on the same source and host with
   no competing builds or GPU benchmarks. Retain all runs and compare actual
   input-to-present p95; require <=10% regression. Keep the final exact-detail
   settle measurement separate from the drag comparison.

The previously verified setter timing, two-case settled pixel parity, scheduler
regressions and callback correctness remain valid bounded results. None removes
these presentation dependencies. Current root work therefore advances export
integration and engine correctness while this acceptance path remains pending.
