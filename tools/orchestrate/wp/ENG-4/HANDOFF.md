# ENG-4 — identical CPU/Metal tone formulation

Branch `wp/ENG-4-tone-precision`, based on `f6b572ba` (`origin/wp/LR-2-tone-curves`, through LR-2f). Local only; no rebase, push, dependency, Cargo.lock, board, Swift, or app changes. The declined B5-32 commit `635f69d8` was not taken or used as an implementation source.

## Root cause and fix

This was a cancellation/formulation mismatch, not an unavoidable GPU precision ceiling. Two operations amplified ordinary f32 rounding on signed post-detail RGB:

1. CPU used libm `ln_1p`, whereas WGSL used `log(1+x) * x/((1+x)-1)`. At the captured small-fixture input, GPU `initial_z` was `2.7418100e-6` against CPU `2.6904836e-6`; at the captured 24 MP GPU input it was `3.5762787e-7` against CPU `3.3760438e-7` when both evaluated that same input. The observed GPU result behaves like the rounded `log(1+x)` rather than preserving the small argument. This is an observation of the compiled result, not a claim based on disassembled Metal code.
2. Both then subtracted nearby O(1) softplus results to obtain O(z) region integrals. GPU white-region integral at the 24 MP input was **-7.4505806e-9**, where the CPU returned **+2.2351742e-8**. Dividing the final reconstructed luminance by tiny Y magnified those errors. CPU itself was inaccurate on these cases; matching the old CPU's cancellation was not an adequate oracle.

The small coefficients also had different operation order: CPU `.2 * amount / 100`, GPU `.2 * (amount / 100)`. CPU now normalizes first, exactly as the GPU host does. The GPU's redundant rounded-slope neutral shortcut was removed; both use the exact non-exposure-control-zero predicate.

The two backends now evaluate the **same real-valued curve with the same f32 formulation**:

- Identical small-argument `log1p`: `2*atanh(x/(2+x))`, evaluated through degree 15 in the same Horner order for `abs(x) < .5`; `log(1+x)` otherwise.
- Identical degree-ten Horner `expm1` for `abs(x) < .5`; `exp(x)-1` otherwise.
- For `z < .5`, evaluate the region difference directly as `log1p(expm1(z)/(1+exp(k)))`. This is algebraically `softplus(z-k)-softplus(-k)`, without subtracting two nearby large values. For `z >= .5`, retain the well-conditioned original softplus difference to avoid HDR `exp(z)` overflow.
- Same exposure and slider clamps, constants, contrast expression, black/shadow/highlight/white accumulation order, luminance divide, neutral path, and nonpositive-luminance bypass.

There is no luminance floor, f64 production path, output clamp change, tolerance widening, or one-sided correction. Hardware transcendental rounding and fused arithmetic still need not be bit-identical. The now-stable gain approaches approximately `0.972887` as Y approaches zero, so upstream ulps no longer cause a percent-scale gain jump.

## Test-first and captured inputs

`a2877dd7` (`test(ENG-4):`) restores the small-chain scaled `< .002` guard and 24 MP all-pixel absolute `< .01` guard, removes the ENG-1e/1h pins, and enables/extends the independent f64 tone test with the actual captured inputs. RED was observed before production edits:

- Small full chain: `.00458771` scaled at amount 1; presence off `.0043850243`.
- 24 MP: presence off `.016636014`, on `.01663196`.
- Original four-sample f64 diagnostic: CPU `1.8405054e-5`, GPU `.005117128`.
- Expanded diagnostic (including captured inputs): CPU `.0001746222921`, GPU `.01144788111`; required bound remains `1e-5`.

Actual pre-tone buffers were captured in the rich chains on both sides. CPU capture was directly after whole-image detail; GPU capture retained/read the resident post-detail tile before tone. Source instrumentation was temporary and is archived as `input-capture.patch`; none remains in production.

| Replay input | Fixture / backend | RGB input f32 bits (decimal u32) |
| --- | --- | --- |
| 0 | 259×263, pixel 10703, CPU | `[3152284792, 3149941360, 1036554289]` |
| 1 | 259×263, pixel 10703, GPU | `[3152284814, 3149941348, 1036554287]` |
| 2 | 6000×4000, pixel 4999168, CPU | `[1033291222, 3170405425, 1017776900]` |
| 3 | 6000×4000, pixel 4999168, GPU | `[1033291228, 3170405430, 1017776934]` |

[Before](tone-before.csv) and [after](tone-after.csv) give every major tone intermediate for **both backends on all four inputs**: exposed RGB, Y, initial log coordinate, contrast coordinate, each region and accumulated output, expm1, gain, and output RGB, including exact f32 bits. GPU replay splices writes into the actual scalar shader source; CPU replay uses the baseline operations or new production helpers. Ordinary uninstrumented GPU tone outputs were also logged: trace writes can shift the last 1–2 ulps on the small input, not the measured cancellation or its conclusion. Full-chain acceptance numbers below come from uninstrumented production execution.

| Actual-side tone gain | Before | After |
| --- | ---: | ---: |
| Small CPU, input 0 | 0.9726920128 | 0.9728869796 |
| Small GPU, input 1 | 0.9924890995 | 9.728868603706e-1 |
| 24 MP CPU, input 2 | 0.9748135209 | 0.9728868604 |
| 24 MP GPU, input 3 | 1.0445276499 | 0.9728869796 |

After-fix f64 diagnostic maximum: **CPU `4.8446242e-9`, GPU `2.2656842e-8`**; both pass the unchanged `1e-5` bound. The test is no longer ignored.

## Full-chain measurements

All values are final encoded document RGB on Apple M4 Max Metal. Each scan checks finite CPU/GPU RGB and preserved alpha; the 24 MP scan covers all 72 million RGB samples. The original sampled scaled bench guard remains intact in both presence configurations.

| Scene / presence | Before max absolute | After max absolute | After worst location |
| --- | ---: | ---: | --- |
| Small, on, amount .35 | .0016056895 | .000012889504 | all-pixel scan |
| Small, on, amount 1 | .00458771 | .00003684312 | all-pixel scan |
| Small, off, amount 1 | .0043850243 | .00004144013 | all-pixel scan |
| 24 MP, off | .016636014 | .00030770898 | pixel 4489548, blue |
| 24 MP, on | .01663196 | .0014633238 | pixel 8962936, blue |

| Requested pixel | Presence | Before GPU / CPU | Before absolute gap | After GPU / CPU | After absolute gap |
| --- | --- | --- | ---: | --- | ---: |
| 10703 blue (84,41) | off | .4546297 / .45024467 | .0043850243 | .45028782 / .45028812 | 2.9802322e-7 |
| 10703 blue (84,41) | on | .4807913 / .4762036 | .00458771 | .47624603 / .47624892 | 2.8908253e-6 |
| 4999168 red (1168,833) | off | .5281837 / .5115477 | .016636014 | .51107913 / .5110788 | 3.5762787e-7 |
| 4999168 red (1168,833) | on | .5281682 / .51153624 | .01663196 | .5110652 / .5110671 | 1.9073486e-6 |

The post-fix ignored 24 MP bench ran once on Metal and passed. Timings are diagnostic under shared-machine load, not a performance claim. See `bench.log` for the exact timings.

## Every Develop golden and approved SDR fingerprint change

All nine stored float Develop references and all five photographic PNGs are unchanged. The scalar formulation changes some synthetic float output bits, but every existing stored-reference and independent Develop-parity test passes its unchanged `1e-4` tolerance. [goldens.json](goldens.json) records complete-capture SHA-256, changed-pixel counts, and max encoded channel delta. Alpha is bit-exact; zero amount bypasses Develop.

| Stored float Develop reference | Amount | Changed pixels / 4403 (render vs old reference) | Max encoded delta | Stored file changed |
| --- | ---: | ---: | ---: | --- |
| sRGB | 1 | 1933 | 1.9103288651e-5 | no |
| sRGB | .35 | 1915 | 6.6906213760e-6 | no |
| sRGB | 0 | 0 | 0 | no |
| Display P3 | 1 | 1923 | 4.8875808716e-6 | no |
| Display P3 | .35 | 1898 | 1.7136335373e-6 | no |
| Display P3 | 0 | 0 | 0 | no |
| Adobe RGB | 1 | 2026 | 2.7239322662e-5 | no |
| Adobe RGB | .35 | 2002 | 9.5367431641e-6 | no |
| Adobe RGB | 0 | 0 | 0 | no |

These small float differences are expected wherever active positive-luminance tone uses the common f32 evaluation rather than libm/cancelling differences; presence and dehaze can propagate them. No float reference was re-recorded or accepted under a changed tolerance.

Photographic RAWs in the repository's `fixtures/raw` symlink were present throughout and explicitly rendered before and after; no fixture skips were used:

| RAW / stored PNG | Pixels | Changed before/after | Changed vs PNG | Max encoded delta |
| --- | ---: | ---: | ---: | ---: |
| Canon CR3 | 250000 | 0 | 0 | 0/255 |
| Fuji RAF | 249696 | 0 | 0 | 0/255 |
| Nikon NEF | 568568 | 0 | 0 | 0/255 |
| DNG / sample | 282968 | 0 | 0 | 0/255 |
| Sony ARW | 252150 | 0 | 0 | 0/255 |

Their neutral non-exposure tone controls take the unchanged bypass, so the support of this fix is empty. The explicit before/after photographic test and exact PNG comparisons all passed.

The first full clean gate correctly failed `hdr_surface::sdr_output_is_bit_identical_to_pre_edr_fingerprints`, exposing one additional stored Develop fingerprint. It was not blindly re-recorded. [sdr_audit.py](sdr_audit.py) inserts the complete old tone function from pinned `f6b572ba` into a temporary test and replays the **actual renderer with the original reused default cache**. It first requires exact equality to **all three old CPU fingerprints**, then compares every pixel with the new production tone. It asserts bit-identical upstream tone inputs, records tone and prequantization output, and independently replays the unchanged ordered-dither quantizer for every channel of every pixel. All other production operators are shared and unchanged.

For each output pixel p, the verified iff predicate is:

1. p has nonneutral tone controls and strictly positive exposed Rec.2020 luminance;
2. the old and new tone output bits differ **at p itself**;
3. at least one channel crosses a quantization threshold: `Q_before(p,c) != Q_after(p,c)`, where `Q(p,c) = clamp(round(255 * encoded(p,c) + dither(p)), 0, 255)`. The audit computes Q independently from the captured prequantization float values and requires exact equality with the actual output bytes.

For these SDR recipes there is no active presence, dehaze, geometry, or local operation after tone: creative color, output sigmoid, gamut conversion/compression, OETF, cache quantization, and ordered dither are pointwise. Thus the support radius is **zero**. All output changes must be at an active changed tone pixel and cross an existing quantization threshold. The audit rejects any changed pixel outside this set and verifies both directions of the predicate over all 189900 pixels across the three cases.

| SDR golden | Pixels | Changed output pixels | Max encoded delta | Outside predicate | Stored change |
| --- | ---: | ---: | ---: | ---: | --- |
| CPU default | 63300 | 0 | 0/255 | 0 | none |
| CPU exposure/highlights | 63300 | 0 | 0/255 | 0 | none |
| CPU shadows/vibrance | 63300 | 2 | 1/255 | 0 | one fingerprint |
| GPU tiles, cases 0/1/2 | 63300 each | fingerprints unchanged | unchanged | n/a | none |
| GPU RGBA8 surfaces, cases 0/1/2 | 63300 each | fingerprints unchanged | unchanged | n/a | none |

The current M4 Max's six GPU fingerprints equal the historical recorded values exactly. Their existing adapter-specific assertion remains scoped to Apple M4; this lane did not broaden or change it.

Both changed samples have active positive luminance. Pixel 32405 has Y≈`.09193938`, z≈`.41262233` (the stable region-difference branch); pixel 19660 has Y≈`.47697487`, z≈`1.29468892` (the shared regular log/exponential path). The latter becomes a small red decrease after pointwise vibrance and perceptual gamut compression, even though its tone RGB increases slightly.

The only accepted fingerprint is CPU case 2, `0x39f4bd02fec83fd1 -> 0xd1434c534f542e11`:

| Pixel | Channel | Byte before -> after | Dithered value before -> after |
| --- | --- | --- | --- |
| 32405, (5,108) | red | 102 -> 103 | 102.49998 -> 102.50004 |
| 19660, (160,65) | red | 152 -> 151 | 151.5 -> 151.49974 |

[sdr-report.json](sdr-report.json) contains each changed pixel's input, tone output, encoded output, dither and pre-round values, the zero-outside-support result, and capture hashes. No unexplained golden changes were accepted. The test was renamed `sdr_output_matches_approved_fingerprints` to accurately describe the updated reference.

## Gates

Every Cargo invocation used:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/ENG-4"
export CARGO_BUILD_JOBS=4
export RAYON_NUM_THREADS=4
```

Normal gates have no capture or fixture override variables. Final source is free of temporary input/trace hooks.

```sh
cargo clean --release -p pipeline-cpu -p pipeline-gpu -p filters
cargo test -p pipeline-cpu -p pipeline-gpu -p filters --release --no-fail-fast
cargo test --release -p filters --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --exact --nocapture
cargo test --release -p tessera-ffi
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

- Final release clean: exit 0; **619 files / 1.3 GiB** removed.
- Three-crate release gate: **exit 0; 95 suites; 482 passed, 0 failed, 23 ignored, 0 filtered**. The first full run had 481 passes and the single SDR fingerprint failure; acceptance and the complete clean rerun are recorded, not hidden.
- Photographic RAW goldens: all five explicitly passed exact PNG comparisons before and after, and passed again in the normal full release gate. The unchanged level-3 `1e-4` test and all three white-balance fixture tests also pass.
- Ignored 24 MP bench: **exit 0; 1 passed**, run once after the formulation fix on Apple M4 Max Metal. Both presence variants pass the restored all-pixel absolute `.01` and existing sampled scaled `.002` bounds. The later clean/rerun only accepted the audited CPU SDR fingerprint; tone code and bench code did not change after this successful bench.
- FFI release: **exit 0; 53 suites; 581 passed, 0 failed, 30 ignored, 0 filtered**.
- Workspace Clippy, all targets, warnings denied: **exit 0**.
- Fmt and `git diff --check`: **exit 0**.
- SDR attribution: **exit 0; 1 passed**, independently rerun with bit-exact upstream-input checks; regenerated report byte-identical to the committed report.

Existing LibRaw native compiler deprecation diagnostics are not Rust Clippy warnings. No build/test retries were used to conceal the SDR reference failure; it was resolved through the explicit per-pixel audit above.

Reproduce the SDR attribution independently, without changing product source or expectations:

```sh
python3 tools/orchestrate/wp/ENG-4/sdr_audit.py --output /tmp/tessera-eng4-sdr-audit
```

The original nine float capture and five RAW capture tests used the existing opt-in `ENG1_CAPTURE` mechanism; normal stored-reference tests ran separately without it. The whole-image level-3 fixture retains its **1e-4** bound.

## Local commits and evidence

- Test-first: `a2877dd7`.
- Fix: 77c9196ec04742090d6db1e8415e6c5dbb315097.
- The subsequent `docs(ENG-4):` commit contains this handoff and final validation record; its hash is the final branch tip reported to Machine A.

All commits end in `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` and include the repository DCO sign-off. Detailed logs, trace replay source, the temporary input-capture patch, all float/RAW captures, and SDR byte captures are archived outside the checkout at `/Users/rutmehta/tessera-evidence/ENG-4/`. Compact intermediate/golden reports live alongside this handoff; the [final evidence manifest](evidence-sha256.txt) records SHA-256. No temporary test remains in the test discovery tree.
