# M2-49 implementation and verification

**Latest retry: prescribed gate and isolated warm-drag benchmark PASS.** This retry directly executed the exact requested chained gate (process `proc_ffd125e9ac9e`, exit 0), then ran the real-RAW benchmark alone (`retry-geometry-budget.log`, exit 0). All eight cases met the unchanged p90 <12 ms and maximum <16 ms checks. Cold Auto still took 13.86 seconds. Earlier intermittent maximum-frame failures are retained below and are not claimed fixed by this passing sample. Real learned-model inference remains unverified without weights, as permitted by the fixture-skip policy. The historical sections below describe earlier attempts; the final retry section supersedes their gate disposition.

This work stays in the M2-49 worktree and user-authorized source paths. No commits or pushes. Swift application consumers are unchanged. Every Cargo invocation exports `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-49`.

## Method

Test-first slices cover composed geometry, CFA estimation, explicit model acquisition, depth caching/visualization/focus, bokeh apertures, and session/export integration. Individual logs contain observed missing-API failures or behavioral failures before implementation. Initial Cargo builds were serialized behind a pre-existing gate build in the same target; several narrowly named red logs consequently compiled later than their corresponding independently observed failure. `geometry-rustc-red.log` and `noise-rustc-red.log` preserve the direct observed initial failures. The initial direct noise compile attempt had a mismatched dependency; the preserved retry contains only the missing estimator API. No successful inference or latency is inferred from compilation.

Automatic noise estimation works on normalized linear raw samples packed into canonical RGGB. Each site is estimated independently. Nonoverlapping 8×8 same-site patches use `(a-b-c+d)/2`: this cancels constant/linear gradients and has unit noise energy for independent noise. The flattest three quarters of patches suppress texture. A nonnegative variance-versus-mean fit separates shot/read terms only when surviving means span more than 0.05; otherwise the observed variance is represented as a constant read term. This is dark-frame-free estimation, not camera calibration. Texture, signal clipping, spatial noise correlation and narrow scene brightness range limit identifiability. Explicit measured calibration remains an override. ISO is not treated as a calibration.

Depth inference uses the pinned Depth Anything model and a content/model/preprocessing-keyed cache under `previews/depth-cache`. Weights are cache-only during rendering. Model downloads are a separate explicit FFI operation with queued, byte progress, ready and failed callbacks. Digest verification precedes atomic installation. Depth is near-to-far for focus and blur, inverse depth for visualization. Subject focus uses an ml-segment subject raster and weighted depth percentiles.

## Verification evidence

- Geometry scalar regression: observed max scene-linear mismatch 1.1222355 before fix; composed distortion, vignette, Guided Upright, transform and crop now satisfy 1e-5 tolerance. Logs: `geometry-scalar-red.log`, `geometry-scalar-green.log`.
- CPU lens-plan tests: Guided and Auto/Level/Vertical/Full analysis-backed plans pass (`geometry-parity-latency-retry.log`).
- Sandbox GPU runs report no adapters. A separate host-produced `host-geometry.log` records a successful L2 interactive/resident-export parity test with maximum display-code error 0. Its earlier 512×384 synthetic sensor slider samples range from 0.79–3.39 ms; these are not a substitute for the real >=30MP budget benchmark, whose separate host result is reported below.
- Registry policy/download tests: 8 passed (`download-regression.log`).
- Depth/bokeh focused tests and narrow all-target clippy passed. Additional corruption recovery, aperture normalization and focal-range evidence is in `depth-edge-green.log`, `bokeh-edge-green.log`, `depth-bokeh-clippy.log`.
- Real depth inference cleanly skipped because pinned weights are unavailable (`depth-model-availability.log`).
- Automatic RAW noise estimation passed on `sample.dng`, `sony-arw.ARW`, `nikon-nef.NEF` and `canon-cr3.CR3`; site-specific shot/read coefficients are preserved in `raw-noise-fixtures.log`. Fuji X-Trans is not a Bayer estimator fixture.
- Final non-GPU FFI run: 44 passed, 7 hardware-specific tests filtered (`session-final-cpu.log`). This includes model callbacks/policy, geometry preservation, session-only flags, depth overlay preview exclusion, process validation and histogram freshness. Behavioral failures before fixes are preserved in `session-final-tests.log` and `histogram-freshness-observed-red.log`; the latter is green in `depth-histogram-freshness-green.log`.
- The real L2 geometry benchmark was actually invoked and failed at GPU acquisition (`geometry-real-latency.log`). Its contract uses a >=30MP NEF, IOSurface plus histogram, Guided and Auto drags, >=30 samples (default 40), p90 <12 ms and max <16 ms, with no warm pixel uploads/readbacks. That sandbox invocation collected no samples because Metal exposes no adapter; the separate host measurement is below. Reproduce on a Metal-capable host with `export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-49; cargo test -p pipeline-gpu --release --test interactive_performance bench_l2_composed_geometry_budget -- --ignored --nocapture` and optionally `PIPELINE_BENCH_NEF=/path/to/30mp.nef`.


## Implemented behavior

- Interactive resident and scalar rendering resolve the same composed inverse geometry: optical distortion, Guided or image-derived Upright, manual transform and crop. Optical gain/CA retain their existing processing stages; spatial remapping is composed at the final geometry stage. Automatic Upright decisions use L0 analysis and are reused at preview levels; warm manual transforms reuse the analysis and lens resolution. Geometry is applied after local and depth effects. Preview frames retain these settings when persisted.
- `DevelopSession.set_render_uncorrected(bool)` exposes the uncorrected image for guide placement without changing the saved recipe. `set_render_depth_visualisation(bool)` is also session-only. Depth frames are marked `is_overlay`, do not replace the RGB histogram, and cannot be persisted as the normal edited preview.
- Interactive CFA denoise installs a lazy automatic-noise adapter, preserving explicitly configured calibration as an override. Export installs the same estimator/adapter and prefers the resident CFA path for supported native recipes; depth/AI combinations use the CPU hook path. Model acquisition is separate from rendering; cached weights are required. Zero-strength/masked-out inference bypasses weights.
- `ModelDownloads.open(manifest_path, cache_path, allow_downloads)` and `request(id, version, listener)` expose ordered queued, downloading bytes/optional total, ready path, or failed reason events. Callbacks run on the worker without renderer locks. Cache hits are digest-verified even when downloads are disabled. Changing the policy requires a new downloader object.
- Depth uses displayed pre-geometry pixels and a content/model/preprocessing keyed raster cache. Proxy/viewport and full-resolution exports use their own content/dimension keys, so cross-resolution depth identity is not guaranteed. Cache hits work without weights; corrupt rasters are recovered. Failed estimation clears the provider's prior histogram. Render errors surface missing weights; export skips only the precise missing-weight error and records a warning. Corrupt weights, incompatible model provenance and inference failures remain errors.
- `Engine.depth_histogram(image_id)` and the session histogram return 256 near-to-far bins. Subject focus uses alpha-weighted fifth/ninety-fifth depth percentiles and updates the existing focal range. Visualization uses normalized inverse depth with exact black/white endpoints.
- Blur supports circle/disc, bubble, five-blade/pentagon, hexagon, octagon, ring, cat-eye and oval/anamorphic apertures. Focused pixels preserve their bits; normalized kernels preserve constant HDR values.
- Export exposes warnings through `RenderedExport::warnings()` and `BatchReport::warnings()`. Nonempty warnings are persisted beside the output as `<filename>.tessera-warnings.txt`; publication refuses overwrites and rolls back newly created warning/XMP files if image publication fails. Metadata retains the original requested recipe even when missing depth weights cause blur to be skipped.

## Consumer handoff and remaining limitations

M2-50 must explicitly enable the Swift panels and call the new bindings; application consumers are intentionally unchanged.

| M2-48 consumer gap | M2-49 disposition |
| --- | --- |
| Upright/Transform ignored by viewport and edited thumbnails | Closed in settings/render/preview code; scalar composition verified. Host L2 parity passed; all eight real-RAW p90 cases pass, but three maximum-frame checks fail. Guided placement must toggle `setRenderUncorrected`. |
| `DevelopEngineGaps.aiDenoise` | Calibration/setup requirement closed with automatic raw estimation and render/export adapters; acquisition callbacks implemented. UI must acquire valid weights before enabling inference. Missing local CFA artifacts prevent model-quality verification. |
| `DevelopEngineGaps.lensBlur` | On-demand cached depth hook implemented; missing weights surface a panel error and skip export blur with a warning. Injected/cached-depth CPU behavior verified; real model inference unverified without weights. |
| `DevelopEngineGaps.lensBlurDepth` | Histogram and session visualization APIs implemented and tested. M2-50 supplies histogram UI and toggle wiring. |
| `DevelopEngineGaps.lensBlurSubject` | Subject-focus action implemented as a computed update of the existing focal range. No persistent subject-tracking recipe flag was added. Segmentation weights are required. |
| `DevelopEngineGaps.lensBlurRefine` | Still open: no depth refine-brush API was requested or added. |
| Constrain Crop | Still unsupported by the scalar geometry operator and excluded from resident plans. M2-50 must not enable this control based on the Upright/Transform work above. |
| Guide-axis schema and persistent Boost/cat-eye controls | Unchanged. Guide orientation remains inferred; runtime aperture controls are not new recipe fields. |

Export batch Swift consumers do not yet show the new Rust warning accessor; the warning report is also available as an output sidecar. Existing AI-mask export combinations with lens warps remain explicitly rejected. Cold background RAW previews retain the existing bilinear/ML-stripped fallback in the unmodified FFI preview consumer; geometry/lens settings now reach that renderer, while ML-edited thumbnails depend on the session storing a completed matching frame. This is not a new general background ML-thumbnail renderer.

The CFA entries in the pinned catalog are local artifacts, not downloadable public URLs. Both `tools/orchestrate/wp/M3-16/artifacts/cfa-fp32.onnx` and `cfa-fp16.onnx` are absent here. Acquisition must use the original or a correctly packaged manifest (so relative local paths resolve) and populate the shared `<support>/models/cache`. The copied application manifest is for cache-only lookup; it must not be used as a relocated local-artifact acquisition catalog. Depth weights are also absent, so learned image quality and inference performance cannot be validated here. Subject focus currently initializes the existing segmenter, which requires U2Net plus SAM encoder/decoder weights even though the subject operation uses U2Net.

Metal exposes no adapter inside this sandbox. Separate host-produced logs provide small-frame GPU parity/timing evidence; the real-RAW maximum-frame check failed, and the host gate found the EDR precision regression discussed below. Resident neural export with actual weights remains unverified. No timing is inferred from scalar runs. The exact sandbox gate and independently continued checks are recorded below.


## Requested gate and environment blockers

The exact requested chained gate was invoked with the required target directory and serial Rust test threads (`requested-gate.log`). It compiled all selected targets, then stopped in image-core's library tests: 19 passed and 2 failed. One failure was the resident test backend's missing/new geometry contract. It is fixed with scalar remap/crop and wide-band storage; the unchanged parity/edit-invalidation assertions pass (`geometry-resident-model-retry.log`, 741.42 seconds in debug). The other is the unchanged ImageIO HEIC fixture test. Independent probes (`heic-probe.log`) show ImageIO returns a successful 32×24 TIFF containing only RGB(0,0,0), while ffmpeg software decoding of the same unchanged HEIC yields the expected constant RGB(180,90,40). This is an ImageIO environment failure; no decoder assertion was weakened.

Because `&&` stops after the failed test stage, the subsequent checks were invoked independently. Strict clippy for all requested packages passed again after the resident-backend fix (`final-clippy-complete.log`, after the EDR fix); `cargo fmt --check` also passed (`final-fmt.log`); strict all-target clippy for the separately modified export crate also passed (`export-clippy.log`). Export's full library run reported 21 passed, 4 existing GPU failures and 2 ignored (`export-hooks-final.log`). The non-GPU export rerun reported 20 passed; its new resident test explicitly reports an unavailable-adapter skip (`export-hooks-verified.log`). Those counts do not establish GPU execution.

Bindings were regenerated from the freshly built FFI library. The new methods/events are present in both generated Swift and C headers. Plain Swift build initially failed on its global module-cache write and nested sandbox setup; a retry uses writable target-directory caches and SwiftPM's `--disable-sandbox` while remaining inside the outer workspace restriction. Build/test outcomes are listed below.

### Swift verification

Generated `TesseraFFI.swift`, `CTesseraFFI.h` and module map from the successful release FFI build. Swift application build passed in 179.89 seconds (`swift-local-build.log`) using the pinned Sparkle 2.10.0 cache copied into this worktree, writable module/package caches under the required target directory, and SwiftPM's nested sandbox disabled. No application consumer was edited. The unmodified plain command's environment failures remain in `bindings-direct.log` and `swift-writable-cache.log` rather than being hidden. Existing Swift concurrency warnings and a cached native object's macOS 26.5-vs-15.0 deployment-target linker warning remain; this is compilation/link verification, not a macOS 15 runtime certification.

### Completed package checks

- `RUST_TEST_THREADS=1 cargo test -p pipeline-cpu -p lens -p ml-enhance -p ml-depth --release`: exit 0, 207 passed, 0 failed, 2 ignored (`cpu-model-final-tests.log`). Model tests that explicitly return early for missing weights remain subject to the availability limitation above.
- Final sandbox image-core library: 22 passed and only the independently reproduced ImageIO failure remains (`image-core-final-tests.log`). The separate host gate passes all 23 library tests, including ImageIO and the fixed resident backend, and its image-core integrations passed.
- Existing export AI-mask integration regression: 6 passed, 0 failed (`export-ai-regression.log`).

### Real-RAW geometry latency: measured, acceptance still open

`host-geometry-budget.log` records an Apple M4 and the 7378×4924 (~36.3 MP) NEF, cropped L2 output 1661×1108. Guided cold presentation took 777.89 ms. Forty warm vertical-drag samples gave median **5.34 ms**, p90 **8.35 ms** (passes the 12 ms target), maximum **27.13 ms** (fails the benchmark's 16 ms adaptive-downgrade check). Warm pixel upload/readback checks passed. The benchmark then aborted, so those numbers do not establish the remaining axes or Auto performance. The host suite was running concurrently; contention is a possibility, not an established explanation. The benchmark now collects all latency failures and asserts at the end, preserving both thresholds, to avoid hiding later cases on the next controlled host run. This gap is not declared closed.

### Host EDR regression and precision fix

The separate host requested gate (`host-final-gate.log`) passed image-core and progressed through CPU/model/GPU suites, then stopped with `GATE_EXIT=101` at `pipeline-gpu --test hdr_surface`: `edr_tiles_match_cpu_reference_and_unit_headroom_is_sdr_linear` measured relative error 0.008452031 at headroom 2.5, exceeding the unchanged 0.005 tolerance. It is not reported as an environmental skip.

A new device-independent test reproduces the same error against the full-f32 scalar reference (0.008379766, `geometry-edr-red.log`). The mapped scalar path was discarding its map before deciding whether to use the f32 optics prefix, leaving distortion-only images on f16 WB checkpoints. Preserving that decision fixes the precision loss: exact error 0 at headrooms 1, 2.5 and 16 (`geometry-edr-green.log`). `geometry_edr.rs` asserts that its fixture really activates calibrated distortion. No existing numerical assertion was relaxed. The original Metal HDR test passes in the post-fix host rerun (`host-post-review-gate.log`), as does L2 interactive composed-geometry parity. That gate later stopped at a stale resident-capability assertion, described below.

- Post-EDR-fix release image-core suite: exit 0, **79 passed, 0 failed, 2 ignored**, with only the known sandbox ImageIO test filtered (`image-core-post-edr-tests.log`). The new full-f32 HDR regression is included.

- A separate ordinary host `./build-ffi.sh && swift build` also completed successfully after regeneration (`host-swift-build.log`, Swift build 72.03 seconds). The deployment-target linker warning remains.

### Resident capability expectation update

The post-fix host gate passed the original Metal HDR regression and L2 parity, then stopped in `resident_fusion::resident_capability_matches_extended_settings_and_backend`: its pre-M2-49 assertion expected a 2-degree crop rotation to disable resident rendering. The new composed-map path intentionally supports this. Updated that expectation to require resident GPU capability and retained an explicit CPU-backend rejection. This changes a feature-capability expectation, not a numerical tolerance. The observed failure is preserved in `host-post-review-gate.log`; the sandbox post-update attempt compiled successfully but stopped at GPU initialization (no adapter), before the assertion (`resident-capability-post-review.log`). A further host rerun is recorded in `host-final-verification.log`. The full requested gate has not yet completed successfully.

### Complete real-RAW latency matrix

The subsequent host run (`host-geometry-budget-all.log`) measured all eight cases with the same 36.3 MP NEF and 40 samples per case. All p90 values pass 12 ms; three worst-frame values fail 16 ms. Warm pixel-transfer assertions passed. These results supersede the earlier incomplete matrix, without erasing its recorded spike.

| Mode / edit | p50 ms | p90 ms | max ms |
| --- | ---: | ---: | ---: |
| Guided vertical | 5.47 | 6.01 | 6.28 |
| Guided horizontal | 5.48 | 6.43 | **21.04** |
| Guided rotate | 5.49 | 5.94 | 6.80 |
| Guided scale | 5.42 | 5.77 | 6.12 |
| Auto vertical | 6.07 | 6.65 | 13.35 |
| Auto horizontal | 6.23 | 6.76 | 7.28 |
| Auto rotate | 6.06 | 6.73 | **24.13** |
| Auto scale | 6.61 | 7.44 | **28.12** |

Cold Auto measured **11887.38 ms**. Neither this cold delay nor the maximum-frame failures is declared solved. Device-side profiling and repeatable timing diagnosis require Metal access unavailable to this sandbox; host logs provide measurements but do not establish the cause of the spikes.

### Earlier host gate: intermittent export/slider contention

`host-final-verification.log` passes the corrected resident capability test, the original HDR test, L2 geometry parity, and reaches FFI integration. It exits 101 at unchanged `export_batch_does_not_starve_slider_drag`: 89 of 120 frames stayed at L2 (31 dropped to L3). Idle slider p90 was 5.3 ms; during export render p90 was 15.5 ms, max 188.1 ms; set-to-frame p90 was 19.4 ms, max 188.7 ms. Five exports completed in 7.81 seconds, with none completed during the 2.81-second drag. The suite reported 8 passed, 1 failed, 3 ignored. The responsiveness assertion remains unchanged. This is an unresolved measured performance failure, not a model-weight skip or successful gate.

Source-level contention review: ordinary exports with no neural denoise or blur retain the existing export dispatch and band scheduler. The interactive path now preserves default Auto lens optics instead of stripping them; warm exposure edits reuse lens resolution and WB/detail caches, and Off Upright does not trigger L0 analysis. No source-only evidence attributes the 188 ms spike to a specific changed stage. Next diagnostic is timestamped decode, band submit/wait and interactive submit/wait on the same host clock. Cold Auto does explicitly develop L0 for its analysis. Scheduling changes without this evidence would be speculative. Further independent host check output, if still running at handoff, is retained in `host-final-checks.log` and `host-all-tests.log`; it is not claimed green without completion.

## Final parent verification and disposition

All verification processes have completed. The parent executed the exact requested command on the host, with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-49` unchanged. The final serialized run, `host-serial-gate.log`, exited **0** through release tests, strict all-target clippy, `cargo fmt --check`, binding regeneration, and ordinary `swift build`. This includes the original Metal HDR test, L2 geometry parity, export/slider concurrency, print rendering and all seven packages' doctests. The separate final checks also exited 0 (`host-final-checks.log`). No application Swift consumers were edited and no commits were made.

Independent review found and fixed two correctness issues, then confirmed both fixes in a read-only re-review:

- L0 Upright analysis is now cached independently of whether a resident lens plan can be built. Nonresident optical combinations (tested with distortion plus defringe) reuse the same homography at L1/L2 and after manual Transform edits. The observed pre-fix L1 error was 1.063286; the regression now passes with a 1e-6 bound.
- Depth initialization now uses the registry's atomic manifest publication, preserving `TESSERA_DEPTH_MODELS`. Regression tests verify old readers retain their manifest snapshot and that the weights override is honored.

The supplemental no-fail-fast run (`host-all-tests.log`) was not green: its print assertion differed only in ICC creation-time byte 35 (56 versus 57), and five doctest targets encountered E0463 dependency lookup errors while a separate binding build reused this target directory. The later fully serialized exact gate passed unchanged, including these tests. The earlier failed logs are retained rather than replaced. No numerical tolerance, responsiveness assertion or print comparison was weakened.

The verdict at that earlier handoff was **FAIL / performance acceptance open**, not because the final prescribed gate failed, but because the separately requested real-RAW latency verification still recorded 21.04, 24.13 and 28.12 ms maxima against its 16 ms check, and cold Auto required 11887.38 ms. All eight warm p90 values were below 12 ms and warm pixel-transfer checks passed. These measurements do not establish a root cause for the intermittent spikes. Do not enable Constrain Crop or imply actual-weight quality verification from this handoff.

## Latest retry verification

The existing implementation was preserved. No source behavior or numerical/performance assertion was changed in this retry. The exact requested chained command ran directly with the required external CARGO_TARGET_DIR and exited 0 (process `proc_ffd125e9ac9e`). Release tests, all-target strict clippy, workspace format check, binding regeneration and ordinary Swift build all completed successfully. Swift reported 36.60 seconds. Existing LibRaw warnings and the macOS 26.5 versus 15.0 native-object linker warning remain.

After that process exited, the real-RAW geometry benchmark ran separately, with no overlapping build or benchmark launched by this worker. `retry-geometry-budget.log` records exit 0 and the Apple M4, 7378×4924 NEF, 1661×1108 L2 output, 40 measured frames per case, and successful warm pixel-transfer assertions.

| Mode / edit | p50 ms | p90 ms | max ms |
| --- | ---: | ---: | ---: |
| Guided vertical | 5.79 | 6.25 | 7.47 |
| Guided horizontal | 5.70 | 6.19 | 15.89 |
| Guided rotate | 5.79 | 6.43 | 15.20 |
| Guided scale | 5.77 | 6.25 | 7.93 |
| Auto vertical | 6.33 | 6.72 | 7.71 |
| Auto horizontal | 6.32 | 6.89 | 10.26 |
| Auto rotate | 6.43 | 6.88 | 8.04 |
| Auto scale | 6.52 | 6.89 | 7.55 |

Cold Guided was 562.42 ms; cold Auto was 13863.94 ms. This meets the existing warm-slider acceptance test, not a cold-start latency guarantee. Earlier intermittent spikes remain a reliability risk, and this run neither establishes their cause nor claims to fix them. The M2-50 consumer handoff and missing-weight limitations above remain applicable. No application Swift consumers were modified, no commits were made, and lib.rs retains only the authorized `mod models;` addition.
