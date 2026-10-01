# ENG-1 — Texture/Clarity conditioning

Branch: `wp/ENG-1-texture-clarity`. Local commits only; M1 is implemented under Machine A’s ENG-1e ruling. ENG-1f found a new 24 MP absolute-bound failure, so the lane is not fully green; ENG-4 owns the remaining tone-stage gap. No push, mailbox, board, dependency, lockfile, Swift gate, or app launch.

**Current acceptance data is in the ENG-1f section below; preceding lane
measurements are retained as historical evidence.**

## Result

CPU and GPU now use the identical presence gain:

```text
gain = finite(L >= epsilon ? decode(adjusted) / L
                           : 1 + (decode(adjusted) - L) / epsilon)
epsilon = 1e-3
```

The floor is **0.1% of scene-linear Rec.2020 white (1.0)**. It is a separate policy constant from the guided-filter epsilon, despite their equal numeric values. Only the texture/clarity gain recombination changed. Guided filters, tone math, operator ordering, profile curves, sharpening, and import/recipe semantics are unchanged. Nonpositive luminance retains its existing bypass. Above the floor, original arithmetic and the unchanged-log bypass are preserved. Below the floor, the continuous expression is evaluated even for unchanged adjusted log luminance. The final ENG-1d section supersedes historical floor-policy measurements below.

ENG-1e retired the small-fixture resident-chain 0.002 scaled guard under Machine A’s ruling, retained the **0.01 absolute** ceiling, and added a presence-off **<= 0.005 absolute** pin for the tone gap owned by ENG-4. The explicit 24 MP test asserts **absolute 0.01 over all 72 million RGB samples**, all-pixel finite RGB, and exact alpha. Its current ENG-1f measurements and sampled scaled-guard status are recorded below. Metal was available; these were real GPU runs, not skips.

The declined `635f69d8` numerics changes were not cherry-picked or reimplemented. The product changes are confined to four files: `pipeline-cpu/src/tone_extra.rs`, `pipeline-gpu/src/presence.wgsl`, `pipeline-gpu/src/tone_local.wgsl`, and `pipeline-cpu/TONE_M2.md`.

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

Read [the ENG-1c report](conditioning/local-gpu/README.md), with the archived
before/candidate per-pixel dumps referenced by SHA-256 below and [predicate JSON](conditioning/local-gpu/report.json).

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
production code. That historical candidate is now integrated by ENG-1c; the branch subsequently
passed the unchanged fixture bound. ENG-1d validation below supersedes the
candidate-only status and records gates on this worktree.

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
fmt **exit 0**. Those historical results applied to the isolated candidate. ENG-1c subsequently
integrated that patch; ENG-1d below records validation of the current worktree.

### Per-golden conditioning footprint (requested by Machine A review)

Derived from `conditioning/report.json` at 6688d478 (`changed_pixel_evidence[].distance`, Chebyshev distance to the nearest pre-fix seed with 0 < |L| < 1e-3; bound 11).

| Golden | Changed pixels / 4,403 | Max encoded delta | Max distance | Outside predicate |
| --- | ---: | ---: | ---: | ---: |
| sRGB, amount 1 | 64 | 0.2069286108 | 8 | 0 |
| sRGB, amount 0.35 | 63 | 0.0724250674 | 8 | 0 |
| sRGB, amount 0 | 0 | 0 | n/a | 0 |
| Display P3, amount 1 | 79 | 0.1983673275 | 8 | 0 |
| Display P3, amount 0.35 | 79 | 0.0694285631 | 8 | 0 |
| Display P3, amount 0 | 0 | 0 | n/a | 0 |
| Adobe RGB, amount 1 | 87 | 0.1996451020 | 8 | 0 |
| Adobe RGB, amount 0.35 | 85 | 0.0698757768 | 8 | 0 |
| Adobe RGB, amount 0 | 0 | 0 | n/a | 0 |

No golden moved outside the conditioning footprint. Photographic RAW goldens: 0 changed pixels each.

### ENG-1c outcome (coordinator)

Case (3) of Machine A's ruling: a CPU/GPU formulation mismatch. `tone_local.wgsl` (whole-image GPU tone path used by `fixture_level3_tolerance_per_operator_and_output`) still divided by raw luminance; the single pixel over 1e-4 (index 69,802, (307,113), L = 8.22e-4) is the conditioning seed itself. The validated candidate patch is applied as a production fix: identical `max(abs(L), 1e-3)` floor (**historical; superseded by ENG-1d’s continuous recombination formula below**). After a clean rebuild (`cargo clean -p pipeline-cpu -p pipeline-gpu -p filters`) the fixture passes at the unchanged 1e-4 bound. The fixture's CPU reference is computed at runtime, so no stored reference changed. A transient `white_balance_v2` RAW-open failure in the first full run passed 3/3 when re-run alone; full no-fail-fast run recorded below.

## ENG-1d — continuous recombination and actual-worktree gates (2026-10-01)

**M1 fixed; not merge-green.** The full required release run on the actual
worktree fails only the resident test's existing **0.002 scaled guard**.
Its **0.01 absolute ceiling passes**. No bound has been relaxed or removed.
This section supersedes the earlier attenuating-floor policy and gate status.

### Formula and test-first evidence

All three sites (`tone_extra.rs`, `presence.wgsl`, `tone_local.wgsl`) now use:

```text
epsilon = PRESENCE_LUMA_FLOOR = 1e-3
if L > 0:
    gain = L >= epsilon ? decode(adjusted)/L
                        : 1 + (decode(adjusted) - L)/epsilon
```

The above-floor unchanged-log bypass and arithmetic are preserved exactly.
Below the floor no `adjusted != z` special case remains. Nonpositive luminance
still bypasses presence. The shared numeric epsilon stays separate from the
guided-filter epsilon. Production/policy changes remain confined to **4 files**:
the three implementations and `pipeline-cpu/TONE_M2.md`.

The pre-existing **L = 0 discontinuity** is deliberately not fixed: the
nonpositive bypass can meet a large gain (approximately 180x in the reviewed
case) immediately above zero. It predates ENG-1 and remains outside this ruling.

- `c6bb544f`: first RED test plus enforcing each rich-chain stage's 0.01 bound.
- `79f9a01e`: extend the test to both GPU recombination paths; RED rerun with
  the three original production files from `5b0871ae`, then restore the fix.
- `e7d9cd3c`: continuous formula and six attribution-approved synthetic references.
- Final `docs(ENG-1d):` commit: this record and removal of unused evidence dumps.

`presence_zero_delta_vs_one_ulp_matches_cpu_below_floor` uses a signed centre
sample at L = 0.0004458446. A one-pixel CPU image makes fine/mid identical,
so its delta is exactly zero. Controlled GPU guided-band buffers differ by
one log-luminance ulp (1.1641532e-10); the neighbour range permits it. The local
production mode-6 shader and resident production `presence_from` helper both
execute on Metal. The test copies no gain formula; it isolates recombination
from platform-dependent guided sums and originally asserted each path against CPU at 0.01 (tightened to 1e-5 in ENG-1f).

| GPU path | RED gap | GREEN gap |
| --- | ---: | ---: |
| Local | 0.16500479 | 0.00000059604645 |
| Resident | 0.16500479 | 0.000000059604645 |

### Regenerated conditioning table

Compared against the same historical pre-conditioning baseline `d0172365`
(stored files at `927c94f1`). Exact RGBA f32 comparisons retain the same active
pre-fix seed predicate `0 < abs(L) < 1e-3`, same radius **11**, alpha identity,
and bit-identical global dehaze airlight/confidence requirements. The sole seed
remains pixel 2101, (29,8). The audit explicitly excludes historical zero-delta
bypasses from the seed set even though the new formula evaluates them below
floor; outside-support changes would still fail acceptance.

| Golden | Changed pixels / 4,403 | Max encoded delta | Max distance | Outside predicate |
| --- | ---: | ---: | ---: | ---: |
| sRGB, amount 1 | 38 | 0.000294983387 | 8 | 0 |
| sRGB, amount 0.35 | 38 | 0.0001032352448 | 8 | 0 |
| sRGB, amount 0 | 0 | 0 | n/a | 0 |
| Display P3, amount 1 | 57 | 0.0002828836441 | 8 | 0 |
| Display P3, amount 0.35 | 55 | 9.900331497e-05 | 8 | 0 |
| Display P3, amount 0 | 0 | 0 | n/a | 0 |
| Adobe RGB, amount 1 | 44 | 0.0002887845039 | 8 | 0 |
| Adobe RGB, amount 0.35 | 42 | 0.0001010894775 | 8 | 0 |
| Adobe RGB, amount 0 | 0 | 0 | n/a | 0 |

**274 changed pixels, zero outside predicate; maximum distance 8.** Six
nonzero-opacity references were accepted only after this audit passed. All
zero-opacity references and photographic PNGs are unchanged. Every accepted
reference matches `after_sha256` in [report.json](conditioning/report.json);
every historical file matches its `before_sha256`. The audit was rerun in
verification mode and reproduced both reports. Attribution unit tests: 3/3.

Photographic RAW captures with `fixtures/raw` present throughout:

| RAW | Pixels | Changed vs PNG | Changed pre/post | Max delta | Outside predicate |
| --- | ---: | ---: | ---: | ---: | ---: |
| Canon CR3 | 250,000 | 0 | 0 | 0 | 0 |
| Fuji RAF | 249,696 | 0 | 0 | 0 | 0 |
| Nikon NEF | 568,568 | 0 | 0 | 0 | 0 |
| DNG | 282,968 | 0 | 0 | 0 | 0 |
| Sony ARW | 252,150 | 0 | 0 | 0 | 0 |

Both historical and new builds passed exact stored-PNG checks. The normal
full release run also passed `raw_fixture_goldens`. See
[raw-report.json](conditioning/raw-report.json). Captures/build logs are in
`/tmp/eng1d-audit-v3`; the audit never rewrites photographic references.

### Full actual-worktree validation and remaining blocker

Code/test/golden tip tested: `e7d9cd3cb59b9e7cf46edd45c5ea0929933ecd03`,
on top of `5b0871ae`, without rebasing. The subsequent docs commit changes no
Rust, WGSL, tests, or goldens. This was the actual worktree, not a candidate
checkout. All runs used the requested external target, three Cargo jobs and
three Rayon threads; `ENG1_CAPTURE`, `PIPELINE_RAW_FIXTURES`, and
`PIPELINE_GPU_ALL_FIXTURES` were unset for normal gates. The RAW symlink stayed
present. Exact commands:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-1-texture-clarity
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
unset ENG1_CAPTURE PIPELINE_RAW_FIXTURES PIPELINE_GPU_ALL_FIXTURES
cargo clean -p pipeline-cpu -p pipeline-gpu -p filters
cargo clean --release -p pipeline-cpu -p pipeline-gpu -p filters
cargo test -p pipeline-cpu -p pipeline-gpu -p filters --release --no-fail-fast
cargo clippy -p pipeline-cpu -p pipeline-gpu -p filters --all-targets -- -D warnings
cargo fmt --all -- --check
```

The requested plain clean removed 392 files / 146.4 MiB. The additional
release clean removed 784 files / 1.4 GiB, avoiding the stale release artifacts
previously observed in ENG-1c.

- Full release: **exit 101; 93 suites; 456 passed, 1 failed, 24 ignored,
  0 filtered**. Sole failing target: `filters --test camera_raw_gpu`.
- `resident_chain_matches_cpu_across_tile_edges_and_preserves_alpha`: amount
  0 → 0; amount 0.35 → 0.0016056895; amount 1 → **0.00458771** (both absolute
  and scaled). The amount-1 result passes 0.01 but fails the existing 0.002
  scaled assertion. Running the built `camera_raw_gpu` binary alone reproduced
  it exactly: **9 passed, 1 failed, 1 ignored**, exit 101.
- Stage isolation: every stage asserts its bound and passes (standalone 5e-6,
  rich 0.01). Encoded rich and rich-no-dehaze peak at pixel 10703, blue,
  GPU 0.4807913 / CPU 0.4762036. **Rich-no-presence is already 0.0043850243**
  at that pixel, GPU 0.4546297 / CPU 0.45024467. This points to the pre-existing
  tone precision limitation documented above; the old attenuating floor had
  kept the full-chain discrepancy under the tighter scaled guard. Tone math
  was not changed and the guard was not weakened. Further numerical work
  requires a scope decision; merge remains blocked meanwhile.
- `fixture_level3_tolerance_per_operator_and_output`: **PASS** at unchanged
  1e-4. The near-black local-path regression and new one-ulp test pass.
- `white_balance_v2`: **3 passed**, no `LibRaw error -100009`; no retry needed.
- Clippy all targets, `-D warnings`: **exit 0**.
- Fmt and `git diff --check`: **exit 0**.
- Ignored diagnostics/benchmarks, including the standalone tone reference and
  explicit 24 MP benchmark, retain their status; their old measurements above
  are historical and are not represented as current ENG-1d results.

Compact record: [VALIDATION-ENG-1d.txt](VALIDATION-ENG-1d.txt). Full logs are
archived under the evidence directory below in `eng1d/`.

### External compressed evidence

No test reads the two compressed ENG-1c per-pixel dumps. They were moved out
of the repository to:

```text
/Users/rutmehta/tessera-evidence/ENG-1/
```

| File | Bytes | SHA-256 |
| --- | ---: | --- |
| before-diff.csv.gz | 2157613 | `4c5903908684dc77f85f28ffa0362116bbb58f1507208e44b59096f8ed22da26` |
| after-diff.csv.gz | 2157630 | `7ea5dd9a4feaf3701980c7769678127f8a6915d2d52f18ce99f4d0e98769b718` |

All ENG-1d commits end with the requested Claude Opus 5.5 co-author trailer.
No Cargo.lock, board.json, dependency, or app changes; local commits only.


## ENG-1e — ruled absolute bound and pinned tone baseline (2026-10-01)

**Required gates green under Machine A option (a).** This section supersedes
ENG-1d's scaled-guard blocker. Changes are committed locally on
`wp/ENG-1-texture-clarity`, on top of `67e18d53`, without rebasing.
Test commit: `d14f99b4bef071d4581a572446b7d2f7c70bad61`.

### Ruling and regression pin

In `resident_chain_matches_cpu_across_tile_edges_and_preserves_alpha`, retire
the full-chain 0.002 scaled guard and retain the existing **0.01 absolute**
assertion. All-pixel finite RGB and bit-exact input alpha checks remain;
CPU reference RGB is also checked for finiteness. Amount zero still checks
bit-exact RGB identity.

Add the same chain at amount 1 with only texture and clarity disabled and
require **<= 0.005 absolute**. This explicitly pins the pre-existing tone gap,
so the ruling is not a silent loosening. The test comment names **ENG-4**,
records pixel 10703, blue, GPU 0.4546297 / CPU 0.45024467, and states that
ENG-4 will close the tone-stage gap and then restore the full-chain guard to
**0.002 scaled**. No production tone or presence math changed in this lane.

### Reproduced measurements and stage isolation

| Resident chain | Maximum absolute RGB gap |
| --- | ---: |
| Full chain, amount 0 | 0 |
| Full chain, amount 0.35 | 0.0016056895 |
| Full chain, amount 1 | 0.00458771 |
| Presence disabled, amount 1 | 0.0043850243 |

The release binary's stage-isolation rerun reproduced ENG-1d: encoded rich
and rich-no-dehaze both peak at pixel 10703, blue, GPU 0.4807913 /
CPU 0.4762036 (gap 0.00458771). Encoded rich-no-presence already has gap
0.0043850243 at that pixel, GPU 0.4546297 / CPU 0.45024467. Encoded
rich-no-sharpen is 0.000062949955. Linear-curve rich combinations are at most
0.000018715858; standalone stages remain within 5e-6 and every rich stage
passes its 0.01 assertion. These results preserve the attribution to the
pre-existing tone precision limitation; ENG-4 is the numerical follow-up.

### Actual-worktree gates

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-1-texture-clarity
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
unset ENG1_CAPTURE PIPELINE_RAW_FIXTURES PIPELINE_GPU_ALL_FIXTURES
cargo clean --release -p pipeline-cpu -p pipeline-gpu -p filters
cargo test -p pipeline-cpu -p pipeline-gpu -p filters --release --no-fail-fast
cargo clippy -p pipeline-cpu -p pipeline-gpu -p filters --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Release clean: **exit 0**, 569 files / 1.2 GiB removed.
- Full release run: **exit 0; 93 suites; 457 passed, 0 failed, 24 ignored,
  0 measured, 0 filtered**. `camera_raw_gpu`: 10 passed, 1 ignored.
- `white_balance_v2`: **3 passed**, no LibRaw -100009; no retry needed.
- Measurement rerun of the built release binary, resident chain plus stage
  isolation with `--nocapture`: **2 passed, 0 failed, 9 filtered**. This is
  separate from the full-run counts above.
- Clippy all targets with `-D warnings`: **exit 0** (LibRaw's existing native
  compiler deprecation messages remain; no Rust clippy failure).
- Fmt and `git diff --check`: **exit 0**.

Local logs: `/tmp/tessera-eng1e/{clean,test,clippy,fmt,measurements}.log`.
The subsequent docs commit changes only this handoff. Both ENG-1e commits
carry the requested Claude Opus 5.5 co-author trailer. No Cargo.lock,
board.json, dependency, app, or remote changes.


## ENG-1f — strict assertions, 24 MP verification, durable archives (2026-10-01)

On top of `356f014d`, without rebasing. **The explicit ignored 24 MP bench
fails its retained absolute 0.01 guard.** No assertion was weakened and no
production math, golden, Cargo.lock, board.json, dependency, or app changed.

### Assertion repairs

- Stage isolation restores the original `worst.0 >= bound` failure semantics:
  equality fails, including at the standalone 5e-6 threshold.
- The zero/one-ulp presence regression now requires `worst <= 1e-5`. Its
  maximum includes both local and resident GPU paths, so both must satisfy it.

### Explicit 24 MP Metal run

Ran once with the requested external target and three Cargo/Rayon workers:

```sh
cargo test -p filters --release --test camera_raw_gpu -- --ignored bench_24mp_cpu_gpu --nocapture
```

- **Exit 101: 0 passed, 1 failed, 0 ignored, 10 filtered.** This executed on
  Metal; `GpuContext` requires the Metal backend and this test unwraps creation.
- CPU: **56.073596542 s**; GPU: **1.975050042 s**, with cold pipelines included
  and upload/readback excluded. These are diagnostic shared-machine timings.
- All-pixel maximum absolute RGB gap: **0.01663196**, at pixel **4,999,168**
  (x=1168, y=833), red; GPU **0.5281682**, CPU **0.51153624**.
- The original sampled scaled **< 0.002** guard passed for every sampled
  pixel. It does not sample the worst pixel (`4,999,168 % 997 = 210`). The
  test did not print a numerical sampled maximum, so none is claimed here.
- All-pixel finite GPU RGB and bit-exact alpha checks completed successfully;
  failure occurred at the final **absolute < 0.01** assertion.
- Machine A’s conditional authorization was to retire a *failing sampled
  scaled guard* and add the ENG-4 presence-off <= 0.005 pin. That condition
  did not occur: both existing bench guards remain unchanged, and no new
  24 MP presence-off result is claimed. The 0.01 absolute failure is an
  unresolved blocker requiring follow-up; the normal suite ignores this bench.

Log: `/Users/rutmehta/tessera-evidence/ENG-1/eng1f/bench-initial.log`.

### Durable evidence verification

Recomputed SHA-256 from the durable archive for **all ten referenced files**:
`before-diff.csv.gz`, `after-diff.csv.gz`, and the eight `eng1d/*.log` files
listed in [VALIDATION-ENG-1d.txt](VALIDATION-ENG-1d.txt). All match their
recorded hashes exactly. Dump sizes also remain 2,157,613 and 2,157,630 bytes.
HANDOFF, the validation record, and the local-GPU README/report now point to
`/Users/rutmehta/tessera-evidence/ENG-1/`; the report explicitly says the dumps
are archived outside the repo and includes each SHA-256. No other persisted
report references the removed dumps. `report.py` retains its output filenames
for future report generation; it does not reference a missing input dump.
The result summary no longer claims the retired resident scaled bound passes;
the old ENG-1c formula is explicitly marked superseded by ENG-1d.


### Actual-worktree gates

Test commit: `2e6182d0808cf2bbd892a2ea1e9d0fe5b40d9abe`. Commands ran
in this worktree, with `fixtures/raw` present:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-1-texture-clarity
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
unset ENG1_CAPTURE PIPELINE_RAW_FIXTURES PIPELINE_GPU_ALL_FIXTURES
cargo clean --release -p pipeline-cpu -p pipeline-gpu -p filters
cargo test -p pipeline-cpu -p pipeline-gpu -p filters --release --no-fail-fast
cargo test -p tessera-ffi --release
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Release clean: **exit 0**, 596 files / 1.3 GiB removed.
- Three-crate release gate: **exit 0; 93 suites; 457 passed, 0 failed,
  24 ignored, 0 measured, 0 filtered**. `camera_raw_gpu`: 10 passed, 1 ignored.
  Both repaired assertions pass. `white_balance_v2`: 3/3; RAW stored goldens
  and level-3 unchanged 1e-4 tolerance pass, with no retry.
- FFI release gate: **exit 0; 51 suites; 562 passed, 0 failed, 29 ignored,
  0 measured, 0 filtered**. No retry.
- Workspace Clippy, all targets with `-D warnings`: **exit 0**. Existing
  LibRaw native-compiler deprecation messages did not produce a Rust Clippy failure.
- Fmt check and `git diff --check`: **exit 0**.

The normal gates are green; the explicitly run ignored bench is **not green**.
No repeated runs or changed thresholds were used to hide the bench failure.
All current logs are archived outside the repo in
`/Users/rutmehta/tessera-evidence/ENG-1/eng1f/`:

| File | SHA-256 |
| --- | --- |
| bench-initial.log | `804512e637578ed15cdeb463863e6ff8a32e086dd7576ea9c4806c2e3b9ffeb2` |
| clean.log | `9ed62725cf451299f16b1022711a174cdb727611738253499d2a957c2a595eee` |
| test.log | `af4bc37de82ad38583ba1fb7f67920298c7868366914b70290ddbb37beafcc98` |
| ffi.log | `91852357ea8c826ee0b913d5757f2e39d642e02660527743f316473fc18a2bc6` |
| clippy.log | `83db5b8e4285374dedaf0e600eafa46f3841d42688cff50a10a618e80239ee14` |
| fmt.log | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
| gate-results.json | `79179e1d0c51f26563c1c1b3d35ddd2414ed8d04b6dfb7770d102ab0ca234d40` |

Both ENG-1f commits end with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
The docs commit changes only evidence references and this validation record.
All work remains local on the existing branch, with no rebase or remote action.

## ENG-1g — 24 MP measurement and stage attribution (2026-10-01)

Measurement only, directly on `3e565432`, without rebase. Tip measured at
`3e565432`; base is the local `origin/main` ref resolved once to
`87536669ea555535ebb6622a17848f2b0fb530f3` (no fetch or remote action).
Base was built in `git worktree add --detach /tmp/tessera-ENG-1g-base origin/main`
with its own `~/.cache/tessera-target/ENG-1-base`. The temporary base worktree
and its target directory were deleted after measurements. No production code,
existing tests, bounds, dependencies, or lockfiles changed; only this handoff
is committed. No app was launched.

### Four-way all-pixel result

Same 6000×4000 synthetic input, `rich_settings()`, amount 1, `profile: None`,
level 0, CPU reference and resident GPU evaluator as `bench_24mp_cpu_gpu`.
The input/reference/settings helpers were verified identical between base
and tip. Presence off changes only texture and clarity to zero. An identical
temporary integration test on both revisions scanned all 72 million RGB
samples, checked finite CPU/GPU values and bit-exact alpha, and logged the
maximum instead of applying a parity acceptance assertion. Existing bench
assertions were never edited. These diagnostic test passes are **not** passes
of the existing absolute `< 0.01` guard: all four measurements exceed it.
Indices are zero-based; GPU/CPU values are final encoded document RGB.

| Revision | Presence | Max absolute gap | Pixel | Channel | GPU | CPU |
| --- | --- | ---: | ---: | --- | ---: | ---: |
| base | on | 6.1408234 | 5,112,798 | blue | 225.30309 | 219.16226 |
| base | off | 0.016636014 | 4,999,168 | red | 0.5281837 | 0.5115477 |
| tip | on | 0.01663196 | 4,999,168 | red | 0.5281682 | 0.51153624 |
| tip | off | 0.016636014 | 4,999,168 | red | 0.5281837 | 0.5115477 |

The original every-997th-pixel scaled metric remains below 0.002 in all four
cases: base/on 0.000946507, base/off 0.00071656704, tip/on 0.0007223487,
tip/off 0.00071656704. Tip/on exactly reproduces ENG-1f. Base/off and tip/off
have identical reported maxima, location, GPU/CPU values and target RGBA.

### Stage isolation at pixel 4,999,168 (x=1168, y=833), red

Cumulative settings isolation on tip, using full-frame renders at each step
and the original encoded profile bridge. Rows are encoded outputs with later
controls neutralized, **not internal buffer readbacks**. Start with neutral
settings (including sharpening and color-noise reduction zero), then restore
rich white balance, detail, tone, presence, dehaze, RGB curve, color, locals,
and effects in evaluator order. Tone here restores exposure/contrast/
highlights/shadows/whites/blacks while presence, dehaze and curves stay neutral.

| Cumulative enabled stages | GPU red | CPU red | Absolute red gap |
| --- | ---: | ---: | ---: |
| Neutral bridge | 0.46875 | 0.46875 | 0 |
| + White balance | 0.46674666 | 0.46674666 | 0 |
| + Detail | 0.4080246 | 0.4080245 | 1.1920929e-07 |
| + Tone (no presence/dehaze/curves) | 0.4591821 | 0.4445941 | 0.0145880282 |
| + Presence | 0.45916852 | 0.44458404 | 0.0145844817 |
| + Dehaze | 0.45916852 | 0.44458404 | 0.0145844817 |
| + RGB curve | 0.48747167 | 0.4716921 | 0.0157795846 |
| + Color | 0.47831452 | 0.462764 | 0.015550524 |
| + Local adjustments | 0.53591484 | 0.518707 | 0.0172078609 |
| + Effects (full original settings) | 0.5281682 | 0.51153624 | 0.01663196 |

**Conclusion:** the remaining tip 24 MP failure is a pre-existing tone-stage
precision gap, not a gap introduced by the ENG-1 presence conditioning. It
persists with presence disabled at exactly the same maximum and values on
base and tip (0.016636014). At the target pixel, detail leaves only about
1.19e-7 red error; enabling tone increases it to 0.014588028 before any presence
or dehaze. Presence slightly reduces that error, while subsequent curves,
color, locals and effects yield the full-chain 0.01663196. Main already
exceeds 0.01 with original settings, by 6.1408234 at a different blue sample;
that older unconditioned-presence failure is distinct from the residual tone
failure exposed here. ENG-4 remains responsible for the residual tone gap;
no threshold or rendering change is proposed or made in this lane.

### Reproduction and evidence

All runs used release mode, Apple M4 Max **Metal** (adapter recorded in every
measurement log), and the requested PATH, `CARGO_BUILD_JOBS=3`, and
`RAYON_NUM_THREADS=3`. Tip target directory was
`~/.cache/tessera-target/ENG-1-texture-clarity`. Temporary measurement source
was removed from both worktrees after execution; its exact copy and all logs
remain at `/Users/rutmehta/tessera-evidence/ENG-1/eng1g/`.

To reproduce, copy archived `eng1g_measure.rs` to `crates/filters/tests/`,
set the appropriate external target directory and requested environment, then:

```sh
ENG1G_MODE=on cargo test --locked --release -p filters --test eng1g_measure -- --nocapture
ENG1G_MODE=off cargo test --locked --release -p filters --test eng1g_measure -- --nocapture
# On tip, modes 0..8 select the cumulative stage rows; on is the final row.
ENG1G_MODE=3 cargo test --locked --release -p filters --test eng1g_measure -- --nocapture
```

All 13 measurement invocations completed successfully, with finite RGB and
exact alpha checks. No parity-bound acceptance is implied. Full Rust/Swift
suites were not rerun for this documentation-only lane. Verified clean
tracked source on both revisions before removing diagnostics, and
`git diff --check` before commit.

| Archived file | SHA-256 |
| --- | --- |
| base-build.log | `9547d6288cbdf86586325b1309a56ca551cff4a643f3bee06093f9410233c859` |
| base-off.log | `21069718d7a81a245d54febaaa9cdc60d585883d724d29b69a14871d2fa4dfd7` |
| base-on.log | `950926200f06c5aac77d66f4795b87fb94613df3cbdc542517bc87d99da0d32d` |
| eng1g_measure.rs | `1f23beac2eb08e56fd5790e4ace453173b4fdadacce8a6233198ae1f6a535a05` |
| tip-0.log | `ec05991235ff402522104d30482192e1fd4e58d501a6920cbe60811c30ba8063` |
| tip-1.log | `6b4ca975b5885bd2bbd52ff607afe25eac3eb27724f933e8ce51e11d4e4dca90` |
| tip-2.log | `853bb398e7d55f2496811d53981f01392c9b00adc6118193ca498d04b5460a94` |
| tip-3.log | `8e19bbbe2233b66bde53c5b0386898fce7c5018c9c73628a1bc303673df7644c` |
| tip-4.log | `c13f127e1c8f9dee67771872ad45294345bc58b05a5b7de0b8df583593aa0a38` |
| tip-5.log | `d9326fb37a19ba8e9a2fc5708099d0af05ce03f5c292cf8ef5f7e314437086ea` |
| tip-6.log | `bfd9956a9aca0f9f17359a973a8975e3a0900644d65353f947831199fbd41591` |
| tip-7.log | `3ea8abebbae6dee55e75623f35f6bc875bca1010d294f04c63cf9eaf2aa13d26` |
| tip-8.log | `2cc7beebe477b5e3c0aa08104c11e55d741f8d94247d1bc403f02a9deae34d27` |
| tip-off.log | `4608cad0a52fe2ec3e5b12066dd070531f9953bb0659bade38ae227e92daeeb2` |
| tip-on.log | `f4bf2c630f7b2f38fa48bd1960378ef7283662ad4dfad5a2a317bb8d41b4e629` |

## ENG-1h — approved 24 MP presence regression guards (2026-10-01)

Local-only follow-up on `e2a2a57a`, branch `wp/ENG-1-texture-clarity`, without
rebase. Machine A approved replacing the ignored bench's all-pixel `< 0.01`
guard with presence-off `<= 0.02` and presence-on `<= presence-off + 1e-4`.
The every-997th-pixel scaled `< 0.002` guard is unchanged and runs in both
configurations. Both runs check finite CPU/GPU RGB and bit-exact CPU/GPU alpha
against the input. Only texture and clarity are zeroed for presence off.
The bench comment names ENG-4, the measured 0.016636014 at pixel 4,999,168 red
(GPU 0.5281837 / CPU 0.5115477), and ENG-4's responsibility to restore the
0.01 absolute bound. No production code or app changes; no app launched.

### ENG-1g attribution retained

These are the historical ENG-1g measurements, not additional ENG-1h runs.
Base is local main `87536669ea555535ebb6622a17848f2b0fb530f3`; tip measured
in ENG-1g was `3e565432`. Indices are zero-based.

| Revision | Presence | Max absolute gap | Pixel | Channel | GPU | CPU |
| --- | --- | ---: | ---: | --- | ---: | ---: |
| base | on | 6.1408234 | 5,112,798 | blue | 225.30309 | 219.16226 |
| base | off | 0.016636014 | 4,999,168 | red | 0.5281837 | 0.5115477 |
| tip | on | 0.01663196 | 4,999,168 | red | 0.5281682 | 0.51153624 |
| tip | off | 0.016636014 | 4,999,168 | red | 0.5281837 | 0.5115477 |

At pixel 4,999,168 red, ENG-1g's cumulative stage isolation measured:

| Cumulative enabled stages | GPU red | CPU red | Absolute red gap |
| --- | ---: | ---: | ---: |
| Neutral bridge | 0.46875 | 0.46875 | 0 |
| + White balance | 0.46674666 | 0.46674666 | 0 |
| + Detail | 0.4080246 | 0.4080245 | 1.1920929e-07 |
| + Tone (no presence/dehaze/curves) | 0.4591821 | 0.4445941 | 0.0145880282 |
| + Presence | 0.45916852 | 0.44458404 | 0.0145844817 |
| + Dehaze | 0.45916852 | 0.44458404 | 0.0145844817 |
| + RGB curve | 0.48747167 | 0.4716921 | 0.0157795846 |
| + Color | 0.47831452 | 0.462764 | 0.015550524 |
| + Local adjustments | 0.53591484 | 0.518707 | 0.0172078609 |
| + Effects (full original settings) | 0.5281682 | 0.51153624 | 0.01663196 |

ENG-1 reduces main's 24 MP worst case from **6.1408** to the pre-existing
tone-stage gap. Presence off has the same maximum on base and tip; tone
introduces the residual before presence is applied. ENG-4 restores the
0.01 absolute bound. This approved guard isolates presence regression from
that outstanding tone precision work.

### Fresh Metal bench and gates

Test commit: `2d07862159d24f5851d3817161121691954f0d87`.
The ignored bench ran **once**, on Apple M4 Max **Metal**, and passed:
**1 passed, 0 failed**, 97.44 s. Both bounds, sampled guards, finite-RGB
checks, and exact-alpha checks passed.

| Presence | All-pixel max absolute gap | Pixel | Channel | GPU | CPU | CPU time | GPU time |
| --- | ---: | ---: | --- | ---: | ---: | ---: | ---: |
| off | 0.016636014 | 4,999,168 | red | 0.5281837 | 0.5115477 | 41.949610667 s | 2.972812750 s |
| on | 0.01663196 | 4,999,168 | red | 0.5281682 | 0.51153624 | 43.586716959 s | 2.166960875 s |

GPU times include pipeline creation and exclude upload/readback. Presence on
is approximately 0.000004054 below presence off, satisfying the +1e-4 guard.

All commands used the requested environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/ENG-1-texture-clarity
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
cargo test -p filters --release --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --exact --nocapture
cargo test -p filters --release --test camera_raw_gpu
cargo clippy -p filters --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Non-ignored release camera-raw GPU suite: **10 passed, 0 failed, 1 ignored**.
- Filters clippy, all targets, `-D warnings`: **exit 0**.
- Workspace format check after formatting the edited test: **exit 0**.
- `git diff --check`: **exit 0**.

Existing vendored LibRaw C/C++ warnings were emitted; the requested Rust
clippy gate passed. Changes are limited to the ignored test and this handoff.
Commits remain local; no rebase or push.

Durable logs: `/Users/rutmehta/tessera-evidence/ENG-1/eng1h/`.

| Evidence | SHA-256 |
| --- | --- |
| bench.log | `8338b79272223470a7e4251d688c179c13e61c44c5fdad912e84993de2f23b38` |
| test.log | `b8e4ba78b499e6b3cc2211565200a515ba70f54c5243386d59a81d4acccdaf5d` |
| clippy.log | `330371267abc951b5b3d52c2f796abcdd9d6e7ba1b8a66edac52614f3692eef2` |
| fmt.log | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` |
