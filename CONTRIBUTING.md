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

## Dependencies and models

`deny.toml` is the executable dependency policy: MIT, Apache-2.0, BSD-2/3, ISC, Zlib, Unicode-3.0, MPL-2.0 and CDDL-1.0 are allowed, with additional listed permissive identifiers and a narrow LGPL exception for `lcms2`. GPL/AGPL dependencies are not accepted; `jpegxl-rs`, `jpegxl-sys` and `libraw-sys` are explicitly banned. Consult [licensing](docs/13-licensing.md) for LibRaw's CDDL selection, data-pack attribution and distribution constraints. Run `cargo deny check licenses bans` after dependency changes and update policy only after review, not just to make a failing gate green.

For a model, verify the **weights'** permissive licence and upstream attribution (not only the code repository's licence), then add an exact ID/version, tensor contract, source URL and SHA-256 to [`crates/ml-runtime/models.toml`](crates/ml-runtime/models.toml). Test resolution/hash validation and representative inference/partition behavior with the owning `ml-*` crate. `ModelRegistry` downloads on explicit resolve/cache miss, verifies bytes before publishing the cache and never silently upgrades a model. Do not check weights into git or introduce an unpinned mutable download.
