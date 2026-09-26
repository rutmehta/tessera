# Contributing to Tessera

Tessera is Apache-2.0 licensed. Contributions use the Developer Certificate of Origin (DCO), not a contributor license agreement. Sign off **every commit** with your real name and email using `git commit -s`; this appends `Signed-off-by: Name <email>` and certifies that you have the right to submit your contribution under the project's licence. Read the [DCO](https://developercertificate.org/) before signing. A signature (`-S`) is different from the DCO sign-off (`-s`).

## Branches and pull requests

Work on a topic branch (work packages use `wp/<id>`), keep changes within the agreed scope, and open a PR against `main`. Explain the behavior and boundaries, link the relevant issue/work package, list the tests actually run, and include screenshots or acceptance evidence for Mac UI changes. Do not commit downloaded raw fixtures, model weights, build products, credentials or `target/`. Contract/schema changes need explicit review and versioning; see [engine contracts](crates/engine-api/CONTRACTS.md). Design/spec documents are goals; compare claims against [current status](docs/STATUS.md) and tests.

## Crate map

- `engine-api`: tile, recipe, stage, tool and document contracts; `image-core`, `jobs`, `raw-decode`/`libraw-ffi`, `pipeline-cpu`, `pipeline-gpu`, `gpu-core`, `pipeline-adobe`: decode, schedule, develop and present tiles.
- `recipe`, `sidecar`, `index`, `previews`, `library`, `cull`, `import-lrcat`, `tether`: edits, XMP, catalog, previews, decisions, import and capture.
- `lens`, `color-mgmt`, `mask-ai`, `merge`, `export`: geometry, profiles, AI masks, merges and output; procedural masks and local edits are also in the image pipeline.
- `ml-runtime` and `ml-*`, `style-profile`, `agent`: pinned models, inference and editing assistance.
- `compositor`, `psd`, `filters`, `brush`, `selection`, `vector`: layered documents and interchange (text layers are not yet a completed UI workflow).
- `tessera-ffi`, `tessera-mcp`, `apps/tessera-cli`, `apps/mac`: Swift bridge, stdio tools, CLI and Mac app.

Read [the architecture map](docs/ARCHITECTURE.md) and the nearest crate README/design doc before changing a boundary.

## Testing and review

Set `CARGO_TARGET_DIR` outside the checkout on macOS (especially when its path contains a colon), then run `bash ci.sh` from the root. It checks `cargo fmt --check`, workspace Clippy with warnings denied, release workspace tests and `cargo deny check licenses bans`. For focused changes, use `cargo test -p <crate>` and the relevant app tests after `bash apps/mac/build-ffi.sh`; run `swift test` from `apps/mac/`. Fetch optional CC0 RAW fixtures with `bash fixtures/fetch.sh`. Tests needing absent fixtures should announce a skip rather than claim fixture coverage.

Goldens must be reproducible and reviewed, not regenerated to silence a regression. The CPU reference is the operator oracle; keep its committed golden crops and input provenance intact. A deliberate rendering change needs a native process revision and a reviewed golden update (the pipeline CPU [operator guide](crates/pipeline-cpu/OPERATORS.md) documents the explicit regeneration command). From [execution plan §1.3](docs/11-execution-plan.md): CPU/GPU are **not bit-identical**. Gate per-operator max absolute error at ≤ 1e-4 linear, a full multi-stage chain at ≤ 2e-3 linear and ≤ 1 code value in 8-bit display output, and golden colour at ΔE2000 ≤ 0.5 where that suite applies. Specialized tests may document tighter or explicitly different bounds; do not silently weaken them. Cached f16 buffers are distinct from f32 in-flight math, and newer resident paths use f32 where f16 exceeded tolerance.

Keep wall-clock performance/benchmark assertions out of ordinary CI gates where they are environment-sensitive; run the ignored timing benchmarks explicitly on comparable hardware and report conditions and measured values. Some existing release tests do retain timing thresholds, so do not mistake `--ignored` for a blanket statement that CI never measures time. Mac interface work must follow [DESIGN.md](apps/mac/DESIGN.md); run `swift test --filter ThemeLintTests` from `apps/mac/` (plus affected tests). The lint rejects raw view colour, spacing, radius and font literals in favor of `Theme` tokens.

## Performance benchmarks

Run `cargo run --locked --release -p tessera-bench -- --suite cpu` from the
checkout (Python 3.10+ is required). Keep `CARGO_TARGET_DIR` outside the checkout
and unset `DYLD_FALLBACK_LIBRARY_PATH` as in `ci.sh`. The built `tessera-bench`
binary is a thin launcher for `tools/bench/runner.py`; keep its source checkout
available. Alternatively, `make -f tools/bench/Makefile bench` runs the same
command, with extra options in `BENCH_ARGS`. The root Makefile is not modified.

The CPU suite needs neither RAW downloads nor a GPU. It measures index search
over 100k rows, recipe + XMP disk round trips, a 45 MP preview pyramid,
pipeline-cpu scale-8 (L3) output, one CPU Web JPEG, and a 20-layer CPU composite.
It uses versioned synthetic fixtures, one warmup and the median of five runs,
with four Rayon threads. Setup is excluded. See [fixture definitions and
limitations](tools/bench/README.md) before interpreting these as product targets.

Results (schema: `tools/bench/results.schema.json`) go to
`bench-results/<host>/<UTC-date-and-time>.json`, with a Markdown table and raw
logs alongside them. This directory is ignored. The auto host ID includes the
machine label, OS kernel major, CPU and thread count. Set `BENCH_HOST` to a
stable machine label (CI does), or `--host` to an explicit full ID. Do not reuse
an ID across different hardware, OS or benchmark configurations.

Record a reviewed starting point with `--record-baseline`. The default file is
`tools/bench/bench-baseline-<host>-<suite>.json`; commit it after inspecting the measurements.
`--baseline path.json` selects an explicit baseline. Never overwrite a baseline
just to hide a regression. `BENCH_GATE=1` fails on regressions (exit 1), missing
baselines, incompatible measurement sets, skips or invalid data (exit 2).
Without it, regressions/missing baselines are reported but don't fail; execution
and validation errors still fail. Recording with the gate enabled is rejected.
Tolerances are fractions: 0.15 for medians/elapsed time and 0.20 for p90 by
default. A `bench/metric` key overrides a metric-wide tolerance. All current
metrics are lower-is-better milliseconds. `--input saved.json` replays validation
and comparison without benchmarking; it must still match the selected suite/host.

For local Metal coverage, fetch `fixtures/fetch.sh`'s fixtures, then run
`cargo run --locked --release -p tessera-bench -- --suite gpu`. This invokes
the existing ignored Develop, export and resident-compositor benchmarks in
isolated processes, checks that GPU export did not fall back to CPU, and rejects
missing measurements. GPU data has a separate baseline from CPU data.

The nightly/manual `bench.yml` workflow runs CPU benchmarks, uploads JSON/logs
even on gate failure, and posts the comparison in the Actions job summary.
On first use, manually dispatch with `record_baseline=true`, download the actual
host's candidate baseline, review it, and commit that file. Nightly runs fail
closed until this is done; the local M4 baseline is not a fabricated GitHub
baseline. GitHub-hosted `macos-15` machines are not fixed hardware. For docs/08's
fixed-hardware gate, set repository variables `TESSERA_BENCH_RUNNER` to a
dedicated macOS runner label and `TESSERA_BENCH_HOST` to its stable name, then
record there. Rebaseline explicitly when the runner image/hardware changes.
The measurement step has a 10-minute budget (builds are separate).

Runner contract tests: `cargo test -p tessera-bench --release`. Full CPU smoke:
`RAYON_NUM_THREADS=4 cargo test -p tessera-bench --release cpu_smoke -- --ignored`.

## Dependencies and models

`deny.toml` is the executable dependency policy: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0 and CDDL-1.0 are allowed, with additional listed permissive identifiers and a narrow LGPL exception for `lcms2`. GPL/AGPL dependencies are not accepted; `jpegxl-rs`, `jpegxl-sys` and `libraw-sys` are explicitly banned. Consult [licensing](docs/13-licensing.md) for LibRaw's CDDL selection, data-pack attribution and distribution constraints. Run `cargo deny check licenses bans` after dependency changes and update policy only after review, not just to make a failing gate green.

For a model, verify the **weights'** permissive licence and upstream attribution (not only the code repository's licence), then add an exact ID/version, tensor contract, source URL and SHA-256 to [`crates/ml-runtime/models.toml`](crates/ml-runtime/models.toml). Test resolution/hash validation and representative inference/partition behavior with the owning `ml-*` crate. `ModelRegistry` downloads on explicit resolve/cache miss, verifies bytes before publishing the cache and never silently upgrades a model. Do not check weights into git or introduce an unpinned mutable download.
