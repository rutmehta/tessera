# ENG-4b — presence precision and curve-axis follow-up

Base: `341f56be`, branch `wp/ENG-4-tone-precision`. Implements items 1–3 of the binding Machine A round-2 ENG-4b review. This is the follow-up to [ENG-4](../ENG-4/HANDOFF.md), not a replacement of its baseline audit.

## Item-by-item response

1. **Done — stable presence math and restored 24 MP check.** Both `presence.wgsl` (resident path) and `tone_local.wgsl` (host-image GPU path) now use the same `log_one_plus` and `exp_minus_one` evaluations as `operators.wgsl`: an atanh series for small log inputs and a Taylor series for small exponential inputs, with the same branches, constants and Horner order. This avoids cancellation at zero without relying on a compensating expression that Metal fast-math can fold away. Existing presence recombination, luminance floor, neighbourhoods, HDR overflow handling, and alpha handling are retained. `bench_24mp_cpu_gpu` again requires **presence-on ≤ presence-off + 1e-4**, in addition to the existing absolute `< .01`, sampled scaled `< .002`, finite RGB and exact alpha checks. `eng4b_dark_local_presence_matches_cpu` covers the separate local GPU path with synthetic signed near-black RGB and a `1e-6` absolute bound.
2. **Done — unified, including the inverse.** WGSL `log_one_plus` also feeds `curve_encode` in `operators.wgsl`; at the base commit, CPU curves still used `ln_1p`. CPU curves, presence and dehaze share `tone_extra.rs::encode/decode`, which now call `tone_math::log_one_plus/exp_minus_one`, matching the shader formulation. The host curve denominator uses the corresponding ordinary-log branch. `eng4b_curve_presence_axis_uses_shared_stable_math` sweeps 20,001 synthetic inputs from 2^-30 to 2^10, crosses both series boundaries, requires exact use of the shared CPU formulation, and independently bounds encoder/decoder relative error against f64 at `3e-7`. Existing CPU/GPU curve and signed/HDR tests remain unchanged.
3. **Done — stale comment fixed.** The comment above `filters/tests/camera_raw_gpu.rs::bench_24mp_cpu_gpu` now states the actual all-pixel comparison, absolute/scaled limits, alpha checks, and release-only timing requirement. It no longer describes an obsolete baseline. Verification: source review, fmt, and the named 24 MP test.

## RED first

`df1fadf2` (`test(ENG-4b):`) precedes all production changes. Observed failures on the baseline:

- `eng4b_curve_presence_axis_uses_shared_stable_math`: at input `1.5193189e-8`, CPU encode bits `859884723` versus shared stable bits `859884724`.
- `eng4b_dark_local_presence_matches_cpu`: texture 100 / clarity 0, maximum absolute RGB error `2.6524067e-6`, failing `1e-6`.
- `bench_24mp_cpu_gpu`: all 72 million RGB samples scanned per configuration; presence-off `0.00030770898`, presence-on `0.0014633238`, failing the restored relative-to-off bound on Apple M4 Max / Metal.

`5ebed48f` (`fix(ENG-4b):`) implements the fix. Both new targeted tests pass without changing their bounds.

## Final gates and golden audit

No golden or fingerprint expectation was edited by this lane. The explicit release clean removed **612 files / 998.8 MiB**.

| Crate | Suites | Passed | Failed (final) | Ignored |
| --- | ---: | ---: | ---: | ---: |
| filters | 28 | 150 | 0 | 8 |
| pipeline-cpu | 29 | 180 | 0 | 3 |
| pipeline-gpu | 38 | 154 | 0 | 12 |
| image-core | 24 | 120 | 0 | 2 |
| engine-api | 14 | 120 | 0 | 0 |
| previews | 4 | 28 | 0 | 3 |
| export | 33 | 100 | 0 | 7 |
| tessera-ffi | 53 | 581 | 0 | 30 |
| Total | 223 | 1433 | 0 | 65 |

**The aggregate first run exited 101**, with 1432 passed and one existing preview wall-clock assertion failure. The other seven crates completed with zero failures. The final previews row above comes from a complete successful release rerun with one test thread, not from hiding the initial failure. All 28 previews tests passed; the RAW preview took **2.618698 s**, below the unchanged 3 s cap. Earlier attempts took 3.893269041 s (full matrix), 4.599630459 s (serial crate), and 5.692743375 s (serial crate overlapping Clippy). An isolated check after Clippy passed at 2.247484917 s, followed by the full successful crate gate. Every attempt produced identical image statistics (mean 0.2884667189542532, standard deviation 0.2552569601127745). This test uses default tone settings and exits the additional-tone stage before the changed axis functions. Shared build load affected the timing; no test threshold, CI flag, system setting or production code was changed to obtain the pass.

Additional final gates:

- Explicit release 24 MP test: **exit 0; 1 passed**. Before/after all-pixel maxima below; all original bounds and alpha checks retained.
- Workspace Clippy, all targets, `-D warnings`: **exit 0**. Existing native LibRaw deprecation diagnostics remain; no Rust Clippy warnings.
- `cargo fmt --all -- --check` and `git diff --check`: **exit 0**.
- Synthetic capture: **3 passed before and 3 passed after**, yielding nine captures per revision. These opt-in captures are separate from the normal full gate, which checked the unchanged stored references without capture overrides.
- SDR fingerprint probe: **exit 0; 1 passed**, plus explicit equality of all nine printed fingerprints to the stored constants. The test's built-in GPU assertion is scoped to Apple M4, so its six GPU values were independently compared on this Apple M4 Max.

| 24 MP configuration | Base max RGB error | Fixed max RGB error |
| --- | ---: | ---: |
| Presence off | 0.00030770898 | 0.00021445751 |
| Presence on | 0.0014633238 | 0.00022757053 |

The fixed presence-on increase is **0.00001311302**, below the restored `1e-4` allowance. Both scans cover all 72 million RGB samples. Fixed worst pixels are 8962936 / blue (off) and 6906328 / red (on); both are synthetic fixture coordinates.

### Exhaustive golden/fingerprint attribution

**Stored golden changes: none. Stored fingerprint changes: none.** All nine stored float Develop references, all five photographic RAW PNG references (CR3, RAF, NEF, DNG, ARW), and all nine recorded SDR fingerprints remain unchanged. The parent ENG-4 CPU case-2 fingerprint `0xd1434c534f542e11` is retained; this lane does not re-pin it or any other expected value.

[golden-audit.json](golden-audit.json) records hashes and per-case measurements. Against base `341f56be`, the six active synthetic CPU Develop captures have small float rounding changes from the shared log/exponential axis used by their texture, clarity and dehaze settings. The basic tone function is unchanged by ENG-4b. Profile conversion and encoded opacity blending propagate those deltas; amount zero bypasses the affected operations. There is no quantized stored-golden change to attribute.

| Synthetic output | Amount | Changed pixels / 4403 vs base | Max encoded float delta vs base | Max delta vs stored reference |
| --- | ---: | ---: | ---: | ---: |
| sRGB | 1 | 1997 | 1.6525388e-5 | 1.9103289e-5 |
| sRGB | .35 | 1981 | 5.7816505e-6 | 6.6906214e-6 |
| sRGB | 0 | 0 | 0 | 0 |
| Display P3 | 1 | 1889 | 3.1888485e-6 | 4.8875809e-6 |
| Display P3 | .35 | 1857 | 1.1175871e-6 | 1.7136335e-6 |
| Display P3 | 0 | 0 | 0 | 0 |
| Adobe RGB | 1 | 1949 | 1.6063452e-5 | 2.7239323e-5 |
| Adobe RGB | .35 | 1923 | 5.6251884e-6 | 9.5367432e-6 |
| Adobe RGB | 0 | 0 | 0 | 0 |

Alpha is bit-exact in every before/after pair. All stored-reference deltas remain below the unchanged `1e-4` tolerance. Normal photographic golden checks passed with the fixture directory present. All three CPU, three GPU-tile, and three GPU-surface SDR fingerprints equal their stored values exactly on this machine.

All builds/tests and final gates use:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/ENG-4"
export CARGO_BUILD_JOBS=4
export RAYON_NUM_THREADS=4
```

Final gate commands:

```sh
cargo clean -p filters -p pipeline-cpu -p pipeline-gpu
cargo clean --release -p filters -p pipeline-cpu -p pipeline-gpu
cargo test --release -p filters -p pipeline-cpu -p pipeline-gpu -p image-core -p engine-api -p previews -p export -p tessera-ffi --no-fail-fast
cargo test --release -p filters --test camera_raw_gpu bench_24mp_cpu_gpu -- --ignored --exact --nocapture
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
# Complete previews retake after the timing-only failure and other gate jobs:
cargo test --release -p previews --no-fail-fast -- --test-threads=1 --nocapture
```

The explicit release clean is necessary here: the unqualified package clean removed zero files. All latency tests are run only in release. Timing values are diagnostics under shared-machine load, not a new performance claim.

## Scope and integration

Only synthetic tests and aggregate numerical evidence are added. No private pixels, private fixture paths, GUI activity, system settings, protected libraries, board edits, or Cargo.lock edits. The existing parent ENG-4 fingerprint audit remains intact. All commits have the required co-author trailer. The branch is pushed only to the explicitly authorized `origin wp/ENG-4-tone-precision` destination.

Full logs (including every failure), synthetic before/after captures, and audit scripts are retained outside the checkout in `$HOME/tessera-evidence/ENG-4b/5ebed48f/`; [evidence-sha256.txt](evidence-sha256.txt) records their hashes without private paths. From the checkout root, reproduce the aggregate float/fingerprint audit with `python3 "$HOME/tessera-evidence/ENG-4b/5ebed48f/audit.py"`; it requires byte-identical reproduction of the committed report. No lane finding remains unimplemented. The final `docs(ENG-4b):` commit contains this record; its hash is reported with the push result.

Machine A remains sole merger. Its instruction to rerun ENG-3/ENG-4 on the combined LR-2 tree before the second merge remains an integration gate for the coordinator; this lane makes commits only on the supplied ENG-4 branch.
