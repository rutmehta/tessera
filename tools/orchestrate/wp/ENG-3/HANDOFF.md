# ENG-3f — sweeps across the actual switch, restored grey boundaries

Tests and docs only on top of `01138f75`; **no production code, golden,
fingerprint, Cargo.lock or board.json change**. Test commit: `2d00acc10410cfa947204e35f1a4265bf6acd608`.
Started by a Codex worker (items 2 and 3 and the test skeletons), completed
and verified by Claude Opus 5.5 (shared sweep support, bounds, docs, gates).

Machine A's four items:

1. **Sweeps that cross the switch — done.** The switch is `D == |Y|`, i.e.
   `rho* = k(1 - |Y|/epsilon)`. Shared support
   `crates/pipeline-gpu/tests/support/eng3_switch.rs`; tests
   `eng3f_curve_actual_switch_sweeps` (pipeline-gpu) and
   `eng3f_photo_actual_switch_sweeps` (filters), each on CPU and Metal,
   2001 samples per sweep, pixels `(r, g, 0)` with opposing red/green:
   - rho sweep 0.10 → 0.15 at Y = 5e-4 (switch at rho* = 0.125, sample 1001);
   - |Y| sweep 7.5e-4 → 8.5e-4 at rho = 0.05 (floor 8e-4, sample 1000).

   Each sweep asserts, from an independent f64 evaluation of the actual f32
   pixels, that it starts in the floor branch, ends in the ratio branch and
   crosses exactly once inside the middle half. The existing ENG-3c sweeps
   are kept and now commented as ratio-branch-only.
2. **Neutral-grey boundaries restored — done.**
   `eng3_curve_signed_floor_boundaries` is byte-identical to its `d824c09e`
   version: extended curve, greys ±0.002, ±0.001001, ±0.000999, ±1e-6, CPU
   versus the f64 mapped value and CPU versus Metal both at 1e-7. It sits
   next to `eng3c_curve_cancelling_pixel_signed_floor_boundaries` and passes.
3. **Floored count — done.** `eng3b_lifted_black_raw_monotone` now asserts
   `assert_eq!(floored, 32)` (measured 32; max RGB oracle error 2.1297947e-7).
4. **Docs — done.** `crates/pipeline-cpu/TONE_M2.md` and
   `crates/filters/README.md` state the switch at `rho* = k(1-|Y|/epsilon)`.

## Step bound and its derivation

With target luminance T the output luminance is `T` in the ratio branch and
`Y + (T - Y)|Y|/D` in the floor branch: continuous at `D = |Y|` and
piecewise C1. By the mean value theorem each adjacent step must satisfy

    |dYout| <= S_rho * |d rho| + S_y * |d Y| + R_i + R_(i-1)

where `d rho`, `d Y` are the actual f32-rounded sample differences and
`S_rho`, `S_y` are suprema of the partial derivatives over the sweep:

| Operator | S_rho | S_y |
| --- | --- | --- |
| Curve, T = f(Y) | `(f - Y) eps / (k Y)` (floor: `(f-Y) Y eps/(k D^2)`, D >= Y; ratio: 0) | `f' + (f - Y)/Y` (floor: `1 + (f'-1)Y/D + (f-Y)/D`; ratio: f') |
| Photo Filter, T = L(1.625 + 0.375/rho), q = 0.625 + 0.375/rho | `max(0.375 L/rho^2, q eps/k)` (opposite-signed floor terms; ratio: first term) | `1 + 2q` (floor: `1 + 2 q L/D`; ratio: `1 + q`) |

evaluated at the least favourable corner (f at Y_max, Y_min and rho_min in
denominators). The Photo Filter test uses colour (0.5, 0.8, 0.5), density 1,
so the source is `(r/0.5, g/0.8, 0)` and T is its luminance.
`R_i = 8 * 2^-24 * A_out,i` is the f32 rounding allowance with
`A_out = sum w_c |out_c|`: 2 units for the cancelling input luminance
(relative error 2u/rho amplified by the gain), 2 for rho → D, 3 for target
and gain arithmetic, 1 for the final product. The bound was fixed before
the first run and was not adjusted afterwards; every sweep passed first time.

## Measured maximum adjacent output-luminance steps

| Operator | Sweep | CPU max step | Metal max step | Asserted bound at that step | Step across the switch (CPU / Metal) |
| --- | --- | ---: | ---: | ---: | ---: |
| Curve | rho | 7.4683130e-6 | 7.4673057e-6 | 7.7527318e-6 | 3.3795833e-9 / 2.1755695e-9 |
| Curve | \|Y\| | 2.4798989e-6 | 2.4798989e-6 | 3.3054691e-6 (Metal 3.3053339e-6) | 2.4041414e-6 / 2.4041414e-6 |
| Photo Filter | rho | 3.0082874e-7 | 3.0076578e-7 | 4.8701565e-7 (Metal 4.8691402e-7) | 2.9896498e-7 / 2.9994361e-7 |
| Photo Filter | \|Y\| | 8.7632537e-7 | 8.7582171e-7 | 1.0185267e-6 | 8.7026656e-7 / 8.6799264e-7 |

Suprema used: curve rho sweep S_rho = 0.29823889, S_y = 75.648622; curve |Y|
sweep S_y = 50.970623; Photo Filter rho sweep S_rho = 0.01875001; Photo
Filter |Y| sweep S_y = 17.250014. Sample spacing: 2.5e-5 in rho, 5e-8 in |Y|.
Curve output luminance runs 0.031566551 → 0.037779850 over the rho sweep
(constant f(Y) after the switch, hence the ~3e-9 step there) and
0.035719966 → 0.038159856 over the |Y| sweep. The largest steps are the
smooth floor-branch slope next to the switch, not a jump: no discontinuity
was found on either backend for either operator.

## Gates (final test source `2d00acc10410cfa947204e35f1a4265bf6acd608`)

Env: PATH with `$HOME/.cargo/bin`, target `$HOME/.cache/tessera-target/ENG-3`,
`CARGO_BUILD_JOBS=5`, `RAYON_NUM_THREADS=5` (CLAUDE-COMMON values).

```sh
cargo clean --release -p filters -p pipeline-cpu -p pipeline-gpu   # 741 files, 1.7 GiB
cargo clean -p filters -p pipeline-cpu -p pipeline-gpu             # 1462 files, 449.6 MiB
cargo test --release -p filters -p pipeline-cpu -p pipeline-gpu --no-fail-fast -- --test-threads=1 --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

All passed on the first attempt (one attempt each, serialized, while another
lane's build was running): **101 suites, 535 passed, 0 failed, 23 ignored**
(existing ignores) — filters 161 / 0 / 8, pipeline-cpu 203 / 0 / 3,
pipeline-gpu 171 / 0 / 12. Workspace Clippy, fmt and diff checks exit 0.
One earlier focused run failed before any measurement because the curve
sweep used a single 2001-wide tile (`tile interior exceeds 256`); the test
now feeds 256-sample tiles. No bound or assertion was changed for it.

---

# ENG-3e — reproducible Photo Filter one-ULP worst case

Follow-up on `e200b4bc`; Machine A accepted the existing `1e-4` bound.
Temporary instrumentation of `eng3_photo_one_ulp_sensitivity` in
`crates/filters/tests/eng3_luminance.rs` reproduces the reported maximum
absolute RGB response **6.1035156e-5** (exactly `0.00006103515625`).
This is the maximum over channels and nine identical pixels for the test's
fixed input pair, not a global maximum over arbitrary RGB. The separate
chromaticity sweeps in this file measure adjacent luminance continuity.

The source RGBA is `[2., -0.2627_f32 / 0.678, 1e-6, 0.7]`, printed as
`[2.0, -0.38746312, 1e-6, 0.7]`; its exact f32 bit patterns in decimal are
`[1073741824, 3200672145, 897988541, 1060320051]`. The second input changes
only green with `next_up()`, to `-0.3874631` (bits `3200672144`), a positive
step of `2.98023223876953125e-8`.

Filter settings: `colour=[0.5, 1.0, 1.0]`, `density=1.0`,
`preserve_luminosity=true`, applied directly with `Adjustment::apply`.
Photo Filter evaluates Y and A on **filtered RGB**, here
`[1.0, -0.38746312, 1e-6]` before the green perturbation, rather than on
source RGB. The following values reproduce production f32 arithmetic:

| Quantity | Original input | Green `next_up()` input |
| --- | ---: | ---: |
| Y = 0.2627r + 0.678g + 0.0593b | 5.9300003130147161e-8 | 8.9102329070556152e-8 |
| A = 0.2627\|r\| + 0.678\|g\| + 0.0593\|b\| | 5.2540004253387451e-1 | 5.2539998292922974e-1 |
| rho = \|Y\|/A | 1.1286638113006120e-7 | 1.6958951221113239e-7 |
| D = epsilon * clamp(1 - rho/k, 0, 1) | 9.9999958183616400e-4 | 9.9999934900552034e-4 |
| Output RGBA (shortest f32 decimal) | `[263.7001, -102.174065, 0.00026370012, 0.7]` | `[263.70016, -102.17408, 0.00026370017, 0.7]` |

With `epsilon=1e-3` and `k=0.25`, the **tapered floor term** is active in
`D=max(|Y|, epsilon*clamp(1-rho/k,0,1))` for both inputs: D is approximately
`1e-3`, much larger than |Y|. Both Y values are positive. The maximum
response is in **red**; alpha remains unchanged. The existing independent
f64 recombination oracle and its `<3e-5` per-output error bound still pass.
The test now names this input `WORST_CASE_RGBA`; temporary prints were removed.

Validation (PATH prepended with `$HOME/.cargo/bin`, target directory
`$HOME/.cache/tessera-target/ENG-3`, build jobs and Rayon threads both 4):

```sh
cargo test --release -p filters --test eng3_luminance -- --test-threads=1 --nocapture
cargo fmt --all -- --check
cargo clippy -p filters --all-targets -- -D warnings
git diff --check
```

The instrumented release run and the final run without temporary prints each
pass all **6 tests**, including CPU/Metal checks. Formatting, filters all-target
Clippy, and diff checks pass. Existing native LibRaw build warnings remain.
Only this handoff and the named test input/comment change; production code,
assertions, and numerical bounds are unchanged.

---

# ENG-3d — rebase onto fbb36594 and combined-tree gates

Rebased `wp/ENG-3-luminance-divisions` from `cc626467` (nine commits on
`f6b572ba`) onto `fbb36594b8ae97fe9ee1d5e4c36269bbe526a649` using
`git fetch origin && git rebase origin/main`. Combined source commit:
`bdceb4730f896bb9dedd803b1e17e3e1cdb08bca`.

## Rebase and policy audit

**No conflicts occurred.** All nine commits replayed without edits, squashing,
or reordering; `git range-diff` reports `=` for every commit and a bytewise
comparison of all nine complete commit messages passes. The final documentation
commit is the only additional commit.

The combined tree retains ENG-4/4b's stable shared `log_one_plus` and
`exp_minus_one` formulation, CPU curve/presence/dehaze axes, corresponding
WGSL helpers, and GPU curve-axis normalization. It also retains all four
ENG-3c cancellation sites with A=0.2627|r|+0.678|g|+0.0593|b|,
rho=|Y|/A, D=max(|Y|,epsilon*clamp(1-rho/k,0,1)), epsilon=1e-3, k=0.25.
D==|Y| keeps literal curve f/Y and Photo Filter channel*f/Y operation order.
Exact-zero handling and the documented signed-channel zero-crossing policy
are unchanged. No source, tests, bounds, or reference expectations were edited
while performing ENG-3d.

## Numerical before → after audit

Before values are the ENG-3c committed handoff and archived `green.log`;
after values are a fresh serialized release run of both `eng3_luminance`
integration binaries on the combined tree: **15 passed, 0 failed, 0 ignored**.
[eng3d-measurements.json](eng3d-measurements.json) retains every printed
measurement before/after, including unchanged entries. Ten of 23 printed
measurement lines differ. All assertions and bounds remain unchanged.

The curve differences follow from upstream ENG-4/4b: CPU curve encode/decode
now evaluates the shared stable series/ordinary-log branches instead of
`ln_1p`/`exp_m1`; GPU uses the stable helpers and matching host normalization.
This changes f32 rounding in curve targets and downstream recombination.
The cancellation policy and the regression tests themselves are byte-identical
to pre-rebase. Photo Filter production files are also byte-identical and all
its measurements are unchanged.

| Measurement | Before | After |
| --- | ---: | ---: |
| Curve one-green-ULP RGB response (bound 1e-5) | 7.6293945e-6 | 3.8146973e-6 |
| Curve near-zero CPU/Metal RGB gap | 0 | 3.8146973e-6 |
| Photo Filter one-green-ULP RGB response (bound 1e-4) | 6.1035156e-5 | 6.1035156e-5 |
| Photo Filter independent f64 per-output error bound | <3e-5 | <3e-5, passes |
| Curve / Photo Filter injected target-ULP zero-delta response (bound 1e-8) | 0 / 0 | 0 / 0 |
| Exact black output Y | 0.037236832 | 0.037236832 |
| Curve signed crossing red/gain | -36.238113 → +38.23612 | unchanged |
| Photo Filter signed crossing red | -261.702 → +263.702 | unchanged |

Fixed Y=0.0005 and rho step 0.00002; each continuity bound remains <1e-6:

| Curve boundary | CPU max adjacent Y step, before → after | Metal max step, before → after |
| --- | --- | --- |
| rho=0.25 | 1.381039618775226e-8 → same | 1.9764900202612345e-8 → 1.8358230588488844e-8 |
| Former hard boundary, rho≈0.9077706 | 7.336307318583923e-9 → 6.830785423406205e-9 | 7.020588957407092e-9 → 6.9886446005251734e-9 |

CPU endpoints at rho=0.25 remain [0.03777984806001186,
0.03777984440922737]. Former-boundary endpoints change from
[0.03777984725274146, 0.03777984584430232] to
[0.037779843496065585, 0.037779842087626456]. Photo Filter maximum
adjacent Y steps remain 6.193295e-8 at rho=0.25 and 5.005859e-9 at the
former boundary. Its primary-ramp max steps (red/green/blue) remain
2.0524487e-5 / 5.2970834e-5 / 4.6329806e-6. Primary luminance preservation
(<1e-8), monotonicity (>=-1e-9), and exact alpha all pass.

Curve primary ramps (256 samples each, 0..0.02):

| Primary | First Y, before → after | Last Y, before → after | CPU min step | Metal min step, before → after | Max CPU/Metal RGB gap, before → after |
| --- | --- | --- | --- | --- | --- |
| Blue | 0.03724188 → same | 0.038524624 → same | 5.0365925e-6 unchanged | 5.0254166e-6 → 5.040318e-6 | 2.9802322e-7 → 1.7881393e-7 |
| Red | 0.03725921 → 0.037259206 | 0.042935397 → 0.042935405 | 2.2303313e-5 unchanged | 2.2303313e-5 → 2.2307038e-5 | 5.9604645e-8 → 4.4703484e-8 |
| Green | 0.03729459 → 0.037294585 | 0.05191148 → 0.051911484 | 5.7335943e-5 unchanged | 5.733967e-5 → 5.7328492e-5 | 2.9802322e-8 → same |

The retained 0..0.002 neutral gradient has unchanged first Y=0.037245348;
last Y 0.039408 → 0.039407995; CPU minimum step 8.5011125e-6 →
8.497387e-6; Metal minimum step 8.489937e-6 → 8.5011125e-6;
maximum CPU/Metal RGB gap 1.8626451e-8 → 1.1175871e-8.

RAW uses the same fixture, scale 8, darkest 255 all-positive pixels plus
black, and exposure multiplier 0.07937281. First Y=0.037370674 remains
unchanged; last Y 0.037454057 → 0.03745405. CPU minimum adjacent Y step
remains -7.450581e-9; Metal minimum -7.450581e-9 → -3.7252903e-9;
maximum CPU/Metal RGB gap 5.9604645e-8 → 2.9802322e-8. Monotonicity
bound remains -1e-7. All **32 → 32** synthetic cancelling transforms enter
the floor. Maximum CPU/Metal RGB error against the independent f64 amended
oracle is **2.6897474114662145e-7 → 2.1297947050413768e-7**, below the
unchanged **2e-6** bound. These are signed stress transforms of photographic
pixels, not a claim about native cancelling colours in the RAW.

## Goldens, captures, and fingerprints

No stored golden file changed. No ENG-3d expectation was re-pinned. This
rebase does inherit ENG-4's already-reviewed CPU SDR case-2 fingerprint
change; it would be incorrect to claim every pre-rebase fingerprint is
unchanged.

All 14 archived pre-rebase captures were SHA-256 checked against the
committed ENG-3c report before comparison. Fresh combined-tree captures show:

| Float capture | Changed pixels / 4403 | Max encoded delta vs pre-rebase |
| --- | ---: | ---: |
| sRGB, amount 1 | 2243 | 1.9103288650512695e-5 |
| sRGB, amount .35 | 2232 | 6.690621376037598e-6 |
| Display P3, amount 1 | 2237 | 4.887580871582031e-6 |
| Display P3, amount .35 | 2204 | 1.7136335372924805e-6 |
| Adobe RGB, amount 1 | 2252 | 2.7239322662353516e-5 |
| Adobe RGB, amount .35 | 2228 | 9.5367431640625e-6 |
| All three profiles, amount 0 | 0 each | 0 |
| Canon CR3, Fuji RAF, Nikon NEF, DNG, Sony ARW RGB8 | 0 each | 0 |

Every synthetic combined-tree SHA-256 equals the corresponding upstream
ENG-4b capture SHA-256, including all six changed outputs. The changed
float values are inherited from ENG-4's stable basic-tone formulation and
ENG-4b's shared curve/presence/dehaze axes, propagated through profile
conversion and opacity blending. Amount zero bypasses those operations.
Alpha remains bit-exact. The synthetic stored-reference maximum deltas are
the same values shown above, all below the unchanged 1e-4 limit. All five
RAW outputs remain exactly equal to their immutable PNG references.
[eng3d-golden-report.json](eng3d-golden-report.json) gives every before/after
SHA-256, pixel count, maximum delta, and upstream-match result. No photographic
pixels are committed.

The fresh SDR probe yields the following exact fingerprints. CPU case 2 is
**0x39f4bd02fec83fd1 → 0xd1434c534f542e11**; all other entries are unchanged:

| Case | CPU after | GPU tiles | GPU surface |
| --- | --- | --- | --- |
| 0 | 0x44fae39b8cad1e | 0x50eac09770927b7a | 0xdbf920dcf5887ebe |
| 1 | 0xcc470d33eeb4d28a | 0x9888e139c4c750cd | 0xfc167ff4409e51d9 |
| 2 | 0xd1434c534f542e11 | 0x9f14a1d177fda67c | 0x6955ddd3131422e4 |

All nine printed values were independently compared to the stored constants,
including GPU values whose in-test assertion is scoped to Apple M4 rather
than this Apple M4 Max. A fresh run of ENG-4's `sdr_audit.py` on the combined
tree reproduces the complete committed ENG-4 attribution JSON exactly:
only red at (5,108) changes 102→103 and red at (160,65) changes 152→151
in CPU case 2. ENG-4's stable tone evaluation moves these two values across
ordered-dither rounding boundaries; all changed bytes satisfy its pointwise
tone/quantizer predicate. No additional fingerprint change arises from the
combined tree.

## Combined-tree gates and every attempt

Environment: PATH prepended with `$HOME/.cargo/bin`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-3`,
`CARGO_BUILD_JOBS=4`, `RAYON_NUM_THREADS=4`. All commands ran sequentially;
the release matrix and targeted integration runs use `--test-threads=1`.
Capture overrides are absent from the full release gate. Other builds were
active on the shared machine, but **every gate attempt passed**; no retries,
relaxed bounds, test exclusions, or production changes were needed.

Both package cleans covered all eight requested crates. The unqualified clean
removed 3,079 files / 2.0 GiB; release clean removed 1,543 files / 5.9 GiB.

```sh
cargo clean -p filters -p pipeline-cpu -p pipeline-gpu -p image-core -p engine-api -p previews -p export -p tessera-ffi
cargo clean --release -p filters -p pipeline-cpu -p pipeline-gpu -p image-core -p engine-api -p previews -p export -p tessera-ffi
cargo test --release -p filters -p pipeline-gpu --test eng3_luminance --no-fail-fast -- --test-threads=1 --nocapture
cargo test --release -p filters -p pipeline-cpu -p pipeline-gpu -p image-core -p engine-api -p previews -p export -p tessera-ffi --no-fail-fast -- --test-threads=1
# Separate opt-in captures, with ENG1_CAPTURE pointing at a pre-created directory:
cargo test --release -p filters --test camera_raw full_develop -- --test-threads=1 --nocapture
cargo test --release -p pipeline-cpu --test golden -- --test-threads=1 --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test --release -p pipeline-gpu --test hdr_surface sdr_output_matches_approved_fingerprints -- --exact --test-threads=1 --nocapture
cargo test --release -p filters --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --exact --test-threads=1 --nocapture
python3 tools/orchestrate/wp/ENG-4/sdr_audit.py --output "$HOME/tessera-evidence/ENG-3d/sdr-audit"
git diff --check
```

All exit codes are zero. The release gate completes **236 suites: 1,522 passed,
0 failed, 65 ignored, 0 filtered** (old base: 225 suites, 1,445 passed,
0 failed, 66 ignored). Counts differ because upstream main adds tests and
ENG-4 restores `tone_signed_rgb_near_zero_luminance_matches_f64_reference`
from ignored to active. Every ENG-3/3b/3c test executes on CPU/Metal.

| Crate | Suites | Passed before → after | Failed after | Ignored after |
| --- | ---: | ---: | ---: | ---: |
| filters | 29 | 156 → 160 | 0 | 8 |
| pipeline-cpu | 33 | 179 → 203 | 0 | 3 |
| pipeline-gpu | 39 | 161 → 169 | 0 | 12 |
| image-core | 24 | 120 → 122 | 0 | 2 |
| engine-api | 16 | 120 → 133 | 0 | 0 |
| previews | 4 | 28 → 28 | 0 | 3 |
| export | 33 | 100 → 103 | 0 | 7 |
| tessera-ffi | 58 | 581 → 604 | 0 | 30 |

The two targeted ENG-3 binaries pass 6+9 tests. Capture commands pass 3+1
tests; SDR fingerprint, explicit 24 MP, and SDR replay probes each pass one.
The 24 MP test scans all 72 million RGB samples per configuration: maximum
absolute errors 0.00021445751 (presence off), 0.00022757053 (on), exactly
matching ENG-4b. The on-minus-off increase 0.00001311302 passes the restored
1e-4 allowance; absolute/scaled, finite, alpha and release timing guards also
pass unchanged. Workspace all-target Clippy with `-D warnings` and fmt pass.
Existing native LibRaw deprecation diagnostics are not Rust Clippy warnings.

[eng3d-validation.json](eng3d-validation.json) records every command attempt,
exit code, UTC start/end time, crate counts, and full-log SHA-256. Logs,
captures, and reproduction scripts remain at `$HOME/tessera-evidence/ENG-3d`.
The historical ENG-3c record below remains intact; its old source hashes,
counts, and zero-difference audit are historical, superseded by this section
for the combined tree.

No Cargo.lock, board.json, source, test, golden, or fingerprint expectation
was edited in ENG-3d. This documentation-only follow-up preserves all nine
rebased commits and is pushed only to `origin wp/ENG-3-luminance-divisions`
with `--force-with-lease`. Machine A remains integration owner.

---

# ENG-3c — amended continuous cancellation policy

Branch `wp/ENG-3-luminance-divisions`, base `b3c8a531`.
This supersedes ENG-3b's hard `|Y| < 0.25 max|c|` predicate and the previous
handoff's claim that its boundary test proved continuity.

## Binding amended policy

All four production sites (`tone_extra.rs`, `operators.wgsl`, filters
`adjust.rs`, and `adjust.wgsl`) now use the existing exact luminance weights:
**0.2627, 0.678, 0.0593**, matching Machine A's numbers.

- `A = 0.2627|r| + 0.678|g| + 0.0593|b|`; `rho = |Y|/A`.
- `D = max(|Y|, epsilon * clamp(1-rho/k, 0, 1))`.
- `epsilon=1e-3`, documented `k=0.25`; gain `1+(f-Y)/copysign(D,Y)`.
- Where `D==|Y|`, keep literal curve `f/Y` and Photo Filter `channel*f/Y`
  arithmetic, preserving the original operation order.
- Photo Filter computes A and Y on filtered RGB. Same-sign colours have
  rho=1 and never receive a floor, including saturated-blue deep shadows.
- Exact black/A=0 retains the existing zero path. Nonzero Y implies A>0;
  both implementations avoid A division on the exact-zero path. Legacy
  nonpositive curve bypass and extended signed-domain activation are retained.
- The signed-channel Y=0 crossing remains discontinuous for a nonzero target.
  This is documented follow-up work, as authorized; negative-Y behavior still
  differs from ENG-1's positive-denominator policy.

## Per-item status and test evidence

| Amended finding | Status / evidence |
| --- | --- |
| Remove the hard cancellation boundary jump | **Done.** `eng3c_curve_chroma_boundary_continuity` sweeps 201 chromaticities at fixed Y=0.0005 across rho=0.25 and the former Y/max=0.25 boundary (rho≈0.9077706). CPU and Metal adjacent-output continuity assertions replace the old self-validating formulas. Photo Filter has analogous sweeps. |
| Saturated blue receives full near-black lift | **Done.** `eng3c_primary_shadow_ramps` checks blue/red/green, 256 samples each including black, channel range 0..0.02, independent f64 log-curve oracle and CPU/Metal monotonicity. |
| Same policy at all four sites | **Done.** `eng3c_photo_chroma_continuity_and_primary_ramps` exercises production CPU/Metal Photo Filter, saturated primary preservation, boundary continuity and exact alpha. |
| Lifted-black RAW oracle includes floored samples | **Done.** `eng3b_lifted_black_raw_monotone` retains its all-positive photographic test and adds 32 deliberately cancelling samples derived from `fixtures/raw/sample.dng`. Each is checked against the amended denominator and independent f64 curve oracle on CPU and Metal; an assertion requires floor coverage. |
| Rename signed floor boundary test and retain actual floor coverage | **Done.** `eng3c_curve_cancelling_pixel_signed_floor_boundaries` uses opposing red/green contributions, tests ±0.999epsilon / ±1.001epsilon (plus ±0.002 and ±1e-6), and asserts which samples have D>|Y|. |
| Document Y=0 sign crossing | **Done.** Existing coloured crossing tests remain on both backends; results below. |
| No golden/fingerprint changes | See final audit below; no references re-pinned. |

## Tests first

1. `98a51668` — `test(ENG-3c): expose chroma boundary jumps and primary shadow dips`.
   RED against unchanged `b3c8a531` production: **10 passed, 5 failed** across
   the two integration binaries. Failures: curve chroma boundary, primary
   ramps, RAW floor oracle, signed cancelling floor boundaries, Photo Filter
   continuity/primary ramps. Numerical failures were verified before commit.
2. `2e4a1339` — `fix(ENG-3c): taper luminance conditioning by weighted cancellation`.
   Targeted GREEN: **15 passed, 0 failed, 0 ignored**, real Metal.
3. Final `docs(ENG-3c):` commit records this handoff and final gates.

Every commit ends with the requested Claude Opus 5.5 co-author trailer.

The signed test uses large cancelling channels so ±0.999epsilon actually
enters the tapered floor (A≈8.4). Its CPU oracle uses production f32 Y/A
before f64 recombination; CPU absolute RGB tolerance is 0.002 and Metal
relative tolerance is 1e-4 because cancellation amplifies backend rounding.
The test failed RED by 0.291 RGB at -0.999epsilon, not by tolerance noise.

**Explicit sensitivity-test adjustment:** the amended denominator varies
with rho, so its derivative contributes to the one-green-ULP Photo Filter
response. Measured max RGB difference increases from 7.6293945e-6 to
6.1035156e-5. The old constant-floor 1e-5 bound is replaced by 1e-4 plus an
independent f64 formula oracle (per-output error <3e-5). No rendering formula
was altered to satisfy the old bound. Curve sensitivity is 7.6293945e-6,
still within its unchanged 1e-5 bound. Both injected mapped-luminance-ULP
zero-delta tests remain exactly zero, within their unchanged 1e-8 bound.

## Continuity and primary-ramp numbers

Fixed Y=0.0005, rho step 0.00002:

| Curve sweep | RED max adjacent Y step CPU / Metal | GREEN CPU / Metal |
| --- | ---: | ---: |
| Former hard boundary, rho≈0.9077706 | 0.01863992266 / 0.01863992641 | 7.33631e-9 / 7.02059e-9 |
| rho=k=0.25 | ~8.55e-9 / ~8.17e-9, but wrong output Y≈0.01914 | 1.38104e-8 / 1.97649e-8, output Y≈0.03777985 |

The old-boundary GREEN endpoints are 0.03777984725 and 0.03777984584.
Photo Filter maximum adjacent Y steps (combined CPU/Metal maxima) are
6.193295e-8 across rho=0.25 and 5.005859e-9 across the former boundary.
Continuity assertions require <1e-6 for both operators and both backends.

Exact black output Y is 0.037236832. Primary ramps:

| Primary | First nonzero output Y | Last Y | CPU min adjacent Y step | Metal min step | Max CPU/Metal RGB gap |
| --- | ---: | ---: | ---: | ---: | ---: |
| Blue | 0.03724188 | 0.038524624 | +5.0365925e-6 | +5.0254166e-6 | 2.9802322e-7 |
| Red | 0.03725921 | 0.042935397 | +2.2303313e-5 | +2.2303313e-5 | 5.9604645e-8 |
| Green | 0.03729459 | 0.05191148 | +5.7335943e-5 | +5.733967e-5 | 2.9802322e-8 |

Blue RED first nonzero Y was 0.00017784061 and minimum step -0.03705899.
Photo Filter primary ramps preserve source luminance within 1e-8, are
monotone within 1e-9 on each backend, and preserve alpha exactly.

RAW: decode `fixtures/raw/sample.dng` at scale 8, take darkest 255 finite
all-positive pixels, uniform exposure multiplier 0.07937281, prepend black.
Retained photographic minimum step is -7.450581e-9 on CPU and Metal (f32
rounding of nearly tied samples, tolerance 1e-7). For the **synthetic signed
stress transform of photographic pixels**, add 0.02 to red and solve green
for the original Y with blue zero; sample every eighth pixel. All **32**
enter the floor. Maximum CPU/Metal RGB error versus the f64 amended oracle
is **2.6897474e-7** (bound 2e-6). This does not claim the unmodified RAW
contains those cancelling colours.

Retained signed-channel zero crossing at Y≈±1e-6: curve red/gain
**-36.238113 → +38.23612**; Photo Filter red **-261.702 → +263.702**.
Exact coloured zero retains the existing curve bypass / Photo Filter source
fallback. This separate sign crossing is not claimed continuous.

## Final audit and gates

**Changed goldens: none. Changed reference fingerprints: none. No re-pins.**
[eng3c-golden-report.json](eng3c-golden-report.json) records all 14 matching
before/after SHA-256 pairs: nine synthetic RGBA captures (sRGB, Display P3,
Adobe RGB at amounts 1 / 0.35 / 0) and five fixture RAW RGB8 captures
(Canon CR3, Fuji RAF, Nikon NEF, DNG, Sony ARW). Total **0 / 1,643,009**
changed pixels; maximum delta zero. All five RAW outputs also match their
immutable PNG references with zero changed pixels. Exhaustive changed-output
attribution is empty. No Cargo.lock, board.json, golden or fingerprint file
was modified.

Capture commands use `ENG1_CAPTURE` with separate pre-created before/after
directories, the existing three `camera_raw` full-develop tests and the
`pipeline-cpu --test golden` suite, then `compare_captures.py BEFORE AFTER`.
Capture variables are absent from the full release gate.

All builds use PATH with `$HOME/.cargo/bin`, target
`$HOME/.cache/tessera-target/ENG-3`, `CARGO_BUILD_JOBS=4`, `RAYON_NUM_THREADS=4`.

```sh
cargo clean -p filters -p pipeline-cpu -p pipeline-gpu
cargo clean --release -p filters -p pipeline-cpu -p pipeline-gpu
cargo test --release -p filters -p pipeline-cpu -p pipeline-gpu -p image-core -p engine-api -p previews -p export -p tessera-ffi --no-fail-fast -- --test-threads=1
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
git diff --check
```

Both clean commands succeeded (1,097 files / 324.9 MiB and 701 files / 1.6 GiB).
Workspace Clippy and formatting pass. Initial Clippy identified two new test
range-loop style warnings; these were corrected to iterator enumeration in
the fix commit and the full Clippy gate rerun successfully.

An initial parallel full run passed **1,445 / 0 / 66** (passed / failed /
ignored). The exact-source parallel rerun had **1,444 / 1 / 66**: the sole
failure was the unchanged FFI test
`document_viewport::frames_for_a_replaced_ring_are_dropped`, asserting that
a dropped render record exists after a fixed 20 ms sleep. All ENG-3 tests
passed in both runs. The test assumes work is in flight within that time;
contention can instead leave it pending when the ring is replaced. This is
a timing-sensitivity diagnosis, not a proven general fix to the FFI test.
The complete six-test viewport suite passed on rerun with default threading.
The failure log is retained as `release-ring-failure.log`; no FFI code or
assertion was changed. The final complete gate uses `--test-threads=1` to
reduce contention, with no command-level test exclusions.

Final eight-crate release gate: **exit 0; 225 suites; 1445 passed,
0 failed, 66 ignored, 0 filtered**. Existing ignored tests are
unchanged; every ENG-3 regression executes on CPU/Metal without skipping.
The full gate was rerun after the Clippy-only test iterator cleanup so the
result corresponds to the exact final source commit `2e4a1339`.

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| engine-api | 120 | 0 | 0 |
| export | 100 | 0 | 7 |
| filters | 156 | 0 | 8 |
| image-core | 120 | 0 | 2 |
| pipeline-cpu | 179 | 0 | 3 |
| pipeline-gpu | 161 | 0 | 13 |
| previews | 28 | 0 | 3 |
| tessera-ffi | 581 | 0 | 30 |

[eng3c-validation.json](eng3c-validation.json) records gate commands, counts,
source hash and archived log SHA-256 values. The final documentation commit
changes no production/test code.

Logs and captures remain outside the repository at
`$HOME/tessera-evidence/ENG-3c`. No photographic pixels are committed.
Machine A remains the integration owner; this lane does not merge or restack
other lanes. The remaining rendering-policy follow-up is continuity at the
signed-channel Y=0 crossing, explicitly outside this amendment.
