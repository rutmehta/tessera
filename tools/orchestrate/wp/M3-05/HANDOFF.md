# Historical M3-05 handoff (superseded by FINAL_HANDOFF.md)

## Delivered

- New ml-enhance crate with real pinned Real-ESRGAN x2/x4 ml-runtime inference.
- Phase-aligned, finite-support halo tiling and shape/finite-value validation.
- Linear-light amount/mask blending, bit-exact zero bypass, advisory sensor noise
  type, and CfaDenoise extension trait.
- Explicit export_one_upscaled integration before resize/sharpen/encode, without
  changing ExportSettings or existing export/batch behavior.
- SR export now consumes undithered f32 display RGB, not an 8-bit intermediate.
  The original display() implementation is retained unchanged. New regression
  tests verify sub-byte detail, legacy quantization and undithered model input.
- Real x2/x4 full-vs-tiled tests now exercise images wider than the complete halo,
  including genuinely different partial-halo patches. Both measured max errors
  are zero on CPU fp32, within the requested 1e-4 threshold.
- BSD-3 model URLs/SHA-256s in models.toml, independently downloaded and hashed.
  Downloaded weights live only in the ignored .cache directory and are not committed.
- Model research, architecture constraints and needed engine-api fields are in
  crates/ml-enhance/README.md. engine-api was not changed.

## Actual verification

The exact requested command was run successfully after final Rust edits:

    cargo test -p ml-enhance -p pipeline-cpu -p image-core -p export --release && cargo clippy -p ml-enhance -p pipeline-cpu -p image-core -p export --all-targets -- -D warnings && cargo fmt --check

Exit 0. Aggregate harness results: 156 passed, 0 failed, 3 ignored.
CARGO_TARGET_DIR stayed /Users/rutmehta/.cache/tessera-target/M3-05.
git diff --check passed. No commits or pushes were performed.

Real x2/x4 weights were present and exercised (not skipped). x2 sharp-edge and
actual doubled PNG export checks passed. Provider evidence is in
coreml-partitions.log: both models execute CoreML subgraphs; shape/resize nodes
fall back to CPU, with CoreML unbounded-shape diagnostics. No all-CoreML or fp16
accuracy claim is made. validation.log contains the full gate output, including
pre-existing bundled LibRaw C++ warnings. upscale-test.log records export smoke.

## Acceptance gaps

- No validated NAFNet ONNX artifact, denoise inference implementation or >=3 dB
  denoise PSNR test. denoise_with is blending infrastructure, not a denoiser.
- No Denoise-stage execution or model-keyed denoise caching in pipeline-cpu/image-core.
- Production SR seam checks cover CPU fp32 on narrow images, not CoreML
  precision equivalence or a large-raw performance qualification.
- No fp16 validation or learned Raw Details/capture sharpening implementation.
- No --upscale CLI argument or batch upscale wiring. The CLI parser is outside
  the allowed edit paths (apps/tessera-cli/src/export.rs).
- Local mask rasters are accepted by the blend API, but not wired to recipe masks.

NAFNet stock global pooling and SIDD sRGB training make a naive small-halo
linear-RGB implementation incorrect. A pinned NAFNetLocal/TLC conversion and
color-domain adapter require real accuracy/seam validation before pipeline
integration. Do not replace this gap with fake weights, identity/smoothing
inference or a fixture-based PSNR claim.

The current LibreNAFNet SIDD model card was rechecked at revision
98351e18aeb71db815ce07dfd8e583c95f0135cc. It declares MIT and repackages upstream
width-64 weights, but publishes only a PyTorch checkpoint, not a ready ONNX/TLC
artifact. No unverified converted model or invented checksum was registered.
https://huggingface.co/LibreYOLO/LibreNAFNetl-restore-sidd/tree/98351e18aeb71db815ce07dfd8e583c95f0135cc

New evidence: model-seams.log; float-red.log / float-green.log and
export-float-red.log / export-float-green.log record targeted test-first runs.
The exact full gate above was rerun after all Rust changes. All edits remain
inside the allowed paths. No target directory was created in the worktree.

Required decisions for completion: authorize a post-demosaic RGB denoise
contract rather than silently changing the existing CFA recipe semantics, and
allow apps/tessera-cli/src/export.rs for the requested CLI flag. Engine-api
field requirements remain recorded in crates/ml-enhance/README.md.

RESULT: FAIL M3-05 is only partially implemented; denoise model, pipeline/cache integration and production acceptance checks remain incomplete.
