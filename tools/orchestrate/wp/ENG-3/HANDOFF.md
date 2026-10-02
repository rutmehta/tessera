# ENG-3 — Curve and Photo Filter luminance divisions

Branch `wp/ENG-3-luminance-divisions`, starting at
`f6b572ba` (LR-2 through 2f). Local only, no rebase or push. Read ENG-1's
original and ENG-1d smooth-floor findings before implementation.

## Implementation and policy

Two operator sites, four production files:

- `crates/pipeline-cpu/src/tone_extra.rs` and
  `crates/pipeline-gpu/src/operators.wgsl`: luminance point-curve recombination.
- `crates/filters/src/adjust.rs` and
  `crates/filters/src/shaders/adjust.wgsl`: Photo Filter preserve luminosity.

Each has its own documented `1e-3` policy constant: 0.1% of scene-linear
Rec.2020 white. This is the same domain and epsilon as ENG-1. For positive
luminance, both CPU and GPU use exactly:

```text
L >= epsilon: gain = f / L
0 < L < epsilon: gain = 1 + (f - L) / epsilon
```

Both operators can accept signed RGB, and LR-2 extended curves explicitly
accept negative luminance. The signed extension uses `abs(L) >= epsilon` for
the original ratio and `1 + (f-L)/copysign(epsilon,L)` below it. WGSL expresses
copysign as `select(-epsilon, epsilon, L>0)`, matching Rust. This preserves
the old negative-domain ratio above the floor and interpolates output
luminance from L to f by `abs(L)/epsilon` below it. An exact zero delta has
gain one; there is no zero-delta branch that could disagree across CPU/GPU.

The curve keeps its original legacy nonpositive-luminance bypass and neutral
black lift. Photo Filter keeps its exact-zero source-RGB fallback, replacing
the old `abs(L)>1e-10` guard for every nonzero sub-floor L. Neither change
promises continuity across those retained exact-zero special cases. In
particular, positive neutral greys approach zero with the floor, while exact
black still maps to the lifted black value; that exact-zero discontinuity is
an explicit consequence of retaining the black-lift special case. This is
an intentional near-black rendering policy, not merely a precision rewrite.
Photo Filter retains `channel*f/L` evaluation order above the floor to avoid
unrelated rounding changes; below it both implementations multiply by gain.
Alpha, neutral controls, recipe semantics, and dependencies are unchanged.

## Test-first evidence

- `a4290282`: `test(ENG-3): expose near-zero luminance division sensitivity`.
  Five runnable regressions failed against unmodified production code.
- `cba7e4d1`: `fix(ENG-3): smoothly floor curve and Photo Filter luminance gains`.
  Includes signed-domain/boundary coverage and policy documentation.
- The following `docs(ENG-3):` commit contains this handoff and golden audit.

All commits end with the requested Claude Opus 5.5 trailer.

| Measurement | Before | After |
| --- | ---: | ---: |
| Curve CPU, one green-channel ulp on signed 3×3 patch | 210028.97 | 9.536743e-7 |
| Photo Filter CPU, one green-channel ulp on signed 3×3 patch | 1481721 | 7.6293945e-6 |
| Curve full-operator CPU/Metal gap, lifted black | 45968.313 | 3.8146973e-6 |
| Curve CPU zero delta vs GPU one luminance ulp | 1.1920929e-7 | 0 |
| Photo Filter CPU zero delta vs GPU one luminance ulp | 1.1920929e-7 | 0 |
| Photo Filter full-operator CPU/Metal gap, positive and negative L | not measured | 0 / 0 |

The cancellation patch has `L=5.9300003e-8`, strictly between zero and epsilon.
The controlled mapped-luminance perturbation is `3.5527137e-15`, exactly one
f32 luminance ulp. The CPU references have exact identity mapping (delta zero).
The GPU tests inject only the mapped luminance into the real production WGSL;
they do not duplicate the gain formula. Shader recombination and branch logic
execute on Metal. The curve test activates the shader's curve stage while
other channel curves are identity. Photo Filter invokes production opcode 28.

Final sensitivity bounds are **<=1e-5** for both sites, tightened from the
initial RED tests' <=1e-3. Controlled zero/ulp parity requires **<=1e-8**.
Full-operator parity requires <=1e-4. No existing bound was widened or ignored.
Additional signed curve coverage checks both sides of ±epsilon against a
flat-curve f64 oracle with <1e-7 bounds. Photo Filter verifies real full-path
Metal parity for positive/negative cancelling luminance and exact alpha.

## Golden acceptance

All nine encoded synthetic Develop captures are bit-identical before/after.
All five photographic RAW captures are byte-identical before/after, and both
runs match their immutable stored RGB8 PNGs exactly. No stored golden changed.

| Golden | Pixels | Changed pixels | Max encoded delta | Max distance |
| --- | ---: | ---: | ---: | --- |
| sRGB, amount 1 | 4403 | 0 | 0 | n/a |
| sRGB, amount 0.35 | 4403 | 0 | 0 | n/a |
| sRGB, amount 0 | 4403 | 0 | 0 | n/a |
| Display P3, amount 1 | 4403 | 0 | 0 | n/a |
| Display P3, amount 0.35 | 4403 | 0 | 0 | n/a |
| Display P3, amount 0 | 4403 | 0 | 0 | n/a |
| Adobe RGB, amount 1 | 4403 | 0 | 0 | n/a |
| Adobe RGB, amount 0.35 | 4403 | 0 | 0 | n/a |
| Adobe RGB, amount 0 | 4403 | 0 | 0 | n/a |
| Canon CR3 | 250000 | 0 | 0/255 | n/a |
| Fuji RAF | 249696 | 0 | 0/255 | n/a |
| Nikon NEF | 568568 | 0 | 0/255 | n/a |
| DNG | 282968 | 0 | 0/255 | n/a |
| Sony ARW | 252150 | 0 | 0/255 | n/a |

The intended direct-change predicate is active pre-fix `0<abs(L)<1e-3` at
the same pixel (pointwise recombination; “active” means the operator is
enabled, including Photo Filter’s former tiny-L fallback). These changes occur after presence
and dehaze. Any downstream spatial propagation would need a justified support
radius before accepting a changed reference. Here the changed set is empty:
zero outside-predicate pixels, no distance to measure, and no seed tracing or
spatial-support assumptions are needed for acceptance. The exact comparison
script deliberately rejects *any* nonempty diff pending a seed audit.

[golden-report.json](golden-report.json) records per-file counts and SHA-256
for both captures. [compare_captures.py](compare_captures.py) reproduces this
comparison without any tolerance and checks finiteness and alpha bits.
Capture directories are archived outside the repository:
`/Users/rutmehta/tessera-evidence/ENG-3/{before,after}`.
The baseline was captured before product edits. Capture mode bypasses only
the stored synthetic-reference check; its existing independent runtime
Develop-parity check still ran. Normal full gates run with capture mode unset.
`fixtures/raw` remained available throughout, with all five repository photos.

## Other division sites audited

Searched Rust and WGSL under pipeline-cpu, pipeline-gpu, and filters for
luminance denominators, division expressions, gain/scale recombination, and
luma-derived variables. No additional clear black-lift analogue was changed.

| Sites | Finding / disposition |
| --- | --- |
| `pipeline-cpu/src/tone_extra.rs` parametric curve; `pipeline-gpu/src/operators.wgsl::curves` parametric block | Unfloored positive-Y ratio, but native warp is anchored at zero with bounded derivative and no black lift. Reported, unchanged. |
| `pipeline-cpu/src/lib.rs::tone`; `pipeline-gpu/src/operators.wgsl::tone` | Unfloored basic-tone ratio. Analytic curve is zero-anchored, but finite-precision softplus differences can produce a cancellation residual. This is the separate ENG-4 tone precision issue documented by ENG-1, not silently changed here. |
| `pipeline-cpu/src/legacy_pv2010.rs::apply`, final `out/y` | Unfloored positive-Y ratio. Fill/brightness are zero-anchored; negative contrast permits a sublinear power and unbounded mathematical gain as Y approaches zero. Different operator semantics; reported for follow-up. |
| Same legacy operator, `recovered/y` | Guard `y>0.75` prevents near-zero division. |
| `pipeline-cpu/src/display.rs::{display_float,display,display_linear}`, `pipeline-cpu/src/output.rs`, `pipeline-gpu/src/operators.wgsl::{display,display_linear}`, `pipeline-gpu/src/managed_output.wgsl` | Positive-Y sigmoid/luminance ratios without a floor. Default/EDR sigmoid exponent 1.5 gives vanishing near-zero gain. Configurable CPU sigmoid contrast/skew can yield sublinear response and unbounded gain; changing it would alter display semantics. Reported, unchanged. |
| `pipeline-cpu/src/lens_blur.rs`, specular boost `1/y` | Guard `y>1` prevents near-zero division. |
| `pipeline-cpu/src/tone_extra.rs::presence`, `pipeline-gpu/src/{presence,tone_local}.wgsl` | Already use the ENG-1 smooth `1e-3` floor; unchanged. |
| Dehaze transmission, optics edge normalization, chroma/gamut and HSL ratios | Bounded denominators or different variables/semantics, not an unfloored luminance recombination analogue. |

## Gates and reproduction

Every Cargo run used:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-3
export CARGO_BUILD_JOBS=4
export RAYON_NUM_THREADS=4
```

Normal gates unset `ENG1_CAPTURE`, `PIPELINE_RAW_FIXTURES`, and
`PIPELINE_GPU_ALL_FIXTURES`:

```sh
cargo clean --release -p pipeline-cpu -p pipeline-gpu -p filters
cargo test -p pipeline-cpu -p pipeline-gpu -p filters --release --no-fail-fast
cargo test --release -p tessera-ffi
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Release clean: exit 0, 73 files / 82.4 MiB removed.
- Three-crate release: **exit 0; 97 suites; 488 passed, 0 failed,
  24 ignored, 0 measured, 0 filtered**. Includes all seven ENG-3 regressions,
  all Develop/filters goldens, photographic RAW goldens, and the legacy
  operator scalar goldens. No retries or skip filters.
- FFI release: **exit 0; 53 suites; 581 passed, 0 failed,
  30 ignored, 0 measured, 0 filtered**. No retry.
- Workspace Clippy, all targets, `-D warnings`: **exit 0**. Existing LibRaw
  native-compiler deprecation messages did not produce a Rust Clippy failure.
- Workspace fmt check: **exit 0**. `git diff --check`: **exit 0**.

The exact commands, gate durations, suite totals, and log SHA-256 values are
recorded in [validation.json](validation.json). All gates used code revision
`cba7e4d1`; the final documentation commit adds this record and clarifies the
exact-zero boundary consequence in TONE_M2.md, without changing Rust/WGSL.

Logs are archived at `/Users/rutmehta/tessera-evidence/ENG-3/`.


Capture reproduction, once with the pre-fix revision and once with the fix:
create the capture directory, set `ENG1_CAPTURE` to it, then run
`cargo test --release -p filters --test camera_raw full_develop_matches_rgb_decode_golden -- --nocapture --test-threads=1`
and `cargo test --release -p pipeline-cpu --test golden -- --nocapture`.
Compare the two directories with:

```sh
python3 tools/orchestrate/wp/ENG-3/compare_captures.py \
  /Users/rutmehta/tessera-evidence/ENG-3/before \
  /Users/rutmehta/tessera-evidence/ENG-3/after \
  --output tools/orchestrate/wp/ENG-3/golden-report.json
```

No Cargo.lock, board.json, dependency, Swift, or app changes. No Swift gate,
app launch, remote write, or real catalog access. The existing ignored ENG-4
tone diagnostics/benchmarks retain their status; this lane does not claim to
fix or freshly measure them.
