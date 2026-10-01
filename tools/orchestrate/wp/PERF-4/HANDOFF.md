# PERF-4 Gaussian handoff (pending execution)

Status: test/source lane prepared; implementation and all compiler-backed evidence are on hold.

## Checkout and commits

- Base: `ebae08bb` (phase-two main snapshot).
- Branch: `codex/perf-4-gaussian`, checkout `/Volumes/betterSSD/tessera-worktrees/codex-lr-0-inventory`.
- `177b0883 test(PERF-4): add scalar parity oracle and Gaussian performance gate` — frozen old scalar/tap-order oracle, F32 tolerance coverage, and ignored paired 24MP r12 convolve benchmark with a 2x assertion.
- `a783c018 test(PERF-4): cover full apply and harden baseline oracle` — correct two-pass baseline source, finite-value guards, sigma 0 and 1x1/1xN/Nx1 coverage, input immutability and pre-cancel test, alternating convolve benchmark order, plus ignored public 24MP r12 apply benchmark with per-trial output digest.
- `39ca8556 docs(perf): record phase two lane plans` — task board and PERF-1/4/5 source-plan records.
- `7c18288b test(PERF-4): isolate benchmark outputs and parity check` — drops each timed result before timing its peer; alternates run order and checks per-implementation digest stability, then performs untimed full-output parity with finite checks.
- `6f214329 docs(PERF-4): record compiler hold and pending evidence` — initial handoff.
- Current test-only HEAD is `7c18288b`; the docs-only update containing this handoff and task board follows it.

## Evidence and hold

No Cargo tests, builds, or benchmarks have run. The RED tests have not yet been executed, so no test failure or baseline timing is claimed. `rustfmt --edition 2024 crates/filters/src/lib.rs` and `git diff --check` succeeded after the latest source change; these checks do not compile the code.

Compiler work remains held by the coordinator's shared resource gate: an external coordinator-owned `swift-test` / `xctest` run is active. Do not launch a standalone compiler or background build until the coordinator releases the gate.

## Next steps after release

1. Capture logs and exit codes under `/Volumes/betterSSD/tessera-validation/perf4/`.
2. On the test-only revision, run focused correctness plus both ignored release benchmarks: paired frozen-baseline/convolve and public `Effect::Gaussian::apply` 24MP r12. Use the coordinator-mandated target directory/job/thread/TMPDIR settings. Record explicit local host and load. Timed benchmark outputs are dropped before the next timed kernel; each implementation's digest must remain stable across its trials. The separate untimed full pixel oracle with finite-value checks and `1/65535` tolerance is authoritative for cross-implementation parity; the digests are diagnostic and are not compared across implementations. The report's original 723.20 ms reference came from a loaded M4 Max 48 GiB host, while this checkout host reported Apple M4 24 GiB, so raw times are not directly comparable.
3. Implement only the narrow `convolve` path documented in `SOURCE-PLAN.md`, preserving kernel, pass order, tap arithmetic, clamping, alpha/color, large-radius, and cancellation semantics.
4. On the candidate revision, repeat correctness and both benchmarks on the same host/load, then run filters tests, clippy, and fmt under the allowed compiler window. Compare public-apply before/after medians and output digests from these same-host runs. Do not infer whole-apply target completion from convolve-only speedup. If the 2x target is missed, report the measured gap without weakening the assertion.

## Current scope limits

No implementation has been made. No baseline/red timing, green timing, test result, clippy result, or full acceptance claim exists yet. PERF-4 still requires RED execution, implementation, GREEN evidence, and independent review.
