# ml-enhance (M3-05 phase-1 RGB enhancement)

This crate implements real Real-ESRGAN x2/x4 and DRUNet RGB inference,
a finite-support tiling executor, linear-light denoise blending, and the
`CfaDenoise` extension point, post-demosaic integration and export CLI wiring.
See [DRUNet API, validation and limits](DRUNET.md) for the new denoiser.


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

The application supports `--upscale 2|4` using app-local `models.toml` and
`models/`. `export_batch_upscaled` shares one session, processes serially,
preflights duplicate names and checks cancellation before atomic publication.
The CLI uses CPU inference because native CoreML diagnostics contaminate its
JSON stdout. Library callers can use CoreML and inspect partition reports.

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

## Denoise selection

Selected MIT DRUNet instead of the initially considered NAFNet SIDD. DRUNet has
finite spatial support and an available pinned ONNX export. Full provenance,
derived halo, color adapter, measured PSNR and limitations are in DRUNET.md.

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

The implemented alternative uses a 192-pixel halo, stride-8 alignment and
fixed display-domain sigma 25/255. Sensor read/shot hints remain advisory.

`denoise_with` currently provides amount (0..100) and M2-08-style raster mask
blending in linear light, validates shape/finiteness/ranges, and bypasses the
inference callback for amount zero or an all-zero mask. Zero-alpha pixels retain
their bits, including signed zero. `NoiseModelHint` is advisory read/shot variance,
not falsely advertised as calibrated sensor conditioning.
`CfaDenoise` explicitly reserves future joint raw inference, which needs training.

## Pipeline integration (engine-api unchanged)

Per the coordinator decision, only the pinned DRUNet ModelRef with
`joint_demosaic=false` is accepted as phase-1 RGB denoise. The raw StageId::Denoise
is reserved. The RGB result is the Demosaic tail, cached under the chained
demosaic/denoise settings plus adapter revision. Tone and WB edits reuse it.
Unknown versions, true joint CFA and chroma-only settings fail explicitly.

Pipeline-cpu exposes `PostDemosaicDenoise` and
`render_linear_scaled_with_denoise`, with no ML dependency. Enable image-core's
`ml-denoise` feature, construct `MlPostDemosaicDenoise` with a caller-owned
registry/options, and inject it with `Renderer::with_post_demosaic_denoise`.
The session loads lazily on nonzero inference; off/zero never resolve weights.
There is no global registry or hidden model selection.

Camera RGB is converted through camera XYZ to bounded linear sRGB. The
out-of-range residual is preserved and added back before the inverse matrix.
Inference requires a whole-sensor host barrier (including viewport requests);
the model itself tiles. Active denoise avoids the GPU resident shortcut. Partial
demosaic-cache residency recomputes the full image. Existing f16 memo storage
can move cold/warm displayed values by one byte. Large-raw memory/performance
and fp16 model accuracy are not qualified.

`MlPostDemosaicDenoise::with_mask(width, height, samples)` accepts M2-08 raster
samples in full level-0 sensor coordinates. Samples/extent are included in the
adapter cache revision. An all-zero raster bypasses model loading. Rasterize
against the complete sensor frame, not a cropped preview.

DenoiseSettings lacks a denoise-local mask reference. It needs a persistent mask
or mask-stack reference and the mask raster revision/frame in the denoise cache
key. A noise-hint/calibration revision should also be pinned when conditioning
is eventually supported. Export tool settings need optional upscale factor and
model reference. Chroma-only is already present but is not implemented here.

True CFA joint inference and learned Raw Details need separate trained models;
capture-sharpening deconvolution is not supplied by these restoration weights.

## Tests

Run the work-package gate from the workspace with CARGO_TARGET_DIR outside it:

    cargo test -p ml-enhance -p pipeline-cpu -p image-core -p export -p tessera-cli --release
    cargo clippy -p ml-enhance -p pipeline-cpu -p image-core -p export -p tessera-cli --all-targets -- -D warnings
    cargo fmt --check

Also exercise the optional runtime adapter:

    cargo test -p image-core --features ml-denoise --release
    cargo clippy -p image-core --features ml-denoise --all-targets -- -D warnings

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
