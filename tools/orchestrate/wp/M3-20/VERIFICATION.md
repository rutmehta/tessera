# M3-20 verification / handoff

## Latest allowlist retry

Re-executed the exact required test/clippy/fmt command: exit 0, keeping
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M3-20`.
The existing implementation needed no further code changes on this retry.
An additional real-model `--nocapture` run passed both tests without skipping:

- Warm complete 1024x1024 pipeline: **1.465047958 s**.
- Executed CoreML partitions: **112 / 583** optimized nodes; CPU fallback
  partitions remain, EP initialization fallback=None.
- Texture statistics L1: **0.013772 LaMa / 0.038086 PatchMatch**.
- Boundary MAE: **0.003342**.
- Model rehashed locally: **208044816 bytes**, matching the pinned SHA-256.
- `git diff --check` passed. Programmatic glob checking of all modified and
  untracked non-ignored files found **zero allowlist violations**. In particular,
  `tests/remove.rs`, `tests/remove_bounded.rs`, `tests/remove_models.rs` and
  `tests/distraction.rs` match the current explicitly allowed test globs.

Real-model output: `.cache/current-models.log`. No tests or thresholds were
changed. No weights were staged or committed, and protected filter files were
not touched. Earlier measurements below are retained as historical evidence,
not measurements from this retry.

Status: **PASS for the executed required gate and real-model regressions.**
The strict two-second performance assertion remains enabled when weights are
cached. No threshold was relaxed, test ignored, or model cache hidden.

## Implementation

- Pinned unmodified Apache-2.0 Carve big-LaMa ONNX export, immutable URL,
  SHA-256, byte size, named tensors and fixed-512 contract in the registry.
  Publisher provenance/license rechecked on this attempt. Locally rehashed
  bytes: 208044816 bytes, SHA-256
  `1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6`.
  No weights tracked. Details: `crates/filters/training/README.md`.
- Explicit download-on-demand plus verified cache-only loading in ml-runtime.
  `<dyn Remove>::auto` chooses cached ONNX or offline PatchMatch. Corrupt
  caches and explicit ONNX failures propagate rather than silently falling back.
- Bounded aspect-preserving working resolution, conservative thin-mask pooling,
  dilation, colour/contrast harmonization and gradient-domain boundary paste-back.
  Alpha and exterior remain exact. Generic caller-supplied ONNX sessions retain
  the stride-eight contract; the pinned graph uses fixed 512 RGB inputs.
- CoreML EP with CPU fallback, six sleeping CPU intra-op workers for LaMa,
  and executed-provider reporting. The all-CoreML guard remains strict.
- Suggestion-only CPU distraction hook for long thin contrast structures and
  dilated/clipped ml-faces boxes, with validated mask union consumed by Remove.
  This does not claim semantic wire detection or full-body segmentation.
- Retry optimization: reconstruct/decode model predictions only for nonzero
  mask coverage. Exterior predictions never participate in harmonization or
  boundary gradients and were previously discarded after needless conversion.
  Existing exact-exterior, alpha, soft-coverage and real quality tests pass.

## Executed gate

External target retained for every cargo command:
`/Volumes/betterSSD/tessera-cache/target/M3-20`.

```
cargo test -p filters -p ml-runtime --release && cargo clippy -p filters -p ml-runtime --all-targets -- -D warnings && cargo fmt --check
```

Result after the optimization: **exit 0**. Both real-model tests executed and
passed, along with the filters and ml-runtime suites. Clippy and workspace fmt
check passed. Existing LibRaw C++ warnings are not Rust clippy failures.
Log: `tools/orchestrate/wp/M3-20/.cache/optimized-gate.log`.
`git diff --check` also passed.

Separate real-model run with `--nocapture`: **exit 0**.
Log: `tools/orchestrate/wp/M3-20/.cache/optimized-models.log`.

- Complete warm 1024x1024 pipeline: **745.731125 ms**, below 2 seconds.
- Executed provider assignments: **112 CoreML partitions / 583 optimized nodes**.
  The remainder are CPU, not full ANE execution. EP initialization fallback=None.
- Quality CPU pipeline: 1.246862458 s.
- Texture RGB mean/std L1 distance: **LaMa 0.013772 vs PatchMatch 0.038086**.
- Boundary MAE: **0.003342**, below the unchanged 0.015 threshold.
- Exact exterior/alpha checks and actual cached Auto selection pass.

The synthetic quality regression does not imply universal photographic
superiority. Missing weights skip explicitly without network; corrupt cached
weights fail. Earlier implementation verification exercised explicit fetching
and missing-cache paths; unit/integration tests for registry/cache behavior
also pass in the current gate.

## Performance investigation and remaining caveat

An initial unchanged isolated run passed at 789.721083 ms. The first full gate
on this retry failed at **2.128827416 s**, reproducing the variability from
previous attempts (including the supplied 2.777898584 s failure).
Log: `.cache/retry-gate.log`.

Temporary stage timings (removed from source) placed most latency inside ONNX:
8.6 ms input preparation, 1.488 s cumulative after inference, and 1.662 s after
blending on one warm diagnostic run. A process snapshot showed two unrelated
rustc processes consuming 334% and 318% CPU. Those processes were not stopped,
reprioritized or otherwise modified. Contention is plausible, not an isolated
causal proof. Two-thread and CPUAndGPU raw-inference probes did not demonstrate
a reliable improvement, so production thread/provider choices were retained.

The masked-only reconstruction change reduces unnecessary CPU work, but does
not prove inference variability is fixed. The latest full gate and measured
model run pass; a hard sub-two-second guarantee under arbitrary competing host
load is not established. Do not present this as a universal latency guarantee.
No test assertions, warm-up policy or fixtures were weakened on this retry.

All changes remain within the M3-20 allowlist. No commits or pushes were made.
