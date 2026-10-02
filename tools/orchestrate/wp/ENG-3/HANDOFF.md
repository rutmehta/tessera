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
