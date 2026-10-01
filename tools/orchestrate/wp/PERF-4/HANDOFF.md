# PERF-4 Gaussian handoff

Status: implementation and requested Rust gates are complete on `04e481cd`; Astra's source review found no blockers on the tiled candidate, and coordinator merge review remains pending. Direct CPU convolve measured 2.235x for the specified synthetic 24MP r12 fixture. Public apply measured 2.0067x from load-shifted before/after medians; treat this as a narrow observed result, not a robust/general whole-apply guarantee. Not merged.

## Checkout and commits

- Base: `ebae08bb` (phase-two main snapshot).
- Branch: `codex/perf-4-gaussian`, checkout `/Volumes/betterSSD/tessera-worktrees/codex-lr-0-inventory`.
- `177b0883 test(PERF-4): add scalar parity oracle and Gaussian performance gate` — frozen old scalar/tap-order oracle, F32 tolerance coverage, and ignored paired 24MP r12 convolve benchmark with a 2x assertion.
- `a783c018 test(PERF-4): cover full apply and harden baseline oracle` — correct two-pass baseline source, finite-value guards, sigma 0 and 1x1/1xN/Nx1 coverage, input immutability and pre-cancel test, alternating convolve benchmark order, plus ignored public 24MP r12 apply benchmark with per-trial output digest.
- `39ca8556 docs(perf): record phase two lane plans` — task board and PERF-1/4/5 source-plan records.
- `7c18288b test(PERF-4): isolate benchmark outputs and parity check` — drops each timed result before timing its peer; alternates run order and checks per-implementation digest stability, then performs untimed full-output parity with finite checks.
- `6f214329 docs(PERF-4): record compiler hold and pending evidence` — initial handoff.
- `67e257db test(PERF-4): keep benchmark digests diagnostic` — uses per-implementation digest stability while keeping `1/65535` full pixel comparison authoritative.
- `b704b62f docs(PERF-4): update held test handoff` — reports compiler-hold state at that checkpoint.
- `e4e896ab perf(PERF-4): improve Gaussian convolution locality` — removes source-buffer clones, separates clamped horizontal borders from interior, and makes vertical tap reads contiguous.
- `14ce7fbf docs(PERF-4): clarify vertical loop order` — corrects loop-order comment.
- `3d57298c perf(PERF-4): stream horizontal Gaussian taps` — loops taps over contiguous x ranges; also fixes release-only benchmark guard lint.
- `299d1f97 perf(PERF-4): tile Gaussian passes in x strips` — processes each pass in fixed 256-pixel strips.
- `b19dc3a9 test(PERF-4): cover Gaussian strip tails` — adds ordinary CI parity cases at widths 257 and 513, height 5/7, sigma 12.
- `684ff733 perf(PERF-4): satisfy iterator lint in tiled loops` and `04e481cd fix(PERF-4): clamp empty strip intersections` — use iterator-based pixel updates and clamp strip intersections so empty edge ranges cannot form reversed slices.
- Current source/test revision: `04e481cd4014309df5e3ca31d4693534a56b5fd6`. The remaining working-tree changes are evidence/coordination documents; no product/test file is modified after the gates.

## Evidence and hold

Final focused Gaussian parity/cancellation tests passed (3 passed, 2 ignored performance tests) and the full Release filters suite passed on `04e481cd`. The paired direct convolve benchmark passed its unchanged ≥2x assertion at 2.235x: baseline median 1,938.054 ms, candidate median 867.070 ms. Both full-buffer digest sequences were stable at `14666caab0baf1c8`; the untimed per-pixel finite check and `1/65535` tolerance passed. Public `Effect::Gaussian::apply` passed with median 1,086.382 ms and stable digest `245b9465587756d3`, versus test-only baseline median 2,180.016 ms (observed ratio 2.0067x). Baseline load averages were 2.79/4.08/4.52, final load 3.43/3.12/3.57. The public whole-apply result therefore has negligible margin and load mismatch; do not generalize it as a robust 2x guarantee. It includes raster read, filtering, amount blend, and raster write.

Final gates on source/test revision `04e481cd4014309df5e3ca31d4693534a56b5fd6`: focused Gaussian parity/cancellation passed; full `cargo test --locked --release -p filters` passed; `cargo clippy --locked --release -p filters --all-targets -- -D warnings` passed; `cargo fmt --all -- --check` passed. Exact commands, environment, trial data, exit codes, and SHA-256 values are in `RESULT.md`. Raw logs and sidecars are under `/Volumes/betterSSD/tessera-validation/perf4/`.

The initial direct baseline benchmark omitted `PERF4_HOST` / `PERF4_LOAD`; its raw log was overwritten during the metadata rerun and is not preserved. Its transcript-observed 1.040x ratio is trace-only and excluded from qualification. The preserved provenance-complete baseline run is the only one used in the before/after comparison. Strict Clippy on `14ce7fbf` failed on constant `cfg!` assertions; Clippy on `b19dc3a9` failed on three `needless_range_loop` diagnostics. Both logs are retained, and final Clippy passed.

## Remaining review and follow-up

1. Astra completed source review with no blockers on the tiled candidate. Coordinator merge review remains required before merge. The branch is not merged.
2. The `cpu::run` Gaussian dispatch currently clones a source buffer before returning to `large::gaussian`; moving only the Gaussian branch ahead of that clone may remove an unused 24MP copy. This is a future narrow opportunity, not part of this candidate; any change needs fresh parity and full-apply measurement.
3. The public apply result is close to 2x and load-qualified; if a strict controlled whole-apply gate is required, rerun only under a coordinator-approved matched-load protocol. Do not weaken either existing acceptance threshold.

## Current scope limits

The exact direct convolve ≥2x target and all Rust correctness/quality checks passed. The whole-apply 2.0067x observation has very little margin and was collected at different system loads before and after. No merge or general performance guarantee is claimed.
