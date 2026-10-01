# ENG-1c Tone fixture investigation

The branch's current production code has a confirmed CPU/GPU formulation
mismatch. There is also a contradiction in the requested case split: **all
pixels exceeding 1e-4 satisfy the conditioning predicate**, but the GPU has
not implemented the conditioning on this execution path. Predicate membership
alone therefore cannot establish that a CPU/GPU parity failure is intended.
There is no stored fixture reference to update: `Audited::run_image` computes
its reference through `CpuStageOp` on the exact same input on every call.

Pending the requested clarification, `candidate.patch` is a validated proposed
fix, not an applied production change. The existing 1e-4 fixture bound and its
runtime reference are unchanged. The test-first commit is `8a418fa2`.

## Exact attribution

The Sony ARW fixture's Tone image is 615 x 410 (252,150 pixels). Exactly one
pixel exceeds 1e-4: index **69802**, **(307,113)**. Its maximum absolute channel
difference is **0.00028468528762459755**, in blue:

| Channel | CPU | GPU before proposed fix |
| --- | ---: | ---: |
| R | 0.00005793571472167969 | 0.00007048177212709561 |
| G | 0.0005763024091720581 | 0.0007010477711446583 |
| B | 0.0013154298067092896 | 0.001600115094333887 |

The active pre-fix divisor at this pixel is **0.0008220475865527987**,
f32 bits **3a577eae**. It is the sole active seed on the audited image and is
itself below 1e-3: nearest-seed distance **0**, outside-predicate count **0**.
No downstream support expansion or assumption about global dehaze invariance
is needed to classify this failing pixel. The ENG-1b radius 11 is included in
the report for comparability, but radius 0 already accepts it.

The trace restores only the CPU divisor expression in a disposable checkout,
then uses the ENG-1b instrumentation after both bypasses and before division.
Guided bands, adjusted-log values, and active decisions are unchanged by the
floor, so these are the exact pre-fix seeds. The captured input SHA-256 is
identical between the failing and pre-fix runs. The pre-fix fixture passes.

`pre-fix-trace.tsv` records four evaluations: audited GPU-upstream input, the
separate end-to-end CPU renderer, then the same two for display output. The
CPU renderer's slightly different input has divisor bits 3a577ea5; the report
retains the **first** seed event, which belongs to the audited input. Global
rows are raw observations across those evaluations, not an assertion that
CPU/GPU global statistics are bit-identical.

`before-diff.csv.gz` and `after-diff.csv.gz` contain **every pixel**, including
zero differences, with coordinates and signed GPU-minus-CPU R/G/B differences.
The latter is the proposed candidate's output. `report.json` records source
capture hashes, dump hashes, seeds, all failing pixels and predicate results.
No photographic source image is duplicated in these reports.

## Root cause and proposed fix

`GpuStageOp::run_image` in `batch.rs` calls `tone_local::run` for this image.
That module loads `tone_local.wgsl`, whose mode 6 still evaluates:

```text
gain = finite(decode(adjusted) / lum)
```

ENG-1 changed the CPU and **resident** `presence.wgsl`, but omitted this
whole-image shader. The proposed patch makes all three use:

```text
gain = finite(decode(adjusted) / max(abs(lum), 1e-3))
```

The positive-luminance and changed-log bypasses remain identical. The floor is
applied to the divisor before division; it is not a clamp on gain. The shaders
use explicit f32 declarations with no `enable f16`, f16 storage, or f16 branch.
The adapter advertises shader_f16 capability, but these kernels do not use it.
No abs/sign/select discrepancy exists in the already-fixed resident shader.

## Test-first evidence

- Unmodified production, clean release fixture: **FAIL**, Tone 2.846853e-4.
- New `local_presence_conditions_near_black_like_cpu`: **FAIL**, first case
  scale 0.0002, texture 20, clarity 0, error 1.5453927e-4, bound 1e-6.
- The regression covers below/near/above-floor nonconstant images and separate
  texture, clarity, combined, and negative controls.
- Candidate whole-image shader fix: unchanged fixture **PASS**, Tone maximum
  **7.748604e-7**; scene-linear, display and DeltaE output assertions pass.

## Reproduction

Use the lane environment from HANDOFF.md. All work is local. Plain
`cargo clean -p ...` did not remove release artifacts in the initial run;
`--release` is essential with this shared target directory. A stale binary
initially referred to another checkout's CARGO_MANIFEST_DIR. That run is not
counted as a numerical reproduction.

```sh
cargo clean --release -p pipeline-cpu -p pipeline-gpu -p filters
ENG1C_CAPTURE=/tmp/eng1c-before cargo test --locked --release -p pipeline-gpu --test fixtures fixture_level3_tolerance_per_operator_and_output -- --nocapture
cargo test --locked --release -p pipeline-gpu --lib local_presence_conditions_near_black_like_cpu -- --nocapture
python3 tools/orchestrate/wp/ENG-1/conditioning/local-gpu/trace.py --output /tmp/eng1c-trace
python3 tools/orchestrate/wp/ENG-1/conditioning/local-gpu/trace.py --candidate --output /tmp/eng1c-candidate
python3 tools/orchestrate/wp/ENG-1/conditioning/local-gpu/report.py --before /tmp/eng1c-before --trace /tmp/eng1c-trace/trace.tsv --after /tmp/eng1c-candidate --output /tmp/eng1c-report
```

The first two commands intentionally fail until the proposed fix is applied.
`trace.py` archives HEAD into a disposable directory, restores the old CPU
divisor for the trace, or applies only the proposed local shader fix for the
candidate. It cleans release artifacts before either run. Candidate mode also
runs the complete requested release/clippy/fmt gates. It never changes the
worktree production source, references, Cargo.lock, board.json, or an app.

## Candidate gate results

The full release gate passed: **93 suites, 456 passed, 0 failed, 24 ignored,
0 filtered**. This includes the new regression, the original photographic
fixture, and the stored RAW goldens. These are disposable candidate results;
production code in this branch still has the diagnosed omission.

Candidate `cargo clippy --locked --all-targets -p pipeline-cpu -p pipeline-gpu
-p filters -- -D warnings` and `cargo fmt --all -- --check` both exited 0.
The compressed dumps were independently reread to verify their hashes, row
counts, maxima and failing indices. `git apply --check candidate.patch` and
`git diff --check` also pass. See `VALIDATION.txt` for the compact gate record.

The compressed per-pixel dumps are external evidence, not test inputs. ENG-1d
moved them to the scratchpad archive; see HANDOFF.md for the absolute path and
SHA-256 hashes. They are no longer shipped in the repository.
