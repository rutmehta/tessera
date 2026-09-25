# ml-enhance (M3-05 partial implementation)

M3-05 is NOT complete. This crate implements real Real-ESRGAN x2/x4 inference,
a finite-support tiling executor, denoise blending, and the `CfaDenoise` extension
point. It does not yet provide a validated NAFNet denoiser or wire denoise into
the pipeline graph. Passing infrastructure tests is not denoise quality evidence.

## Super-resolution

`SuperResolution::load(registry, factor, options)` explicitly resolves a pinned
model (and may download it). `super_resolution(rgb, factor)` uses ml-runtime's
ORT/CoreML session, with CPU fallback. Input is planar display-encoded RGB in
[0,1]. Both scale and SHA-256 must match the selected model. Do not feed raw CFA
or linear Rec.2020 directly to the display-RGB-trained network.

`export::export_one_upscaled` runs SR after the existing sRGB render and before
resize/output sharpening/ICC encoding. It keeps the existing atomic no-clobber
publication and metadata policy. Standard export and batch export are unchanged.
This adapter now uses an undithered f32 display transform before inference.
Ordinary exports retain their existing renderer for byte-identical output.

The application's `--upscale 2|4` parser remains unwired: its implementation is
in `apps/tessera-cli/src/export.rs`, outside M3-05's permitted edit paths. The
new function takes a caller-owned session to avoid network access or hidden
model selection during ordinary exports. Batch upscale is not implemented.

### Selection and provenance

Selected Real-ESRGAN instead of SwinIR because its convolutional RRDB architecture
has finite spatial support and avoids attention-window alignment/context issues.
There is no diffusion model. Learned textures can still differ from reality.

- Upstream: https://github.com/xinntao/Real-ESRGAN
- BSD-3-Clause: https://github.com/xinntao/Real-ESRGAN/blob/master/LICENSE
  Copyright (c) 2021, Xintao Wang. Retain upstream license when redistributing weights.
- ONNX exporter/model card:
  https://huggingface.co/SceneWorks/real-esrgan-onnx/blob/09f741bac80a246b407da3ee902bf5f3291b602f/README.md
- Pin: `09f741bac80a246b407da3ee902bf5f3291b602f`.
- x2 SHA-256: `7115ba92e8a1bfa63d68558ef006ef3d91273a068d321b1439f8bb1c9179002c`.
- x4 SHA-256: `5c586662929cbc686c1a5c38d9c060dbdb4ea5863a1f7672b8c0761e6b89c033`.
- Both downloaded artifacts were hashed locally. URLs, hashes and probe tensor
  shapes are in `crates/ml-runtime/models.toml`. Weights are not source files.

These are fp32 exports. No fp16 accuracy claim is made. CoreML actually executes
subgraphs, but dynamic shape/resize operations can remain on CPU. Inspect the
returned `partition_report`; requesting CoreML does not prove complete offload.

### Tiling contract

RRDB has 23 residual-in-residual blocks, each with three dense blocks of five
3x3 convolutions. The radius bound also reserves seven input-grid convolution
steps for stem/body/upsampling/output. x2's initial pixel-unshuffle doubles
support in input coordinates. The conservative halos are therefore 704 input
pixels for x2 and 352 for x4. This is much larger and slower than the upstream
approximate 16-pixel overlap, deliberately prioritizing complete spatial support.
Interior size is 128 input pixels. x2 patches start on even coordinates.

Source architecture:
https://github.com/XPixelGroup/BasicSR/blob/master/basicsr/archs/rrdbnet_arch.py

At image boundaries, odd x2 dimensions are edge-padded to even, and the result
is cropped back to exactly factor*original dimensions. The model supplies other
boundary padding. There is no per-tile normalization or blending of borders.
The complete input and output remain resident in host memory, plus a patch and
its model activations. This has not been performance-qualified for large raws.

The generic `run_tiled` API accepts a caller-proven finite `SpatialContract`.
It rejects insufficient halos and phase-inconsistent tiling. It is NOT valid
for global attention/pooling architectures.

## Denoise status and why NAFNet is not registered yet

Preferred candidate: NAFNet SIDD (MIT), not Restormer, for its simpler restoration
architecture and clearly published permissive license. This is a candidate,
not a delivered production model.

- https://github.com/megvii-research/NAFNet/blob/main/LICENSE
- https://github.com/megvii-research/NAFNet/blob/main/docs/SIDD.md
- https://github.com/megvii-research/NAFNet/blob/main/basicsr/models/archs/NAFNet_arch.py
- https://huggingface.co/LibreYOLO/LibreNAFNetl-restore-sidd

The stock NAFBlock uses `AdaptiveAvgPool2d(1)` for channel attention. No finite
halo independent of image size provides exact full-frame equivalence. Upstream
NAFNetLocal replaces pooling with Test-time Local Conversion, which is a distinct
inference contract needing a pinned export and a derived receptive radius.
The SIDD checkpoint is trained/evaluated in sRGB, not arbitrary unbounded camera
linear RGB. Simply running stock weights over independent linear-RGB patches
would not establish the requested PSNR/seam guarantees.

Remaining model work: reproduce a licensed SIDD ONNX export, pin its actual bytes,
choose and version the color adapter and TLC contract, validate >=3 dB PSNR on
the synthetic gradient and full-vs-tiled <=1e-4, then validate fp16. No fake hash,
identity denoiser, smoothing substitute, or fixture-based PSNR claim is provided.

`denoise_with` currently provides amount (0..100) and M2-08-style raster mask
blending in linear light, validates shape/finiteness/ranges, and bypasses the
inference callback for amount zero or an all-zero mask. Zero-alpha pixels retain
their bits, including signed zero. `NoiseModelHint` is advisory read/shot variance,
not falsely advertised as conditioning an unconditioned SIDD network.
`CfaDenoise` explicitly reserves future joint raw inference, which needs training.

## Engine-api integration gaps (engine-api unchanged)

Existing `DenoiseMethod::Neural` stores a ModelRef and `joint_demosaic`, but its
contract says CFA and StageId::Denoise precedes Demosaic. Phase-1 RGB denoise needs
an explicit domain/placement contract (for example, a distinct post-demosaic
neural method), with preprocessing/model revision in cache keys. Do not silently
reinterpret existing joint-CFA recipes as post-demosaic sRGB inference.

DenoiseSettings lacks a denoise-local mask reference. It needs a persistent mask
or mask-stack reference and the mask raster revision/frame in the denoise cache
key. A noise-hint/calibration revision should also be pinned when conditioning
is eventually supported. Export tool settings need optional upscale factor and
model reference. Chroma-only is already present but is not implemented here.

Pipeline-cpu and image-core denoise execution/caching are still unchanged and
unsupported neural settings continue to fail explicitly. Existing off-render
regressions are run, but that is not evidence of an integrated denoiser.

## Tests

Run the work-package gate from the workspace with CARGO_TARGET_DIR outside it:

    cargo test -p ml-enhance -p pipeline-cpu -p image-core -p export --release
    cargo clippy -p ml-enhance -p pipeline-cpu -p image-core -p export --all-targets -- -D warnings
    cargo fmt --check

Real model tests look in TESSERA_ENHANCE_MODEL_CACHE or
`tools/orchestrate/wp/M3-05/.cache`, for SHA-addressed `.onnx` files. They skip
explicitly if absent, never download during tests, and fail if a cached model is
corrupt. The x2 sharp-edge test and PNG export test execute actual weights. The
x4 test checks real output dimensions and provider reporting. Generic tiling
seams are tested against ml-runtime's convolution fixture. `model_seams.rs`
additionally compares real x2/x4 weights against full-frame CPU fp32 inference
on long, narrow images wider than the complete halo. Both measured maximum
errors were zero. This validates actual partial-halo stitching, but is not
denoise quality evidence or a CoreML precision-equivalence claim. Nearest-neighbour in the
coordinate test is only a scaling/cropping oracle, not the SR implementation.
