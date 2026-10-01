# PERF-4 Gaussian handoff (pending execution)

Status: test/source lane prepared; implementation and all compiler-backed evidence are on hold.

## Checkout and commits

- Base: `ebae08bb` (phase-two main snapshot).
- Branch: `codex/perf-4-gaussian`, checkout `/Volumes/betterSSD/tessera-worktrees/codex-lr-0-inventory`.
- `177b0883 test(PERF-4): add scalar parity oracle and Gaussian performance gate` — frozen old scalar/tap-order oracle, F32 tolerance coverage, and ignored paired 24MP r12 convolve benchmark with a 2x assertion.
- `a783c018 test(PERF-4): cover full apply and harden baseline oracle` — correct two-pass baseline source, finite-value guards, sigma 0 and 1x1/1xN/Nx1 coverage, input immutability and cancellation tests, alternating convolve benchmark order, plus ignored public 24MP r12 apply benchmark with per-trial output digest.
- `39ca8556 docs(perf): record phase two lane plans` — task board and PERF-1/4/5 source-plan records.
- Current HEAD: `39ca8556`; working tree was clean when recorded.

## Evidence and hold

No Cargo tests, builds, or benchmarks have run. The RED tests have not yet been executed, so no test failure or baseline timing is claimed. `rustfmt --edition 2024 crates/filters/src/lib.rs` and `git diff --check` succeeded; these checks do not compile the code.

Compiler work remains held by the coordinator's shared resource gate: the external `tessera-ffi` release build ended, but a coordinator-owned `swift-build` / `swift-frontend` process is running, followed by strict checks. Do not launch a standalone compiler or background build until the coordinator releases the gate.

## Next steps after release

1. Capture logs and exit codes under `/Volumes/betterSSD/tessera-validation/perf4/`.
2. Run the focused correctness test and the pre-optimization ignored release convolve benchmark on this test-only HEAD, using the coordinator-mandated target directory/job/thread/TMPDIR settings. Record explicit local host and load; the report's original 723.20 ms reference came from a loaded M4 Max 48 GiB host, while this checkout host reported Apple M4 24 GiB, so raw times are not directly comparable.
3. Run the ignored public `Effect::Gaussian::apply` 24MP r12 benchmark on the same host/load after source optimization. It measures raster read, filter work, amount blend, and raster write. Compare old/new medians and output digests from the same test revision; do not infer whole-apply target completion from convolve-only speedup.
4. Implement only the narrow `convolve` path documented in `SOURCE-PLAN.md`, preserving kernel, pass order, tap arithmetic, clamping, alpha/color, large-radius, and cancellation semantics. Then run correctness, both benchmarks, filters tests, clippy, and fmt under the allowed compiler window. If the 2x target is missed, report the measured gap without weakening the assertion.

## Current scope limits

No implementation has been made. No baseline/red timing, green timing, test result, clippy result, or full acceptance claim exists yet. PERF-4 still requires RED execution, implementation, GREEN evidence, and independent review.
