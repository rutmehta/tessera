# ENG-1 — Texture/Clarity conditioning

Branch: `wp/ENG-1-texture-clarity`. Local commits only; ready for coordinator review with the rendering changes and validation limits below. No push, mailbox, board, dependency, lockfile, Swift gate, or app launch.

## Result

CPU and GPU now use the identical presence gain:

```text
gain = finite(decode(adjusted) / max(abs(luminance), 1e-3))
```

The floor is **0.1% of scene-linear Rec.2020 white (1.0)**. It is a separate policy constant from the guided-filter epsilon, despite their equal numeric values. Only the texture/clarity recombination divisor changed. Guided filters, tone math, operator ordering, profile curves, sharpening, and import/recipe semantics are unchanged. Nonpositive luminance and unchanged adjusted log luminance retain their existing bypasses. Above the floor, the original arithmetic is preserved.

The retained resident-chain scaled bound of 0.002 passes, and its absolute ceiling is tightened from 0.025 to **0.01**. The explicit 24 MP test now asserts **absolute 0.01 over all 72 million RGB samples**, in addition to the original sampled scaled guard, all-pixel finite RGB, and exact alpha. Metal was available; these were real GPU runs, not skips.

The declined `635f69d8` numerics changes were not cherry-picked or reimplemented. The product changes are confined to `pipeline-cpu/src/tone_extra.rs`, `pipeline-gpu/src/presence.wgsl`, and `pipeline-cpu/TONE_M2.md`.

## Test-first history

- Actual starting HEAD: `87ff1ff173e4d6d2053a534b7cfd9343aef906e1`, the brief-only commit immediately following requested base `e6c3e5da`.
- RED: `d01723659a7a6193ad1e323b311a56f72f7411f7` — `test(ENG-1): expose signed-luminance presence sensitivity`.
- GREEN implementation: `996e2d4d57a0fe0e87a49cc4105ff87fbb289e29` — `fix(ENG-1): condition texture and clarity luminance gain`.
- The following `docs(ENG-1):` commit contains this handoff and compact validation evidence. Its hash is the branch tip reported to the coordinator.

All commits end with the requested Claude Fable 5.1 co-author trailer. RED was observed before product changes: both one-ulp tests failed, as did the tightened resident-chain assertion. No existing assertion was weakened.

| Measurement | RED / baseline | Final 1e-3 floor |
| --- | ---: | ---: |
| CPU-only signed 17×17 patch, one blue ulp, absolute output gap | 8.01123 | 0.00000023841858 |
| Synthetic 259×263 rich chain, one source blue ulp at (84,41) | 0.10030365 | 0.000005155802 |
| Resident chain, amount 0, maximum absolute RGB gap | 0 | 0 |
| Resident chain, amount 0.35, maximum absolute RGB gap | 0.00716877 | 0.0006299019 |
| Resident chain, amount 1, maximum absolute RGB gap | 0.020483017 | 0.0017997772 |
| 24 MP all-pixel maximum absolute RGB gap | 6.1408234 (historical B5-32 baseline) | 0.0064618886 |

The full-chain one-ulp output changed from `17.700655 -> 17.600351` before the fix to `0.15071067 -> 0.15071583` after it. The signed patch uses strong negative texture/clarity and checks the operator directly, independently of Metal or downstream tone/color.

The first candidate floor, 1e-4, met the requested absolute ceiling but failed the **unchanged** scaled guard: maximum gap 0.0047510564. The final 1e-3 floor retains both guards. This choice has visible effects at some cancelling pixels, described below; it is not represented as a precision-only fix.

The 24 MP worst sample was pixel index 20,760,770, blue: GPU 0.112942874 versus CPU 0.11940476. CPU elapsed 85.529190459 s, GPU 1.299781417 s, including cold pipelines and excluding upload/readback. Shared-machine load makes these diagnostic timings, not a before/after performance claim. Historical 24 MP RED was not rerun in this lane.

## Every Develop golden: results and limitations

All three **synthetic** `filters/tests/camera_raw.rs` Develop golden tests pass: sRGB, Display P3, Adobe RGB, each at full, 0.35, and zero amount. Before/after captures compared every final encoded RGBA f32 sample (4,403 pixels per case). Capture instrumentation was temporary and removed. Both baseline and final runs passed their existing independent Develop-reference assertions.

These deltas are **larger than the brief's expected tiny changes**. No stored golden was regenerated, replaced, or weakened. These synthetic goldens construct their reference through the Develop renderer at runtime; the table separately records the change against pre-fix output, which those parity assertions alone would not detect.

| Golden | Amount | Changed pixels / 4,403 | Max absolute encoded channel delta |
| --- | ---: | ---: | ---: |
| sRGB | full | 64 | 0.2069286108 |
| sRGB | 0.35 | 63 | 0.0724250674 |
| sRGB | 0 | 0 | 0 |
| Display P3 | full | 79 | 0.1983673275 |
| Display P3 | 0.35 | 79 | 0.0694285631 |
| Display P3 | 0 | 0 | 0 |
| Adobe RGB | full | 87 | 0.1996451020 |
| Adobe RGB | 0.35 | 85 | 0.0698757768 |
| Adobe RGB | 0 | 0 | 0 |

Justification: a temporary trace of each synthetic golden recorded one floored pixel in each renderer/filter evaluation (six events across the three profile tests). Every event was signed RGB, approximately `[-0.00825019, -0.02218884, 0.2977606]`, with positive Y in `0.0004458446..0.00044586882` and luminance condition number `sum(abs(weight*channel))/Y` in `78.20358..78.20788`. The direct change is therefore restricted to a cancelling near-black pixel in these fixtures. Downstream processing propagates its change to additional output pixels. The lower 1e-4 candidate left all nine captures byte-identical but did not retain the old scaled parity bound.

The final floor attenuates the intended post-presence luminance by `Y / 1e-3` below the threshold. Consequently the original log-domain no-new-extrema constraint does not guarantee the final RGB luminance minimum there. This intentional near-black rendering policy is documented in TONE_M2.md and needs to be considered during integration review.

**Blocked coverage:** `pipeline-cpu/tests/golden.rs::raw_fixture_goldens` was invoked and reported its no-fixture skip. Its five stored RAW goldens (Canon CR3, Nikon NEF, Sony ARW, Fuji RAF, sample DNG) depend on downloaded camera photographs, not synthetic fixtures. The user's SYNTHETIC-only override takes precedence; none was read, compared, or updated. Their per-pixel deltas are unmeasured. Their default texture/clarity controls are neutral, so this change should bypass them, but that is code inspection, not a measured golden result.

The worktree-local `fixtures/raw` symlink was temporarily moved out of discovery during gates and restored afterward; its shared target was never changed. Two non-skipping RAW tests were explicitly filtered: `fixture_as_shot_roundtrip_and_slider_directions` and `fixture_level3_tolerance_per_operator_and_output`. Other absent-fixture checks reported their normal skip/unavailable messages. No real catalog was opened or used.

## Rust gates and reproduction

Every Cargo invocation used:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-1-texture-clarity
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
unset PIPELINE_RAW_FIXTURES PIPELINE_GPU_ALL_FIXTURES
```

With RAW fixtures unavailable in this worktree for the synthetic-only run:

```sh
cargo test --release -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p filters -- --nocapture --test-threads=3 --skip fixture_as_shot_roundtrip_and_slider_directions --skip fixture_level3_tolerance_per_operator_and_output
cargo clippy --all-targets -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p filters -- -D warnings
cargo fmt --all -- --check
cargo test --release -p filters --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --nocapture
```

- Release gate: **exit 0**, 114 suites, **613 passed, 0 failed, 24 ignored, 2 filtered**. Counts include tests that return early on absent RAW fixtures; they are not claims of RAW coverage.
- Clippy: **exit 0**, all targets, warnings denied.
- Fmt: **exit 0**. `git diff --check` also passed.
- Explicit 24 MP scan: **1 passed**, absolute all-pixel assertion enabled.
- Final synthetic golden capture: **3 passed**, all nine variants compared against baseline.
- Temporary tracing was removed and production/test source restored byte-for-byte to the fully gated content.

See `VALIDATION.txt` for compact captured measurements. No Swift gates or app launch were performed by instruction.

## Remaining limitation: standalone tone precision

The existing ignored `pipeline-gpu/tests/operators.rs::tone_signed_rgb_near_zero_luminance_matches_f64_reference` diagnostic was explicitly rerun using the test executable built by the full gate. It still **fails** its independent 1e-5 reference bound: CPU `0.0000184050537388597`, GPU `0.005117127928857115`. These match the unchanged baseline. The diagnostic exercises tone without texture/clarity, so a presence divisor floor cannot repair it. It remains ignored exactly as on main; no threshold or test status was changed. This does not invalidate the separately measured passing resident-chain and 24 MP 0.01 assertions, and it is not claimed fixed.

The tests establish the bounds on the specified synthetic scenes, not on every possible signed/HDR input. The existing nonpositive-luminance bypass remains unchanged.

## B5-29c compatibility / representability

No Adobe-key translation, recipe schema, Lua/XMP parsing, FFI, or retained-source handling changed. In particular, `recipe.unknown["lrcat_develop_source"]` remains untouched. There are no newly unrepresentable settings in this lane.

The 2,000-image synthetic retained-source golden retained digest `d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`. The streaming/JSON byte-identity tests pass. The synthetic 20,000-image scale test passed: counted Rust peak heap 31 MB at 2k, 51 MB at 20k inspect, 51 MB at 20k streaming; inspect 38.819533458 s and stream 45.252138625 s under shared load. Recipe bytes and source retention are unchanged; only rendering near the presence floor changes.

## ENG-1b — approved golden attribution and acceptance (2026-10-01)

This follow-up supersedes the earlier RAW blocked-coverage paragraph. The user
authorized reading the repository photographic fixtures and accepting golden
changes only when every changed pixel belongs to the conditioning fix's
support. Work remains on `wp/ENG-1-texture-clarity`, directly on `67912fef`,
without rebase or push. No production renderer, dependency, Cargo.lock, board,
app, or real Lightroom catalog changes/access were made in ENG-1b.

### Test-first and accepted changes

- `927c94f1` — `test(ENG-1b):` adds deterministic attribution/capture tooling,
  per-pixel reports, and persistent pre-fix synthetic references. The prior
  synthetic tests had runtime parity references only, not stored files.
- RED: all three new stored-reference profile assertions failed against the
  fixed renderer, at full-opacity maxima 0.20692861 (sRGB), 0.19836733 (P3),
  and 0.19964510 (Adobe RGB), exit 101.
- `cd3451c8` — `fix(ENG-1b):` accepts only the six affected reference files,
  with [acceptance report](conditioning/ACCEPTANCE.md). Zero-opacity references
  and all photographic PNGs are unchanged. Two test loops were adjusted to
  `as_chunks` to satisfy the current clippy lint; no tolerance was weakened.
- This `docs(ENG-1b):` commit appends the final handoff. Every ENG-1b commit
  ends with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.

### Predicate, evidence, and per-golden table

The [reproducible audit](conditioning/audit.py) builds pre-fix and fixed
operators in a disposable source tree and compares every final encoded RGBA
f32 bit pattern. The historical operator is pinned to
`d01723659a7a6193ad1e323b311a56f72f7411f7`; the audit verifies that the only
production difference is the authorized divisor floor and its declaration.
Production instrumentation is confined to the disposable tree. It records
active pre-fix divisors, preserving the original nonpositive and unchanged-log
bypasses. All output samples must be finite and alpha must remain bit-exact.

For each changed pixel p, the predicate is: there exists an active pre-fix
pixel s with `0 < abs(L_s) < 1e-3` and Chebyshev distance `d(p,s) <= 11`.
The sole seed is pixel index 2101, `(29,8)`. Its pre-fix luminance is
0.0004458632320165634 (sRGB), 0.0004458688199520111 (P3), or
0.0004458613693714142 (Adobe RGB).

The radius comes from `tone_extra.rs::dehaze`: radius-3 dark-channel minimum,
then two radius-4 means in `guided`, giving 3 + 2*4 = **11**. Presence's initial
changed divide is pointwise, since both versions compute the same bands first.
The audit verifies that the final global dehaze airlight RGB and confidence
are bit-identical, so those statistics do not create image-wide support.
The remaining configured color, linear-mask local exposure/saturation,
vignette, coordinate-generated grain, profile transform, and opacity blend
are pointwise; geometry is identity. See the full
[operator proof](conditioning/README.md).

Opacity 0.35 reuses the full-opacity developed-image cache; its report explicitly
names the full-opacity `trace_source`. Only the final encoded blend differs.
Opacity zero bypasses Develop and has an empty seed set.

| Golden | Amount | Changed pixels / 4,403 | Max absolute encoded channel delta | Outside predicate |
| --- | ---: | ---: | ---: | ---: |
| sRGB | 1 | 64 | 0.2069286108 | 0 |
| sRGB | 0.35 | 63 | 0.0724250674 | 0 |
| sRGB | 0 | 0 | 0 | 0 |
| Display P3 | 1 | 79 | 0.1983673275 | 0 |
| Display P3 | 0.35 | 79 | 0.0694285631 | 0 |
| Display P3 | 0 | 0 | 0 | 0 |
| Adobe RGB | 1 | 87 | 0.1996451020 | 0 |
| Adobe RGB | 0.35 | 85 | 0.0698757768 | 0 |
| Adobe RGB | 0 | 0 | 0 | 0 |

**PASS: all 457 changed pixels satisfy the predicate; no blockers.** The measured
maximum distance is 8, inside the justified bound of 11. The committed
[per-pixel report](conditioning/report.json) lists every changed pixel's
index, nearest-seed distance, and predicate result, exact divisor/global bits,
and SHA-256 of both captures. Recomparison of the baseline files in `927c94f1`
to the accepted files reproduced that report exactly. No attribution tolerance
is used. The normal stored-reference tests retain the existing 1e-4 encoded
portability tolerance in addition to their original runtime parity assertions.

### Photographic RAW goldens

Repository inventory identifies `pipeline-cpu/tests/golden.rs::raw_fixture_goldens`
as the stored photographic Develop golden test. It ran against every file in
`fixtures/raw`, with the symlink left in place. Both pre-fix and fixed runs
passed exact RGB8 comparison against all five stored PNGs. Before/after
renders are also byte-identical. Existing golden settings have neutral
texture/clarity, so their conditioning support is empty. No RAW golden updates
were necessary or performed.

| RAW / stored PNG | Pixels | Changed vs stored PNG | Max delta vs PNG | Changed pre/post fix | Max pre/post delta | Result |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Canon CR3 / canon-cr3 | 250,000 | 0 | 0/255 | 0 | 0/255 | PASS |
| Fuji RAF / fuji-raf | 249,696 | 0 | 0/255 | 0 | 0/255 | PASS |
| Nikon NEF / nikon-nef | 568,568 | 0 | 0/255 | 0 | 0/255 | PASS |
| DNG / sample | 282,968 | 0 | 0/255 | 0 | 0/255 | PASS |
| Sony ARW / sony-arw | 252,150 | 0 | 0/255 | 0 | 0/255 | PASS |

See [RAW report](conditioning/raw-report.json). The final normal release gate
also reran the photographic goldens and confirmed the same zero deltas.

### Final gates and reproduction

All Cargo runs used the requested PATH, external target directory,
`CARGO_BUILD_JOBS=3`, and `RAYON_NUM_THREADS=3`. Normal gates unset
`ENG1_CAPTURE`, `PIPELINE_RAW_FIXTURES`, and `PIPELINE_GPU_ALL_FIXTURES`.

```sh
cargo test --locked --release -p pipeline-cpu -p filters -- --nocapture --test-threads=3
cargo clippy --locked --all-targets -p pipeline-cpu -p filters -- -D warnings
cargo fmt --all -- --check
python3 -m unittest discover -s tools/orchestrate/wp/ENG-1/conditioning -v
python3 tools/orchestrate/wp/ENG-1/conditioning/audit.py --output /tmp/eng1b-fresh
# Recheck completed captures without rebuilding:
python3 tools/orchestrate/wp/ENG-1/conditioning/audit.py --verify-captures --output /tmp/eng1b-audit
```

- Final release gate: exit 0, **55 suites, 314 passed, 0 failed, 11 ignored,
  0 filtered**, after the final test-loop lint changes.
- Clippy all targets with warnings denied: exit 0.
- Fmt and `git diff --check`: exit 0.
- Attribution unit tests: **3 passed**, including one-ulp rejection outside
  support, inclusive diagonal boundary acceptance, and empty-support rejection.
- Both isolated render builds passed all 3 synthetic tests and the photographic
  test covering all 5 files. The final capture verifier passed and regenerated
  both committed JSON reports byte-for-byte. The initial tracer needed explicit
  cache provenance for opacity 0.35; the completed verifier uses the documented
  shared full-opacity render trace and retains the same radius.

The original ENG-1 standalone ignored tone-precision diagnostic remains outside
this golden-only follow-up; no claim is made that ENG-1b fixes it. The 11 ignored
release tests retain their existing status.

## ENG-1c — exact Tone attribution and omitted local GPU path

Read [the ENG-1c report](conditioning/local-gpu/README.md), including the full
compressed before/candidate per-pixel dumps and [predicate JSON](conditioning/local-gpu/report.json).

The clean release fixture reproduces `image Tone: 2.846853e-4`. Exactly one
pixel exceeds 1e-4: 69802, (307,113), in the 615x410 Sony image. It is itself
an active pre-fix seed: L=0.0008220475865527987, bits 3a577eae. **All failing
pixels satisfy the predicate; none are outside support.** Radius zero suffices.

Nevertheless this is a proven formulation mismatch: `GpuStageOp::run_image`
calls `tone_local::run`, whose `tone_local.wgsl` still divides by raw luminance.
ENG-1 only changed the separate resident `presence.wgsl`. The fixture's CPU
reference is computed at runtime, so no stored expected data exists to update.
The adapter's f16 capability is unused by both f32 presence shaders.

This contradicts the ruling's inference that all-inside-support necessarily
means an intended CPU/GPU delta. Clarification was requested before changing
production code. The branch retains its production/reference code unchanged;
`conditioning/local-gpu/candidate.patch` proposes the omitted floor. This is
**not an integrated or green branch** while that decision is pending.

Test-first commit: `8a418fa2`, with the required co-author trailer. The new
near-black local-path regression fails before the production fix at 1.5453927e-4.
In a disposable candidate checkout, the unchanged fixture passes with Tone
maximum 7.748604e-7 and no pixels above 1e-4. The pre-floor CPU trace run also
passes, independently confirming that the old GPU matches the old CPU policy.
No 1e-4 bound, runtime reference, stored golden, Cargo.lock, board, or app was
changed. All commits are local, on top of 6688d478, without rebasing.

Reproduction uses the requested lane environment, plus explicit
`cargo clean --release -p pipeline-cpu -p pipeline-gpu -p filters` before
comparing checkouts. Plain clean left stale release artifacts on this host.
See the report for candidate gate results and exact reproduction commands.

Candidate gates completed: release **456 passed, 0 failed, 24 ignored,
0 filtered** in 93 suites; clippy all targets with `-D warnings` **exit 0**;
fmt **exit 0**. These results apply to the isolated proposed patch, not to
unchanged production in the branch. Applying it awaits resolution of the
case-split contradiction above.
