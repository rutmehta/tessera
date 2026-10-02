# ENG-3b — cancellation-only luminance conditioning

Branch `wp/ENG-3-luminance-divisions`, based on `6f184e5e`.
This handoff supersedes ENG-3's original all-subfloor policy and its acceptance
of the neutral near-black discontinuity. Machine A's ENG-3b rulings in
A-ROUND2.md and LR-RULINGS.md were read before implementation.

## Item-by-item response to A

| Finding | Status / implementation | Test evidence |
| --- | --- | --- |
| Floor only cancellation, not ordinary deep shadows | **Done.** CPU `tone_extra.rs` and WGSL `operators.wgsl` use `abs(Y)<1e-3 && abs(Y)<0.25*max(abs(RGB))`. Otherwise the original nonzero `f(Y)/Y` ratio remains. The relative constant is documented as `k=0.25`; exact-zero and legacy nonpositive bypasses remain. | `eng3b_lifted_black_gradient_monotone`, `eng3_curve_signed_floor_boundaries`, `eng3b_curve_relative_cancellation_boundary` |
| Same policy for Photo Filter | **Done.** CPU `adjust.rs` and WGSL `adjust.wgsl` use the same predicate on **filtered** RGB/Y. Outside it, preserve original `channel*source_luma/Y` arithmetic; exact zero retains source RGB. | `eng3b_photo_positive_shadows_preserve_luminance`, `eng3_photo_full_gpu_parity_and_alpha` |
| Lifted-black photographic RAW and synthetic-gradient evidence | **Done.** Numeric before/after evidence below. Tests run the production curve on CPU and Metal and compare with an independent f64 log-domain curve oracle. No photographic pixel data is committed. | `eng3b_lifted_black_raw_monotone`, `eng3b_lifted_black_gradient_monotone` |
| Cancellation remains conditioned | **Done.** Original one-ulp sensitivity and controlled zero-delta-vs-one-ulp bounds remain unchanged. Threshold tests check relative ratios 0.249 and 0.251 with both luminance signs. | `eng3_curve_one_ulp_sensitivity`, `eng3_photo_one_ulp_sensitivity`, `eng3_curve_zero_delta_vs_one_ulp`, `eng3_photo_zero_delta_vs_one_ulp`, `eng3b_curve_relative_cancellation_boundary` |
| Fix sign jump at coloured L=0, **or document with coloured-pixel test** | **Done via A's documentation alternative.** Retained signed floor has a gain sign discontinuity for cancelling coloured pixels with nonzero target. Both operators have real CPU/Metal tests on either side of zero; measurements below. This is bounded conditioning, not continuity. | `eng3b_coloured_curve_zero_crossing_documented`, `eng3b_coloured_photo_zero_crossing_documented` |
| State negative-L difference from ENG-1 | **Done.** This lane uses `1+(f-L)/copysign(epsilon,L)` within the cancellation predicate. ENG-1 uses a positive denominator; the negative-L forms differ. README, TONE_M2 and this handoff say so explicitly. | Signed neutral oracle, relative-threshold and coloured-crossing tests above |
| Assert each production-shader replacement matches | **Done.** Explicit `assert!(src.contains(...))` immediately precedes the curve replacement and each of the two Photo Filter replacements. A shader text drift fails instead of silently disabling injection. | `eng3_curve_zero_delta_vs_one_ulp`, `eng3_photo_zero_delta_vs_one_ulp` |

## Test-first commits

- `9e1455c4` — `test(ENG-3b): expose lifted-black dips and pin signed cancellation`.
  RED against unmodified `6f184e5e`: **8 passed / 5 failed**, no ignored tests.
  Failures: gradient monotonicity, RAW monotonicity, signed neutral greys,
  relative cancellation threshold, and Photo Filter shadow preservation.
  An initial tile-size harness error was corrected and the numerical failures
  rerun before this commit. The existing cancellation tests stayed green.
- `2f89448d` — `fix(ENG-3b): restrict luminance floors to channel cancellation`.
  Targeted GREEN: **13 passed / 0 failed / 0 ignored**, real Metal enabled.
  Includes an additional direct GPU monotonicity assertion.
- Final `docs(ENG-3b):` commit records the final clean gates and audit.

Every commit ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

## Lifted-black evidence

Curve `(0,0.1) -> (1,1)` in the existing log curve domain. Exact black maps
to `0.037236832` in all runs. The synthetic test uses 256 neutral samples
from 0 through 0.002. The photographic test decodes `fixtures/raw/sample.dng`
through the production CPU RAW pipeline to scene-linear Rec.2020 at scale 8,
selects the darkest 255 finite all-positive samples, and applies one uniform
exposure multiplier **0.07937281** so their maximum luminance is 0.0002.
It preserves their photographic chromaticities, sorts them by luminance,
and prepends exact black. This is an exposure stress test of photographic
samples, not a claim that the unmodified photo already has this shadow range.

| Measurement | Before (RED) | After (CPU) | After (Metal) |
| --- | ---: | ---: | ---: |
| Synthetic first nonzero output Y | 0.000299902 | 0.037245348 | within 1.8626451e-8 RGB of CPU |
| Synthetic minimum adjacent Y step | -0.03693693 | +8.5011125e-6 | +8.489937e-6 |
| Synthetic final output Y | 0.039408 | 0.039408 | within bound above |
| Photographic first nonzero output Y | 0.004713115 | 0.037370674 | within 5.9604645e-8 RGB of CPU |
| Photographic minimum adjacent Y step | -0.032523718 | -7.450581e-9 | -7.450581e-9 |
| Photographic final output Y | 0.00765081 | 0.037454057 | within bound above |

No near-black dip remains. The photo's nearly tied samples are monotone to
f32 rounding: the smallest backward step is 7.45e-9, below the explicit 1e-7
monotonicity tolerance. Both CPU and Metal are checked independently.
The independent f64 oracle tolerance is 2e-7 and CPU/Metal RGB bound 2e-6.
Photo Filter's 257-sample positive-shadow gradient independently preserves
source luminance to 1e-9, with CPU/Metal RGB agreement to 1e-9 and exact alpha.

Cancellation measurements remain:

| Test | Measured result | Unchanged bound |
| --- | ---: | ---: |
| Curve one green-channel ulp | 9.536743e-7 | <=1e-5 |
| Photo Filter one green-channel ulp | 7.6293945e-6 | <=1e-5 |
| Curve full CPU/Metal cancellation gap | 3.8146973e-6 | <=1e-4 |
| Curve zero delta vs injected mapped-luminance ulp | 0 | <=1e-8 |
| Photo Filter zero delta vs injected mapped-luminance ulp | 0 | <=1e-8 |
| Photo Filter full CPU/Metal gap, negative / positive | 0 / 0 | <=1e-4 |

Retained coloured-zero-crossing behavior at Y approximately +/-1e-6:
curve red/gain changes from **-36.237827 to +38.235832**; Photo Filter red
changes from **-261.69998 to +263.69998**. Each is pinned on CPU and Metal
with a 1e-4 parity bound. Exact coloured zero retains the previous bypass
(curve) or source-RGB fallback (Photo Filter). The signed policy is deliberately
**documented, not fixed**, as A explicitly allowed; continuity would require
a separate rendering-policy decision.

## Exhaustive golden / fingerprint attribution

**Changed goldens: none. Changed reference fingerprints: none. No re-pins.**
Before/after captures were rerun on this machine for `6f184e5e` production
and `2f89448d` production. The existing strict comparison script reports:

- Nine synthetic float RGBA captures: sRGB, Display P3, Adobe RGB, each at
  amount 1 / 0.35 / 0. All **0 / 4403 changed pixels**, max delta 0,
  finite channels and bit-identical alpha.
- Five photographic RGB8 captures: Canon CR3 **0 / 250000**, Fuji RAF
  **0 / 249696**, Nikon NEF **0 / 568568**, DNG **0 / 282968**, Sony ARW
  **0 / 252150** changed pixels. Max delta 0 for every capture; each run
  also matches the immutable PNG reference exactly.

[eng3b-golden-report.json](eng3b-golden-report.json) lists all 14 before/after
SHA-256 pairs. Every pair is identical. There is no changed output requiring
pixel attribution. Only the intentional direct regression output changes:
nonzero `abs(Y)<epsilon` samples **outside** the relative cancellation
predicate now use the original ratio. No other operator was modified.

Capture reproduction uses `ENG1_CAPTURE` pointing to separate before/after
local directories, then:

```sh
cargo test --release -p filters --test camera_raw full_develop_matches_rgb_decode_golden -- --nocapture --test-threads=1
cargo test --release -p pipeline-cpu --test golden -- --nocapture
python3 tools/orchestrate/wp/ENG-3/compare_captures.py BEFORE AFTER --output REPORT.json
```

## Final gates

All runs used `PATH="$HOME/.cargo/bin:$PATH"`, `CARGO_BUILD_JOBS=4`,
`RAYON_NUM_THREADS=4`, and `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-3`.
Capture and fixture-override environment variables were unset for final gates.

```sh
cargo clean -p filters -p pipeline-cpu -p pipeline-gpu
cargo clean --release -p filters -p pipeline-cpu -p pipeline-gpu
cargo test --release -p filters -p pipeline-cpu -p pipeline-gpu -p image-core -p engine-api -p previews -p export -p tessera-ffi --no-fail-fast
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Both clean commands: **exit 0**; release clean removed 73 files / 83.5 MiB.
- Eight-crate release: **exit 0; 225 suites; 1,443 passed, 0 failed,
  66 ignored, 0 measured, 0 filtered**. No retries or skip filters.
  All 13 ENG-3 regressions passed again after the clean.

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| filters | 155 | 0 | 8 |
| pipeline-cpu | 179 | 0 | 3 |
| pipeline-gpu | 160 | 0 | 13 |
| image-core | 120 | 0 | 2 |
| engine-api | 120 | 0 | 0 |
| previews | 28 | 0 | 3 |
| export | 100 | 0 | 7 |
| tessera-ffi | 581 | 0 | 30 |

Workspace Clippy (`--all-targets -- -D warnings`): **exit 0**.
Workspace fmt check and `git diff --check`: **exit 0**. Existing LibRaw
native-compiler deprecation messages did not cause a Rust Clippy failure.

[eng3b-validation.json](eng3b-validation.json) records commands, exact counts,
durations and log SHA-256 values. Final gates ran against source `2f89448d`;
the final documentation commit changes no production or test code. Logs and
before/after captures are archived outside the repository at
`$HOME/tessera-evidence/ENG-3b/2f89448d`.

## Scope and remaining work

No Cargo.lock or board.json changes, dependency updates, private photographic
pixels, user metadata, app-data access, GUI launches, focus changes, or system
settings changes. Only the explicitly authorized lane branch is pushed.
Machine A remains sole merger. A's combined ENG-3/ENG-4-tree rerun before the
second merge remains integration-owner work: ENG-4 changes are not on this
lane, and no restack or merge was requested here.
