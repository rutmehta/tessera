# B5-32 — Camera Raw bright-value GPU/CPU parity

Branch: `wp/B5-32`, based on `68264c74`. Local commits only; no push, GUI launch, or screen capture. No `Cargo.lock` or `board.json` changes. B5-28 profile-curve semantics are unchanged.

## Result and limitation

Fixed a real near-zero-luminance tone precision defect in CPU and GPU evaluation. The independent f64-reference test improves from **1.8405e-5 CPU / 0.00511713 GPU** to **1.0117e-8 CPU / 2.2657e-8 GPU**. The rich chain without texture/clarity improves from **0.00438502** to **0.0000424013** maximum absolute encoded error.

**A blanket absolute 0.01 HDR bound is still not met.** The original 259×263 rich-chain maximum improves from **0.020483017 to 0.015808105**, so its absolute ceiling is tightened from 0.025 to **0.02**, retaining the existing 0.002 scaled guard. Other isolation cases use **1e-4 or 5e-6 absolute**, not relative tolerances.

The 24 MP benchmark now reports an all-pixel maximum in addition to testing its original every-997th-pixel sample. That sample meets **absolute 0.01** (measured **0.00609684**). **The full scan does not:** its largest error changes from **6.1408234 to 16.488846**, at pixel `(798, 852)`, blue, GPU **229.05931** vs CPU **212.57047** after the fix. This is a regression in the worst full-frame parity metric, not a claim of an overall HDR parity fix. All-pixel finite RGB and bit-exact alpha are checked. The full-scan RGB error is reported, not hidden behind the sampled assertion.

## Root cause

There are two distinct effects:

1. **Repairable tone evaluation error.** Tone uses `log1p`/`expm1` and integrated softplus controls. WGSL's existing `log_one_plus` evaluated `log(1+x)` with a rounding correction; it still had poor near-zero accuracy on this Metal path. Also, `softplus(z-center) - softplus(-center)` subtracts nearly equal values. The resulting tone error is amplified by the final division by luminance. A test with identical signed RGB input to each backend and an independent f64 reference isolates this from profile curves, matrices, sharpening, dehaze, and texture/clarity.
2. **Ill-conditioned signed-luminance gain, still present.** Sharpening decoded noise can produce negative R/G and positive B whose weighted sum is almost zero. Texture/clarity multiplies RGB by `decode(adjusted)/Y`. Small normal f32 differences in the earlier matrices/detail/tone become large RGB differences through this division. Dehaze is not the trigger: disabling it leaves the failing maximum unchanged.

At the original failing pixel `(84, 41)`, a diagnostic detail-only round trip gives linear Rec.2020 approximately `[-0.006959494, -0.005868249, 0.097931265]`, with `Y ≈ 3.924e-7`. The sum's condition number `sum(abs(weight*channel))/abs(Y)` is approximately **30,000**. These diagnostic values include output encode/decode and matrix round-trip rounding; they are not a capture of the resident scratch buffer.

The committed isolation test reproduces the sensitivity independently of Metal: change only the source blue at `(84,41)` to `next_up()`. After the precision fix, the scalar CPU blue changes **17.687162 → 17.603212**, a gap of **0.08395004** for one input f32 ulp. Before the fix the same change produced **17.700655 → 17.600351**, gap **0.10030365**. Thus ordinary f32 input/intermediate rounding alone can exceed 0.01 under the existing mapping. Improving pointwise tone accuracy cannot make this singular gain well-conditioned; it can redistribute its extreme outliers, as the 24 MP maximum demonstrates.

### Alternatives ruled out

- **f16 checkpoints:** the Camera Raw resident chain uses f32 storage and rejects a packed export. No f16 conversion was added or removed.
- **Texture sampler/filtering precision:** this path uses storage buffers and explicit arithmetic; the bridge LUT is not a hardware-filtered texture.
- **B5-28 LUT endpoint slope:** CPU and GPU bind/use the same sampled curve and last-segment extension/inverse. No curve table, slope, sign extension, primaries, matrix, or encoded-space amount blend changed.
- **Different operator order:** both paths apply detail, tone, texture/clarity, dehaze, curves/color/effects in the same order. The standalone sharpen/dehaze tests agree to a few e-6 with the profile curve enabled.

A uniform 0.01 bound would require a separate precision strategy that also addresses upstream rounding, or a defined policy for the near-zero signed-luminance gain (for example a floor or chroma/gain limitation). Clamping signed RGB or flooring Y would change rendering semantics and is deliberately not done here. This handoff documents the requested tolerance exception rather than declaring arbitrary HDR parity solved.

## Fix

- `pipeline-cpu/src/lib.rs`: for `abs(z) < 0.5`, evaluate the identical softplus integral as `log1p(expm1(z)/(1+exp(center)))`. Keep the original formula away from zero.
- `pipeline-gpu/src/operators.wgsl`: same integral, plus a degree-nine Horner evaluation of `log1p(x)` for `abs(x) < 0.125`. Its truncation remainder is below 1.1e-10 on that interval. The existing implementation remains outside it. Shared shader code covers isolated and fused tone paths.
- Algebraic identity: `softplus(z-c) - softplus(-c) = log1p(expm1(z)/(1+exp(c)))`; the change preserves the mathematical transfer function.
- No parameter/schema/API/FFI/colour-policy changes. Corrected floating-point evaluation can change numerical output, especially at the ill-conditioned outliers; it does not introduce a new tone curve.

A separate cancellation-safe log/exp experiment in `presence.wgsl` only reduced the original gap from 0.020483 to 0.018465. It was reverted; no presence-shader changes are shipped.

## Tests and measured absolute error

`bright_value_stage_isolation_with_and_without_profile_curve` uses the same noise samples, sRGB primaries and either sRGB TRC or its linear twin. It collects all cases before failing, checks every RGB sample and exact alpha, and reports the one-ulp CPU sensitivity.

| Case | Before | After | Final bound |
| --- | ---: | ---: | ---: |
| Near-zero signed tone vs f64, CPU | 0.0000184051 | 0.0000000101166 | 0.00001 |
| Near-zero signed tone vs f64, GPU | 0.00511713 | 0.0000000226568 | 0.00001 |
| Curve on, sharpen only | 0.00000189245 | 0.00000189245 | 0.000005 |
| Curve on, dehaze only | 0.00000240468 | 0.00000240468 | 0.000005 |
| Curve on, sharpen + dehaze | 0.00000201166 | 0.00000201166 | 0.000005 |
| Curve on, rich without sharpen | 0.0000629500 | 0.0000629500 | 0.0001 |
| Curve on, rich without texture/clarity | 0.00438502 | 0.0000424013 | 0.0001 |
| Curve on, rich without dehaze | 0.020483017 | 0.015808105 | 0.02 |
| Curve on, rich | 0.020483017 | 0.015808105 | 0.02 plus existing scaled guard in the chain test |
| Linear twin, rich | 0.0000246167 | 0.0000199601 | 0.0001 |
| 24 MP, original sampled coverage | Not separately measured | 0.00609684 | 0.01 absolute |
| 24 MP, all RGB samples | 6.1408234 | 16.488846 | Reported limitation; not an absolute-parity pass |

The interrupted run recorded 24 MP timings of CPU 254.16 s / GPU 3.386 s before and CPU 35.93 s / GPU 1.617 s after. The continuation rerun reproduced the after error metrics exactly, with CPU **211.30 s** / GPU **5.218 s**, excluding upload/readback and including cold pipelines. Shared-machine load varied substantially, so **these are not a performance comparison**.

## Test-first history

- `c55a5e2c` — `test(B5-32): isolate Camera Raw bright-value GPU parity regression`. Observed failures: original rich-chain absolute 0.01; the independent CPU/GPU tone f64-reference test; 24 MP all-pixel absolute 0.01.
- `635f69d8` — `fix(B5-32): stabilize near-zero tone evaluation and bound parity diagnostics`.
- Documentation commit follows the fix. Every commit carries the requested co-author trailer.

## Gates

Release tests completed on 2026-10-01: `cargo test --release -p filters -p pipeline-cpu -p pipeline-gpu` exited 0: **454 passed, 0 failed, 23 ignored**, across 93 test/doc-test suites. Metal stage isolation and the signed-tone f64-reference test were also rerun with `--nocapture`; the after values above reproduced exactly. The ignored 24 MP benchmark was run explicitly with `cargo test --release -p filters --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --nocapture`: **1 passed**, reproducing sampled and full-scan after metrics exactly. Its passing assertion covers the original sampled RGB parity plus all-pixel finite RGB and exact alpha; it does not certify full-frame RGB parity.

`cargo clippy --all-targets -p filters -p pipeline-cpu -p pipeline-gpu -- -D warnings`: exit 0, `Finished dev profile` (1m 58s).

`cargo fmt --all -- --check`: exit 0, no output.

`cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh`: FFI build/binding generation succeeded (arm64 archive), Swift development build succeeded. First full gate reported:

```text
Executed 861 tests, with 3 tests skipped and 2 failures (0 unexpected)
Test run with 5 tests in 2 suites passed
SWIFT GATE FAILED (exit 1):
DocumentAdaptiveWideAngleTests.testOKSurfacesAFailedFinalTrace
DocumentHistoryKeyboardTraversalTests.testDocumentSwitchKeepsGlobalHistoryPreferencesAndShowsSelectedDocument
```

The first failure was a missing final-trace error message (`nil`); the second was a SwiftUI history-control object-identity assertion. Both tests passed unchanged on an immediate targeted `swift test -c release --skip-build --filter ...` rerun: **2 passed, 0 failed**. They are outside the modified Camera Raw/tone code. Both also passed within the subsequent full gate retry. No test was disabled or modified to bypass these intermittent failures. The unchanged retry of `tools/orchestrate/swift-gate.sh` exited **0**, with the required result:

```text
Build complete! (39.35s)
Executed 861 tests, with 3 tests skipped and 0 failures (0 unexpected) in 509.070 (509.378) seconds
Test run with 5 tests in 2 suites passed after 0.028 seconds.
SWIFT GATE OK
```

The retry's release build completed in 155.10 s. See `VALIDATION.txt` for compact captured diagnostics and gate output from this continuation. Baseline before values were recorded by the interrupted run; the continuation reproduced the after values on Metal.

Commands use `PATH="$HOME/.cargo/bin:$PATH"` and `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-32`. Builds run serially with no added `-j`.
