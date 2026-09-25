# M3-05 relaunch handoff

This supersedes HANDOFF.md and RETRY.md from the earlier incomplete pass.

## Delivered

- MIT DRUNet RGB denoiser, pinned ONNX URL/revision/SHA-256 in models.toml. Actual bytes independently hashed. Selected over NAFNet because its finite support permits exact aligned halo tiling rather than global-pooling approximations.
- Linear-sRGB transfer adapter, 192-pixel halo/stride-8 alignment, amount blending, optional M2-08 raster masks, noise-hint validation, and CfaDenoise future-training extension point.
- Coordinator-approved post-demosaic tail via pipeline-cpu PostDemosaicDenoise trait, with no runtime dependency in pipeline-cpu. Camera-to-linear-sRGB adapter preserves unbounded residual. Raw StageId::Denoise stays reserved.
- image-core optional `ml-denoise` feature supplies lazy real runtime adapter and full-sensor inference barrier. Demosaic caches include denoise settings/model and adapter/mask revision. Tone/WB reuse is tested. Resident path falls back for active denoise.
- Real-ESRGAN x2/x4 export, CLI `--upscale 2|4`, serial shared-session batches, cancellation before publication, duplicate-name preflight and atomic no-clobber output.
- engine-api unchanged. Missing persistent denoise-mask reference, explicit RGB placement metadata, sensor-noise calibration revision and export upscale/model fields are documented in crates/ml-enhance/README.md.

## Verification performed by parent

Exact required command executed after final Rust edits:

    cargo test -p ml-enhance -p pipeline-cpu -p image-core -p export -p tessera-cli --release && cargo clippy -p ml-enhance -p pipeline-cpu -p image-core -p export -p tessera-cli --all-targets -- -D warnings && cargo fmt --check

Exit 0: 193 passed, 0 failed, 3 ignored. Evidence: final-validation.log.

Additional optional adapter release suite and Clippy passed with `--features ml-denoise`. Evidence: feature-validation.log. Real cached DRUNet ran through both pipeline-cpu and image-core, not just a fake hook. Zero sensor mask avoids downloads and changes cache identity.

Real model re-execution: linear-light PSNR 20.376933 -> 38.324625 dB, gain 17.947691 dB. DRUNet reports one executed CoreML fused subgraph. CPU full/tiled tests exercise distinct partial-halo patches. Real x2/x4 exports and CLI dimension checks ran with cached weights. Evidence: final-model-evidence.log and final-validation.log.

CARGO_TARGET_DIR remained /Users/rutmehta/.cache/tessera-target/M3-05. No worktree target/ exists. `git diff --check` passed. All changed/untracked paths are within the allowlist. No commits or pushes.

## Explicit limits

- Models remain fp32. fp16 accuracy is not qualified, and no fp16 accuracy claim is made.
- CLI SR deliberately uses CPU: CoreML native dynamic-shape diagnostics contaminate JSON stdout. Library SR supports CoreML with CPU fallback and partition reporting; DRUNet CoreML execution is verified.
- Model sigma is fixed at 25/255 in display space, not calibrated sensor noise. This is phase-1 RGB restoration, not joint CFA denoise, learned Raw Details or capture-deconvolution sharpening.
- Full-image host buffers are retained; large-raw performance/memory is not qualified. Partial cache residency recomputes full inference. Active inference is not interruptible mid-call.
- Existing renderer memo storage is f16; cold/warm display samples may differ by one byte. Amount-zero bypass itself is bit-exact.
- Masks are caller-supplied immutable level-0 sensor rasters. Recipe persistence awaits engine-api fields. CLI/export does not automatically inject a denoise registry for neural recipes; applications use the explicit renderer hook.

RESULT: PASS
