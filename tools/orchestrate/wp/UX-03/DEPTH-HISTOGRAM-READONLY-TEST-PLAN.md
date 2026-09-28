# Read-only saved-recipe depth histogram: test-first checkpoint

Integration: exact final sourcea31d2518 accepted here after focused3 and adjacent7 tests, fmt and strict Clippy; evidence6be9d5d7 and coordinator verification are preserved alongside this plan. Earlier checkpoint statuses below are historical. No writer lease is enabled.

Status: tests-only checkpoint following main `3dbd4a47`, committed as `77eb68d0`; primary intended RED observed. Adobe99 follow-up is frozen at `c2737914` and also reaches the intended second-writer RED after passing process-specific validation with a populated cache. No production histogram refactor is included in these checkpoints.

## Behavioral contract

`Engine::depth_histogram` must compute from the persisted recipe as observed by the call, without opening a `DevelopSession` or starting its `develop-save` worker. An already-open editor's unsubmitted settings must not affect the result. The call must not publish recipe, XMP, or index changes. Existing content-addressed depth cache hits remain usable, while an uncached request with no locally available depth model remains an error and performs no download.

The test seam is a `cfg(test)` counter owned by each `Engine`. It increments only after a Develop save worker starts successfully. This gives the behavior test an observable writer-construction boundary without a process-global counter or cross-test race. It is not an implementation allowance for production telemetry.

## Test coverage added

- `engine_histogram_uses_saved_pixels_without_starting_a_writer_or_publishing` opens one visible editor on a tiny indexed RGB JPEG whose disk recipe has a legacy Raw source tag. It pre-seeds deterministic depth cache rasters for both the saved input and materially changed live input, confirms those rendered inputs and expected histograms differ, then calls the Engine API. It requires the saved-input histogram, no extra writer construction, unchanged recipe/XMP bytes and unchanged index change head. It restores the visible editor's original exposure before closing because close may persist live state.
- `engine_histogram_keeps_the_cached_model_miss_error_without_downloading` makes a cold-cache request with no model installed and requires the stable missing-model error. The depth estimator's cache-miss policy disallows downloads.
- Follow-up `engine_histogram_rejects_unsupported_saved_process_with_a_populated_cache` seeds a valid native-input cache entry, writes Adobe process revision 99 as the saved recipe, then requires the Engine API to report the process-version error and leave writer count unchanged. This prevents a cache hit from hiding dropped process validation.

The synthetic cached depth rasters avoid inference and model downloads. The Raw-tagged RGB fixture is compatibility coverage, not an independent numeric proof of source-kind normalization: depth input is built from `RawImage` and settings. Preserve normalization by source review while factoring the shared setup. Existing Develop initialization also installs renderer denoisers and mask hooks; an implementation must preserve those behaviorally relevant renderer inputs while removing its save worker.

## Intended implementation factoring after RED is captured

Extract the shared recipe snapshot under the existing destination gate-read → catalog lock → ID-to-path revalidation sequence. Keep the same catalog-resolved path behavior; do not add filesystem canonicalization. The editor must continue capturing its decoded recipe and `OwnerBaseline` in that one critical section. Keep `RawImage::open` and render setup outside the lock. For the histogram, construct only image/render/depth dependencies—no `Shared`, editable `State`, owner/session lifecycle, listener, save state, or worker. Use `session_renderable(saved.settings, true, false)` and saved process version, then share the renderer-snapshot and pre-geometry depth-input helpers with Develop. Preserve its settings sanitization, explicit CFA → automatic CFA → RGB denoiser fallback, renderer mask hooks, depth provider attachment/cache rules, and in-memory RGB normalization. The read-only context must not reuse a visible editor's live denoiser adapter. Avoid a new exported API and do not acquire a writer lease in this slice.

The extracted path must preserve gate/catalog read ordering and revalidate the image ID-to-path mapping. The saved process version must reach the renderer, and the same mask cache hooks, CFA/RGB denoisers, and source-kind normalization used by visible Develop must remain available. A later lease implementation must additionally prove the histogram works while a lease for that same destination is held without acquiring another lease.

## Observed test gate and environment

At frozen HEAD `77eb68d0aa32fa43d414ebb6522c3b6a34142307`, the primary test compiled and failed at the intended assertion: the returned histogram matched the saved input, but Engine histogram increased the per-Engine started-writer count from 1 to 2. Direct exit was 101. The separate cached-model-miss control passed (direct exit 0), preserving the stable missing-model error. Raw logs, commands, source hashes, direct exits, and cache-relocation records are under `tools/orchestrate/wp/UX-03/evidence/depth-histogram-readonly-2026-09-28/`.

The run used Rust 1.98.1, macOS 26.6.2 arm64, two Cargo jobs, and the worktree target cache. Existing Develop backend startup printed CPU/GPU L2 calibration timings; this was an automatic side effect of the tiny native test harness, not a performance or GPU acceptance claim. The inactive cache is preserved on BetterSSD and `target` is a symlink; future commands must explicitly set `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated`.

No lease activation, FFI/generated binding change, or Swift/UI work is included. The exact Adobe99 checkpoint was source-reviewed, then run on the external target; preserve both REDs before the implementation work.
