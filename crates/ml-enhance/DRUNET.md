# DRUNet RGB denoise

## API and color contract

`Denoiser::load(&ModelRegistry, SessionOptions) -> Result<Denoiser>` explicitly
resolves pinned weights (may download). Methods take `&mut self`:

- `denoise(&Tensor, amount: f32, Option<NoiseModelHint>) -> Result<Tensor>`
- `denoise_masked(&Tensor, amount: f32, Option<NoiseModelHint>, mask: &[f32]) -> Result<Tensor>`
- `partition_report() -> Result<PartitionReport>` finalizes actual executed-node
  profiling after representative inference, including any CPU fallback.

Input is NCHW **bounded linear sRGB**, not camera-linear or linear Rec.2020.
Adapter `linear-srgb-v1-sigma25` applies the sRGB transfer function, appends a
fourth channel fixed at sigma=25/255, runs DRUNet, clips restored display RGB to
[0,1], and decodes to linear sRGB. Existing `denoise_with` blends in linear light
using amount/100 times the H*W raster mask. Amount is **blend**, not sigma.
Nonzero inference rejects negative/HDR input rather than silently clipping scene
data. Callers must establish sRGB primaries and a bounded domain before inference.

Amount zero and all-zero masks bypass inference, returning bit-exact clones even
for finite out-of-range values and signed zero. Zero-alpha pixels retain their
bits. `NoiseModelHint` is validated but **ignored** in adapter v1: sensor read/shot
variance is not display-domain AWGN sigma without calibration and propagation
through color/transfer transforms. No automatic noise estimation, sensor-quality
validation, chroma-only or joint CFA inference is claimed. `CfaDenoise` remains
an explicit extension point for future trained raw models.

## Provenance and spatial contract

- MIT model card/export:
  https://huggingface.co/synthscript/drunet-color-onnx/blob/a2b9fccfa27b197f44a3876c567f5e48970c44a7/README.md
- Upstream architecture/weights: https://github.com/cszn/KAIR
- MIT license: https://github.com/cszn/KAIR/blob/master/LICENSE
  Retain upstream license when redistributing weights.
- Model ID: `enhance/drunet-color`.
- Revision: `a2b9fccfa27b197f44a3876c567f5e48970c44a7`.
- Locally measured SHA-256:
  `2ae3ab5eb15daac2ee79be984d584b908ce7f0f60b27be87d005f728c2aa0087`.
- Inspected export: fp32 `input` [N,4,H,W], fp32 `output` [N,3,H,W];
  61 Conv, 3 ConvTranspose, 32 Add, 28 Relu nodes. H/W divide by 8.
- Exposed `DENOISE_MODEL_ID`, `DENOISE_VERSION`, `DENOISE_SHA256`,
  `DENOISE_ADAPTER_VERSION`, `DENOISE_SIGMA` constants pin these contracts.

The local residual U-Net dependency radius is 185; halo 192 aligned to 8 covers
support. Interior tiles are 128; bottom/right edges replicate to multiples of 8
and output crops to original dimensions. Complete input, encoded RGB and output
remain resident, plus patch activations. This conservative halo is expensive.

Stock NAFNet SIDD was rejected because `AdaptiveAvgPool2d(1)` introduces global
context: no fixed finite halo establishes exact full-frame equivalence. A TLC
export would need a distinct pin and support proof. SIDD sRGB checkpoints also
are not camera-linear models. DRUNet display-AWGN training is an alternative,
**not** a claim of SIDD or sensor-noise quality.

## Actual validation and limits

`tests/denoiser.rs` uses cached real weights, never downloads during tests, and
prints SKIP if the SHA-addressed file is absent. With downloaded weights:

- Deterministic 81x65 gradient, Gaussian display-domain noise sigma=25/255,
  converted to linear for input and PSNR: 20.376933 -> 38.324625 dB,
  **+17.947691 dB**. Synthetic evidence, not a natural-photo benchmark.
- CPU fp32 full/tiled maximum error **0** in both model and linear-adapter
  domains on 640x16 and 16x640 images. Every patch is smaller than the full
  frame; patch lengths 320, 448, 512 establish genuinely distinct contexts.
- Bit-exact amount-zero/mask bypass; linear partial-mask blending; hint
  invariance; odd-size crop and invalid-input rejection pass.
- macOS default-session probe executed one fused CoreML subgraph, no CPU nodes
  in that report. E5RT emitted a shape-inference diagnostic on teardown despite
  successful inference and test exit. Large-image/device performance is unqualified.
- **No fp16 export is registered and no fp16 accuracy claim is made.** CoreML
  internal precision is provider-controlled; CPU fp32 seam results do not prove
  CoreML or fp16 numerical equivalence.

Pipeline/CLI integration is separate; do not silently reinterpret pre-demosaic
CFA recipes as this post-demosaic RGB adapter. Include model revision, adapter
revision, amount and mask content/revision in caller cache keys.

Run from the worktree:

```sh
CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-05 cargo test -p ml-enhance --release -- --nocapture --test-threads=1
```

Weights live in ignored `tools/orchestrate/wp/M3-05/.cache/<sha>.onnx`, or set
`TESSERA_ENHANCE_MODEL_CACHE`. Red/green validation logs are in the work-package
scratch directory. The initial API test failed with missing Denoiser symbols;
the quality test then failed explicitly at the not-yet-implemented inference
boundary before the real session adapter was added. No identity/filter stand-in
is used for quality evidence.
