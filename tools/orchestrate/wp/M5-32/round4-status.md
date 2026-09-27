# M5-32 round 4

## Verification

The exact requested chained gate was run to completion against the final code:

`cargo test -p compositor -p psd -p brush -p filters -p ml-filters -p engine-api -p tessera-mcp -p tessera-ffi --release && cargo clippy -p compositor -p psd -p brush -p filters -p ml-filters -p engine-api -p tessera-mcp -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace && (cd apps/mac && ./build-ffi.sh && swift build)`

Exit 0. Release suites: 804 passed, 0 failed, 30 ignored. Clippy with warnings denied, formatting, workspace check, FFI generation/build and Swift build all passed. Log: `round4-final-gate.log` (`GATE_EXIT=0`). The ignored performance tests were also explicitly executed, as described below. Existing model-dependent tests remain subject to their original weight prerequisites.

`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32` was retained for every Cargo invocation. The final gate explicitly exported `MACOSX_DEPLOYMENT_TARGET=15.0` to match build-ffi.sh and avoid deployment-target rebuild churn. Existing vendored LibRaw C++ warnings and a Swift link warning about a blake3 object built for macOS 26.5 remain nonfatal; this is not a claim of testing deployment on macOS 15.

## Five deliverables

1. Backward-compatible adjustment metadata from prior rounds remains covered by serde/native/PSD tests: Color Lookup filename/dither, Match Color neutralize, Auto shadow/highlight clip percentages. This round packs dither/neutralize into the resident payload and implements them in the shared shader. Both interpreter and waited-for specialization pass bit-exact CPU comparisons at F32/U8/U16, L0/L2, including HDR/negative inputs and alpha. Lookup spatial dither has a separate flat-field multi-tile test proving variation and no tile reset. Neutralize removes matched source mean Lab chroma before intensity/fade. As documented, numerical equivalence with proprietary Photoshop internals is not claimed.
2. Existing AlphaDisplay/spot metadata persists in native documents and PSD channel display resources, and is exposed by engine-api and FFI channel summaries. Legacy Alpha defaults remain intact. Channel regressions pass.
3. Existing PaintTarget/StrokeTarget channel destinations, layer-mask destinations, brush adapters, MCP dispatch and ordinary undo/redo pass. Contract 1.6.0 and its changelog remain in place.
4. Remove retains the tile-aligned stroke ROI plus 128px donor margin/dilation and full explicit sampling area. This round caches known target samples and small donor-run expansions, tightens conservative rejection, and skips duplicate candidates without changing random draws or reducing configured iterations or random sampling budgets. A scalar-reference test checks exact patch costs for unknown samples, image edges and transformed footprints. Existing CAF quality/ROI tests pass unchanged.
5. The shared compositor adapter now recognizes `neural/photo_restoration`, validates denoise-only `photo_enhancement` in [0,1], advertises it in the offline neural catalog, and supports explicit CPU model loading through a pinned PhotoRestoration slot. No implicit downloads, GFPGAN, or scratch reduction. The pre-existing DocumentSession dispatch is now connected end to end; missing weights leave both destructive pixels and smart-object history/stacks unchanged. Catalog, adapter and FFI regressions pass.

## Real-photo performance

Host: Apple M4. Fixture: Wikimedia Commons Fronalpstock_big.jpg, resized with image::Triangle to 6000x3000, a centered 300x300 mask, default CAF, CPU backend, dilation 2. Timer covers the complete Remove call, excluding file decode/fixture construction. The benchmark is explicit/ignored so normal tests never download or require a local photograph; set M532_PHOTO to reproduce.

- Final-code Cargo benchmark: 638.365 ms (`round4-photo-final.log`).
- Final gate's newly built benchmark, executed after the gate: 736.907 ms (`round4-photo-postgate.log`), strict <1s assertion passed.
- CPU periodic-texture fixture: 129.027 ms.
- Auto CPU fallback periodic-texture fixture: 148.143 ms, MAE 0.000000.

Timing is sensitive to this shared host's concurrent builds: additional real-photo wall-clock runs exceeded 1s (about 1.97–4.25s), retained in `round4-benchmarks.log` and `round4-benchmarks-final.log`. These are not silently excluded or relabeled passes. The <1s result is a measured baseline, not a latency guarantee under arbitrary CPU contention. Synthetic quality checks are not a claim that photographic inpainting reconstructs hidden ground truth.

Fixture source: https://upload.wikimedia.org/wikipedia/commons/3/3f/Fronalpstock_big.jpg
SHA-256: 24eb29eccdf0af691b406d1a3d22c0ef5d761cc454d8a917848c33958f6fc857
The downloaded JPEG is not retained in the change set. Re-download it to an allowed local fixture path and set M532_PHOTO before running `cargo test -p filters --release --test m532_photo_perf -- --ignored --nocapture`.

## Scope

Only allowed source/test/doc paths changed. Resident changes are restricted to program.rs payload handling and adjustments.wgsl's two new option branches; no render files or other Machine B files changed this round. The pre-existing brief edit is preserved. Generated bindings were exercised and are unchanged relative to round 3. No commits or pushes were made. No Kanban task ID was supplied in the environment (kanban_show reported task_id required), so no board lifecycle transition was possible.

RESULT: PASS
