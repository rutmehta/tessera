# PERF-4 Gaussian result

## Revisions and result

- Repository: `rutmehta/tessera`, branch `codex/perf-4-gaussian`.
- Test-only baseline: `67e257dbd9c69fb35a87d4b02513c48b21fc6862`.
- Final source/test revision: `04e481cd4014309df5e3ca31d4693534a56b5fd6`.
- The product/test tree did not change after final checks. Later commits only update this result, `HANDOFF.md`, the task board, and the copied independent review. Not merged.
- Final direct 24MP r12 convolve paired trials (ms): baseline `[2087.443, 1936.047, 1938.054]`, candidate `[867.070, 954.037, 861.682]`. Medians are `1938.054 ms` and `867.070 ms`, `2.2352x`. The unchanged ≥2x test assertion passes. Each timed output is released before timing its pair. Both per-implementation digests stayed `14666caab0baf1c8`; the separate full-pixel finite check passed at maximum absolute tolerance `1/65535`.
- Public `Effect::Gaussian::apply` trials (ms): baseline `[2955.731, 2180.016, 2105.707]`, final `[1323.467, 1086.382, 1055.713]`. Medians are `2180.016 ms` and `1086.382 ms`, observed ratio `2.0067x` (2180.016 / 1086.382 = 2.006675...). Digests match at `245b9465587756d3`. This is just over 2x and is load-qualified; baseline and final load averages differ, so do not treat it as a robust/general whole-apply guarantee. The public benchmark includes Raster read, filtering, amount blend, and Raster write.
- Independent review is copied verbatim at [`INDEPENDENT-VERIFICATION.md`](INDEPENDENT-VERIFICATION.md). It verifies the evidence sidecars and reports the full suite as 150 passed, 10 ignored, 0 failed over 28 harnesses.

## Host, environment, and run window

Host: Apple M4, 24 GiB, Darwin 25.6. Required Cargo environment on build/test/benchmark commands:

```text
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/codex-lr
CARGO_BUILD_JOBS=2
RAYON_NUM_THREADS=2
TMPDIR=/Volumes/betterSSD/tmp/.
```

Ignored performance benchmarks additionally set `PERF4_HOST='Apple M4 24 GiB, Darwin 25.6'` and `PERF4_LOAD` to the contemporaneous `uptime` load averages and the fresh process-scan result. The preserved baseline runs were around 2026-10-01 01:37 EDT, with load averages `2.79 / 4.08 / 4.52`. Final direct convolve ended 01:53:08 EDT at `3.73 / 3.14 / 3.60`; final public apply ended 01:53:28 EDT at `3.43 / 3.12 / 3.57`. Final suite, Clippy, and fmt completed by 01:55:15 EDT. The later external batch-16 pipeline began around 01:56 EDT and did not overlap these records. Filesystem mtimes are an approximate run-window record, not a process-start audit.

## Commands and final evidence

The following commands were run on final revision `04e481cd`, with the environment above. Logs are outside the repository so they remain raw and individually hashable.

| Purpose | Command | Exit | Raw log SHA-256 |
| --- | --- | ---: | --- |
| Focused Gaussian parity and pre-cancel | `cargo test --locked --release -p filters --lib gaussian_ -- --nocapture` | 0 | `6080a4a348b8e9f127c35c668059f28f9095332460653dec8df189ba600d0826` |
| Full filters Release suite | `cargo test --locked --release -p filters` | 0 | `0e84c24d215f2dcdec9140b3aaa5b607f29f320b8e1f0dcf016a1b4387420765` |
| Paired direct convolve benchmark | `cargo test --locked --release -p filters --lib benchmark_gaussian_r12_24mp_against_frozen_baseline -- --ignored --nocapture` | 0 | `eff3ac9483bf2cf6ae9d66e9be30b63a2131ee1a77fc91b120cfec061abc67e0` |
| Public apply benchmark | `cargo test --locked --release -p filters --lib benchmark_gaussian_apply_r12_24mp -- --ignored --nocapture` | 0 | `b004f2ccc38cbb2f52f281dae68f7ce60c604e533cf4ff79aeaf66373a962681` |
| Strict Clippy | `cargo clippy --locked --release -p filters --all-targets -- -D warnings` | 0 | `fe74478cc7e71b4c206badc9c002a94835d0d9ad33083ef7006096b9eb0da683` |
| Formatting | `cargo fmt --all -- --check` | 0 | `e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855` (empty log) |

Corresponding log, `.exit`, and `.sha256` files are named `final-04e481cd-*` in `/Volumes/betterSSD/tessera-validation/perf4/`.

## Baseline evidence and failed candidates

At test-only baseline `67e257db`, focused Gaussian parity passed (1 test). The preserved direct benchmark completed its full-pixel parity check and correctly failed the ≥2x assertion at `0.966x`: baseline trials `[1943.099, 1889.317, 1890.862] ms`; existing `convolve` trials `[1947.952, 1956.541, 1956.603] ms`. Both digests stayed `14666caab0baf1c8`. The public apply baseline passed with trials `[2955.731, 2180.016, 2105.707] ms`; median `2180.016 ms` and digest `245b9465587756d3`.

The first direct baseline attempt omitted `PERF4_HOST` / `PERF4_LOAD`; its raw log was overwritten during the metadata rerun and is not preserved. Transcript-observed values were `1.040x`, but this run has no raw log and is excluded from qualification. The preserved explicit-metadata baseline run above is authoritative.

Intermediate product candidates were retained and measured before the final approach: `14ce7fbf` paired at `1.241x`, `3d57298c` at `1.346x`, and initial fixed-strip candidate `299d1f97` at `2.208x`. Final reruns at `04e481cd` follow after lint fixes and demonstrate `2.235x`. The first strict Clippy attempt on `14ce7fbf` failed on constant `cfg!` assertions. After converting the release guard to `if cfg!(debug_assertions) { panic!(...) }`, Clippy on `b19dc3a9` identified three `needless_range_loop` diagnostics in the horizontal strip traversal. Iterator-based destination updates and safe empty-range intersections fixed them; final strict Clippy on `04e481cd` passes. Both failure logs are preserved.

Earlier attempt logs, including expected performance-gate failures and the Clippy diagnostics, remain under `/Volumes/betterSSD/tessera-validation/perf4/` with their own sidecars where available. The initial no-metadata direct-run log loss is the only unpreserved raw attempt.

## Scope and remaining caveat

The implementation is confined to `convolve` in `crates/filters/src/lib.rs`: dedicated zeroed pass outputs replace cloned source buffers; horizontal and vertical work runs in 256-pixel strips with ascending tap order per output pixel; horizontal borders clamp exactly as before; per-row cancellation checks, pass order, kernel generation, alpha/color behavior, and the large-radius path are unchanged. Regular parity cases include 1x1, 1xN, Nx1, undersupported kernels, HDR/negative samples, fractional alpha, and 257/513-pixel strip tails.

The 24MP direct fixture clears its 2x requirement. Whole apply only barely clears 2x in the observed, load-shifted comparison. `cpu::run` still clones the source before Gaussian dispatch even though that clone appears unused; removing that copy is a possible future narrow optimization, not part of this patch, and requires its own source review and before/after parity/performance evidence.
