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
