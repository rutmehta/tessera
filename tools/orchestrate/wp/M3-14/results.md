# M3-14 implementation and verification

Status: incomplete, not ready to mark PASS. All edits are within the allowed paths. No commit was made. `engine-api` is unchanged. Cargo used `/Users/rutmehta/.cache/tessera-target/M3-14` throughout.

## Latest verification (supersedes prior verification claims below)

This run preserved the existing implementation and made no production changes. The missing RGB entry point was confirmed in `image-core/src/source.rs` and the renderer entry points. Adding it requires permission to edit `crates/image-core/**`, outside this work package's allowed paths. Renderer pyramid averaging precedes Tone/Output, so clipping counts are not preserved. Neither tests nor acceptance bounds were weakened.

- Ran the exact required command chain. Release tests exited 101 in `crates/tessera-ffi/tests/develop.rs:1179`, `export_batch_does_not_starve_slider_drag`: the drag changed from L2 to coarser levels. An isolated rerun failed the same assertion. Evidence: `verification-current.log`, `develop-current.log`. No claim about baseline behavior is made.
- Ran clippy and fmt independently because the chain short-circuited. Both passed, as did the standalone tessera-mcp release tests and `git diff --check`. Evidence: `clippy-current.log`, `fmt-current.log`, `mcp-current.log`. Agent non-ignored tests passed in the combined run.
- Ran the ignored benchmark over all five fixtures. It failed DNG shadow clipping parity again: preview 0.028406, full 0.118098, absolute error 0.089692. Skin delta-E parity remains unverified. Evidence: `bench-current.log`.
- Latest NEF observations: CPU decode/full render 6.538 s, cold preview 649 ms, cold perception 732 ms, warm FakePlanner three-step edit 144 ms, amortized step 48 ms. These are measurements, not full acceptance: the baseline is a single CPU decode/render, not a historical FakePlanner loop.

Blocked acceptance: RGB-through-Renderer requires an out-of-scope API extension; clipping parity needs a rendering/statistics decision; the required FFI regression gate currently fails. A successful clippy run does not make this package PASS.

RESULT: FAIL metric parity and RGB Renderer requirements remain unmet; required release-test chain fails the FFI export/slider regression.

## Retry verification and additional fix

- The exact required release-test / clippy / fmt chain passed on this retry, including the previously timing-out FFI fallback test. It passed again after the change below. Evidence: `verification-retry.log`. No timeout, CI flag, or test assertion was relaxed. `git diff --check` also passed.
- Found and fixed a separate repeated-decode path in `src/mutations.rs`: crop/style validation and create/adjust-mask coverage now use the Console's cached preview renderer instead of opening `Source` and running a separate CPU render. Candidate validation retains the recipe's process version.
- Regression coverage warms the Console decode, corrupts the temporary source file, then executes create-mask, adjust-mask, crop, and style operations. It failed with a JPEG decode error before the fix and passed afterward. Evidence: `cache-red.log`, `cache-green.log`.
- Re-ran the ignored five-fixture benchmark. It still fails the unchanged 0.02 clipping-parity bound for DNG: preview 0.028406 vs full 0.118098, absolute error 0.089692. Evidence: `bench-retry.log`. All five fixtures ran. The new mutation-path fix does not change this exposure-only benchmark path.
- Retry NEF timings: full CPU decode/render 13.684 s, cold GPU preview 1.186 s, cold perception 0.668 s, warm FakePlanner three-step edit 0.199 s (0.066 s amortized per step). These are observed timings, not a claim that every cold run meets the target.
- Renderer entry points take `RawImage` (CFA), not an RGB source (`crates/image-core/src/render.rs:455-473`, `source.rs:14-18`). Meeting the literal RGB-through-Renderer requirement needs an RGB entry point in image-core, which is outside the allowed write paths. No fake CFA conversion or duplicate renderer was introduced to conceal that gap.
- The renderer explicitly box-averages the white-balanced image before preview Tone/Output (`crates/image-core/src/render.rs:5-14`). Exact clipping fractions are not preserved by that averaging. A renderer/statistics design decision is still required rather than relaxing the metric bound. Skin delta-E fixture parity remains unverified.

Current result: FAIL on acceptance criteria, despite the required ordinary test/lint/format chain passing. The measurements and failure notes below are from the previous attempt, retained as historical evidence.

## Implemented

- Console-owned decoded source cache and persistent Renderer/backend, shared by RAW previews, comparisons, and scene-linear/display histograms.
- RAW goes through image-core Renderer with GpuStageOp when initialization succeeds, CpuStageOp otherwise. Recipe process version is selected per render. Native resident GPU caches persist across steps.
- Preview pyramid level has source long edge <=1024. The agent's existing 512px planner image is resized from that preview, not a full-resolution render.
- RGB sources are decoded once and box-downsampled in linear light before CPU development. image-core Renderer currently accepts RawImage/CFA only, so RGB does NOT use Renderer or GPU stage memoization. This is a remaining scope gap, not a GPU claim.
- Agent style-feature extraction shares Console's decoded source instead of decoding RAW again.
- Explicit Console::render_final provides opt-in full-resolution pixels without introducing a full-resolution render into the agent loop.
- Metrics documentation specifies preview resolution, clipping fractions, linear luminance, and normalized face-box scaling.
- Unit test counts Demosaic/CameraProfile/WhiteBalance invocations across three exposure changes and verifies unchanged upstream counts with advancing Tone/Output counts on the CPU fallback. Another proves decoded source reuse across display, linear, and final outputs. GPU invocation counts are not separately instrumented.

## M4 measurements

Ignored benchmark: `cargo test -p agent --release --test preview_bench -- --ignored --nocapture`.
Five real fixtures copied to temporary directories to avoid changing source fixtures/sidecars. FakePlanner executes exactly one three-step exposure plan.

| Fixture | Old CPU decode + full render | Cold GPU preview | Cold agent perception | Warm FakePlanner three-step edit | Amortized step |
|---|---:|---:|---:|---:|---:|
| Canon CR3 | 13.021 s | 373 ms | 390 ms | 217 ms | 72 ms |
| Sony ARW | 2.156 s | 150 ms | 184 ms | 158 ms | 53 ms |
| Nikon NEF | 5.264 s | 766 ms | 778 ms | 271 ms | 90 ms |
| Fuji RAF | 15.013 s | 187 ms | 175 ms | 120 ms | 40 ms |
| DNG | 2.393 s | 146 ms | 191 ms | 122 ms | 41 ms |

These are recorded observations, not hard performance assertions. CPU baseline measures one decode and render, not a complete historical FakePlanner run. Warm timings include repeated perception in Agent::edit. See bench.log for raw output. Earlier timings under load varied substantially.

## Unmet acceptance criteria

The five-fixture metric test fails. DNG shadow clipping is 0.028406 at preview resolution vs 0.118098 full-resolution, an absolute difference of 0.089692 (8.9692 percentage points). The test keeps the 0.02 bound and was not relaxed to conceal this failure. Averaging before nonlinear development/clipping is not equivalent to counting clipped full-resolution pixels. A policy/renderer change is needed to meet this acceptance criterion without full-resolution metric rendering. The test interprets the bound as absolute 0.02 for [0,1] metrics; a relative 2% requirement would be stricter. Skin delta-E fixture parity is not yet covered.

RGB uses a cached CPU preview fallback but not GPU/Renderer stage memoization. Decoded source snapshots currently live for the Console lifetime and are not bounded by an LRU or refreshed on external source replacement; large batch lifetime management needs further work.

## Verification

- Required `cargo test -p tessera-mcp -p agent -p tessera-ffi --release && cargo clippy -p tessera-mcp -p agent -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check` was run.
- Initial linking failed with disk full. Only this work package's external Cargo target artifacts were cleaned. A clean rebuild then linked successfully.
- Release test command fails in unchanged `crates/tessera-ffi/tests/fallback.rs:56`, `missing_jpeg_returns_pending_then_callback_and_cached_bytes`, on its 3-second timeout. Repeated twice after successful linking. No claim is made that the baseline passes or fails this test.
- Agent unit and integration tests all passed in the release run. FFI agent scripted review/redo/accept/revert and batch tests passed.
- All 11 tessera-mcp tests passed when running the release test binaries built by the combined command (the combined command stops in FFI before reaching them).
- `cargo clippy -p tessera-mcp -p agent -p tessera-ffi --all-targets -- -D warnings` independently passed.
- `cargo fmt --check` and `git diff --check` passed.

RESULT: FAIL DNG clipping parity exceeds the 2% bound; RGB Renderer/stage memoization and skin parity remain incomplete. The required release-test/clippy/fmt chain now passes.
