# M2-34 results

## Retry verification (2026-09-26 05:53 UTC)

Moved the two measured baseline files under `tools/bench/` to resolve the
previous attempt's root-level path violations. Updated default lookup, baseline
tests, artifact upload and documentation. Baseline values and tolerances are
unchanged. The baseline test now fails if the baseline directory is empty.

Re-executed the exact required test/Clippy/fmt/actionlint chain: **exit 0**.
Runner contracts: **10 Python tests passed**. Explicit four-thread CPU smoke:
**1 passed in 30.65 seconds**. `git diff --check` passed and a full untracked-file
scope audit found no paths outside the allowlist. External CARGO_TARGET_DIR
remained `/Volumes/betterSSD/tessera-cache/target/M2-34`.

Also exercised `BENCH_HOST=local-m4 BENCH_GATE=1 make -f tools/bench/Makefile bench`.
All six measurements completed, but the collector correctly exited **1**
(`make` exits **2**) because five exceeded the existing 15% baseline threshold:

| Bench | Fresh median ms | Baseline median ms | Status |
|---|---:|---:|---|
| index-search | 9.084 | 4.056 | REGRESSION |
| sidecar-roundtrip | 9.659 | 7.959 | REGRESSION |
| preview-pyramid | 2624.443 | 1009.287 | REGRESSION |
| pipeline-cpu-l3 | 577.530 | 372.562 | REGRESSION |
| export-web | 3036.926 | 2544.067 | REGRESSION |
| compositor-20-layer | 7.534 | 7.442 | ok |

Raw results: `bench-results/local-m4-Darwin-25-Apple-M4-threads4/2026-09-26T05-53-54.660865Z.json`.
This is a functioning gate, not a passing performance comparison. The cause of
the slowdown was not established. No baseline was rerecorded to hide it.
The measurements and GPU verification below are retained from the first attempt;
GPU measurements were not repeated in this retry. No commit or push was made.

## First-attempt measurements

Measured 2026-09-26 UTC on Apple M4, Darwin 25, arm64. Four Rayon threads,
serial benchmark processes, release builds. This is a local development machine,
not a dedicated fixed-hardware CI host. No rendering crates were modified.

## CPU smoke and baseline

One warmup plus five timed operations per bench; median milliseconds.
The first run was recorded, then a fresh run was gated against it.
Fresh CPU suite: **27.115 seconds wall time**, gate exit **0**.
Build time excluded. Initial cold release build of the worker dependencies:
**2m 10s** (the initial test then intentionally failed before implementation).

| Bench | Recorded median ms | Fresh median ms |
|---|---:|---:|
| index-search | 4.056 | 3.845 |
| sidecar-roundtrip | 7.959 | 9.046 |
| preview-pyramid | 1009.287 | 967.542 |
| pipeline-cpu-l3 | 372.562 | 357.358 |
| export-web | 2544.067 | 2563.698 |
| compositor-20-layer | 7.442 | 7.184 |

The sidecar fsync timing moved +13.7% on the fresh run, close to the 15%
default tolerance. This is why runner load and storage must be controlled;
tolerances were not increased to force a pass.

## GPU local run

Full collector run completed with exit **0**, including actual Metal Develop,
GPU export (used_gpu=true), and resident compositor. Not merely parser tests.

| Bench | Metric | ms |
|---|---|---:|
| develop-slider | median_ms | 1.100 |
| develop-slider | p90_ms | 1.300 |
| export-web | elapsed_ms | 475.000 |
| resident-l2 | median_ms | 13.910 |
| resident-dab | median_ms | 2.610 |
| resident-l0 | median_ms | 197.500 |

Develop stayed at L2 on Sony ARW. Export uses full-resolution development then
Web resizing, not the M2-21c opt-in pyramid path with known precision failures.
Resident fixture is the existing 100-layer 20 MP document. GPU export is one
cold decode/render/encode sample; the resident metrics are existing medians.

## Verification

- Required command executed directly, exit **0**:
  `cargo test -p tessera-bench --release && cargo clippy -p tessera-bench --all-targets -- -D warnings && cargo fmt --check && actionlint .github/workflows/bench.yml`.
- Cargo test: runner_contracts passed; it runs **10 Python unittest tests**.
  cpu_smoke is intentionally ignored in ordinary correctness runs.
- Explicit `cargo test -p tessera-bench --release cpu_smoke -- --ignored --nocapture`:
  **1 passed**, 35.32 seconds (this first smoke used the ambient thread count).
- Schema tests cover required/extra fields, invalid values, host traversal,
  units/backends, duplicates and committed baseline validity.
- Comparison tests cover threshold equality, regressions, improvements, per-bench
  tolerances, order independence, missing/mismatched measurements and invalid tolerances.
- CLI integration exercises recording, gate rejection while recording, missing
  baseline, fresh comparison, synthetic +16% regression (exit 1 gated, exit 0
  ungated), incomplete suite failure and unchanged baseline bytes after failures.
- Adapter tests reject GPU fallback, skipped/missing output and incomplete resident results.
- Child process failures and timeout termination are tested.
- Existing LibRaw C++ build warnings remain; Rust Clippy passed with warnings denied.
- All Cargo commands retained the supplied external CARGO_TARGET_DIR.

## Files and operational limits

- CPU baseline: `tools/bench/bench-baseline-local-m4-Darwin-25-Apple-M4-threads4-cpu.json`.
- GPU baseline: `tools/bench/bench-baseline-local-m4-Darwin-25-Apple-M4-threads4-gpu.json`.
- Fresh CPU JSON: `bench-results/local-m4-Darwin-25-Apple-M4-threads4/2026-09-26T05-50-02.578531Z.json` (ignored).
- Raw CPU/GPU subprocess logs and Markdown tables remain under bench-results.
- Baseline files are deliverables for version control; no commit or push was made.
- Cargo.lock also resolves the already-present vector workspace member and its
  dependencies, absent from the starting lockfile. No existing package versions
  were upgraded. This keeps the new workflow's --locked builds usable.
- Workflow validated locally with actionlint, not dispatched to GitHub.
- GitHub/dedicated-host baseline must be measured via manual record_baseline dispatch,
  reviewed, and committed before nightly gates can pass. No cross-host baseline
  or invented hosted-runner measurements are supplied. Missing baseline fails closed.
- GitHub-hosted macos-15 is not fixed hardware. Repository variables support a
  dedicated macOS runner; runner provisioning is outside this package.
- The CPU suite uses synthetic RGB inputs, not 45 MP RAW decode. See tools/bench/README.md
  for exact timed boundaries and which docs/08 targets are not covered.
