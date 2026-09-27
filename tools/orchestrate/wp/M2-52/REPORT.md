# M2-52: production AI Denoise

## Selection and recipe stability

`ModelRegistry::preferred_ai_denoise` chooses digest-verified CFA fp32, then fp16, only when cached or a local research artifact exists. A present local file is verified and imported without network access. Missing research artifacts are normal, not errors. Otherwise it returns the pinned MIT DRUNet colour model, whose HTTPS URL pins a Hugging Face revision and SHA-256. Corrupt/unreadable artifacts still fail closed rather than hiding integrity failures.

The current macOS UI sends the CFA identifier when enabling AI Denoise. `DevelopSession::set_settings` treats that as automatic selection ONLY on an Off -> Neural transition and replaces the live model ref before rendering/history/save. Download requests for those legacy CFA IDs share the selection policy. Unknown versions are rejected rather than normalized. Explicit DRUNet refs are not changed.

Existing neural edits, amount/tone changes, recipe reload, undo/redo and export never reselect based on availability. A DRUNet edit stays DRUNet if research weights appear later. An existing CFA edit with missing weights fails honestly instead of silently changing look. To explicitly adopt the available model, toggle AI Denoise off and on and commit the edit. Selection records the model that will execute; if acquisition/inference fails, the recipe retains that requested concrete model, not a false claim of successful inference.

Compatibility rule: the post-demosaic renderer now uses automatic sigma rather than the earlier fixed sigma25 adapter, including previously saved RGB DRUNet edits. This intentional adapter upgrade changes its memo revision to `linear-srgb-v2-auto-m249-rms50/camera-residual-v1`. It does not depend on weight availability. The low-level fixed-sigma Denoiser APIs remain unchanged for existing callers. CFA/Bayer inference is unchanged.

## Render path

CFA runs at its existing pre-demosaic stage, including the Metal resident handoff. DRUNet uses the existing full-sensor post-demosaic barrier before camera profile/white balance/tone/geometry. This is a CPU-buffer/hybrid render path, not a new fully Metal-resident DRUNet graph. Resident capability rejects RGB neural denoise and falls back before inference. Export uses the same post adapter through the existing hooks, while resident export remains CFA-only.

The colour barrier converts camera RGB to linear sRGB, preserves negative/HDR residuals, and bounds the model input. DRUNet encodes sRGB, conditions on one frame-wide sigma, restores display RGB, decodes to linear sRGB, blends Amount/mask in linear light, restores scene residuals and returns to camera primaries. Inference is tiled with the existing stride-aligned halo contract and memoized at the Demosaic tail.

Sigma reuses M2-49's flat-patch same-plane shot/read estimator on the demosaiced linear sRGB channels. At each channel mean mu, propagate variance by the sRGB transfer derivative, then take RGB RMS and clamp to 0..50/255. This is a first-order approximation, not calibrated propagation through demosaic correlations. Inputs below 8x8 return an explicit estimation error. Zero amount/mask bypasses inference.

## Verification

Weights were fetched once into ignored `crates/ml-enhance/.cache`, using the manifest's pinned URL and verified SHA-256. Set `TESSERA_ENHANCE_MODEL_CACHE` to that absolute cache directory to run the real-weight tests; absent weights print SKIP without network activity.

Added coverage: CFA cache/local presence and absence selection, clean-install ModelDownloads fallback, concrete recipe model-ref serialization/replay, automatic sigma/blend behavior, real DRUNet inference and post adapter, synthetic Bayer raw noise reduction and interactive/export parity.

The real raw regression (64x64 flat noisy Bayer fixture, central 48x48 measurement) produced variance 0.0013645547290071726 -> 0.000988772034901282. Interactive/export RMS L2 was 0 (tolerance 0.002). Output was finite and non-black. The real automatic DRUNet synthetic RGB tests reported PSNR gains of 14.470836 dB and 15.735278 dB at two noise levels. These are fixture results, not a general quality claim.

## Final gate results

Executed the exact requested chained command. The full release suite completed with 511 passed, 0 failed, 30 ignored across 124 result groups (including doctests). The first chain then stopped at a new-test clippy lint (`chunks_exact_to_as_chunks`). Changed that test to `as_chunks`, reran its five release tests successfully, and reran the full requested clippy command successfully. No production code changed after the full release suite.

`cargo fmt --check`, `apps/mac/build-ffi.sh`, and `swift build` all completed with exit 0. FFI generation produced the arm64 archive and bindings. All Cargo builds retained `/Volumes/betterSSD/tessera-cache/target/M2-52`, outside the worktree. No commits were made.

Swift linking also warned that the cached `blake3_neon.o` object was built for macOS 26.5 while linking for 15.0. The build succeeded, but this run does not certify runtime compatibility on macOS 15 hardware.

Logs: `gate.log` records the original suite and initial lint, `clippy.log` the successful complete lint rerun, and `mac-build.log` the successful FFI/Swift builds. Real-weight release test executables from the gate were additionally run with an explicit cache environment: all six `ml-enhance` DRUNet tests, the masked/unmasked post-adapter test, and the raw noise/parity test passed (see `real-models.log` and `real-weights.log`). The downloaded model SHA-256 was independently rechecked. CoreML printed the existing E5RT teardown diagnostic after successful inference/test exit; it was not suppressed. LibRaw's existing C++ build warnings remain.
