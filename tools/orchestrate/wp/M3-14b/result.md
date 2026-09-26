# M3-14b implementation and verification

## Implemented

- Full-resolution SDR output reduction in the resident renderer. The WGSL kernel counts 256-bin RGB/display-luma and linear-luma histograms plus any-channel clipping unions per 16K-pixel band. Four workgroup-local shards limit contention. CPU merging uses u64 counts. Per-channel clipping counts are the RGB histogram endpoints.
- Mean linear-sRGB luminance is derived from exact marginal counts, avoiding GPU floating-point summation drift.
- Whole-output memoization allows one dispatch and one statistics readback on a cached output. Frames above the operators' 2^24 sample limit still develop per tile, then gather with integer indexing for the reduction and small crop readbacks. Console uses a bounded 1.5 GiB GPU memo LRU.
- Agent perception and every applied step use native metrics and refresh the histogram. VLM JPEGs remain previews. Skin CIE76 uses native-resolution face crops. Noise uses a central native patch. Quantiles/contrast are explicitly documented 1/256-bin estimates.
- Console's default display histogram uses the reduction. Unsupported/RGB/CPU-only cases measure full-resolution output. Nondefault-bin clipping counts use integers rather than f32 accumulators, fixing saturation above 2^24 clipped pixels.
- Every metric's resolution and fallbacks are documented in the agent/MCP READMEs. Engine-api is unchanged.

## Verification

Ran the exact requested chain with CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-14b:

    cargo test -p agent -p tessera-mcp -p image-core -p pipeline-gpu --release && cargo clippy -p agent -p tessera-mcp -p image-core -p pipeline-gpu --all-targets -- -D warnings && cargo fmt --check

- Release tests: 178 passed, 0 failed, 11 ignored.
- Clippy with -D warnings: passed.
- Workspace fmt: FAILED on pre-existing module ordering in apps/tessera-cli/src/main.rs:10. The file is byte-identical to HEAD and outside this work package's allowed paths. It was not modified.
- Scoped fmt (`cargo fmt -p agent -p tessera-mcp -p image-core -p pipeline-gpu --check`): passed.
- `git diff --check`: passed. No modifications outside the allowlist.
- Explicitly ran the ignored five-fixture benchmark: passed. All five have exact histogram bins/clipping counts versus CPU counting of the same full-resolution output, independent CPU per-pixel mean error below 1e-4, and pixel-identical native face crops.
- Regression tests cover warm single-dispatch reuse, exposure invalidation, union versus per-channel counts, partial bands, buffer reuse, sparse clipping lost by resizing, native skin delta, all eight EXIF crop transforms, CPU fallback, and nondefault histogram clipping above the f32 exact-integer limit.

## Timings (release, observed run)

Five-sample median cached reduction / amortized complete fake-planner step:

| Fixture | Cached reduction | Complete step |
| --- | ---: | ---: |
| CR3 | 3.891417 ms | 91.299541 ms |
| ARW | 4.202916 ms | 57.766430 ms |
| NEF, 36,329,272 pixels | 8.812916 ms | 116.486986 ms |
| RAF | 5.993250 ms | 48.660347 ms |
| DNG | 4.693166 ms | 59.443666 ms |

The <=20 ms NEF target is met for the cached reduction, not the entire agent step (which includes preview rendering, edited-stage development, crop measurement, sidecars and perception).

The independent scalar RAW renderer comparison retains M3-14's original 2-percentage-point gate. CR3 and RAF have separate full-render mean differences of 0.002665 and 0.005219 respectively. Those are not reduction errors and this task does not claim bit-identical GPU/scalar RAW rendering. Exact reduction parity is measured on identical full-resolution output pixels.

Full evidence: `verification.log`, `five-fixture-bench.log`.

RESULT: FAIL required workspace cargo fmt --check fails on unchanged, out-of-scope apps/tessera-cli/src/main.rs module ordering.
