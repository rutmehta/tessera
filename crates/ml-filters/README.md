# ml-filters (M3-21, round-2 scope)

Apache-2.0 standalone raster filters. Nothing is registered in the compositor.
`catalog()` is offline metadata with names, scalar schemas, weight requirements,
and explicit feature limitations for the three shipped filters: Skin Smoothing,
Colorize, and JPEG Artifact Removal. Denoise-only Photo Restoration remains an
explicit API, not a shipped catalog entry. Calling `load` opts into the existing atomic,
SHA-256 checked model registry; constructing the catalog never downloads.

## Contract

`NeuralFilter::apply(&self, &Raster, &Params, &Cancel) -> anyhow::Result<Raster>`
returns a new raster. This deliberately uses a fallible return rather than the
brief's bare `Raster`: missing/corrupt weights, unsupported controls, invalid
input, and cancellation must not silently publish an unchanged or partial edit.
The eventual compositor adapter must handle these errors.

Inputs are normalized, bounded **display-sRGB**, three-channel RGB or straight
RGBA. U8/U16/F32 rasters retain depth, dimensions and alpha. HDR/non-finite input
is rejected rather than implicitly tone-mapped. Callers must convert document
color spaces first. JPEG and restoration decode display RGB to linear sRGB for
`ml_enhance::Denoiser` and encode its result back. Its existing tiled/padded/
stitched DRUNet path is reused unchanged.

Cancellation is cooperative during raster processing. Active ONNX execution,
including the existing DRUNet tiled invocation, cannot be interrupted by this
adapter. Cancellation is checked before and after inference; cancelled work is
never returned. A session mutex serializes inference. Zero amount on skin/JPEG/
restoration returns a COW clone without inference. Read-only input is never edited.

## Filters

- Skin Smoothing: no weights. Caller supplies pixel-coordinate x/y/width/height
  face boxes (or `Params::with_faces(&[ml_faces::Face])` from YuNet). Empty boxes
  mean no-op, not automatic full-frame treatment. A feathered ellipse intersected
  with a broad YCbCr skin-color heuristic limits changes. Separable bilateral
  low/high separation retains the high residual and smooths the low band.
  Blur is 0–16 pixels, Smoothness 0–1. The color heuristic is not a learned skin
  segmentation model and needs real-world validation across skin tones/lighting.
- Colorize: verified DDColor paper-tiny fp16 graph with fp32 I/O, 512-square
  inference; bilinear chroma upsampling with original full-resolution Lab L.
  Artifact Reduction 0–1 is an L-guided chroma bilateral filter. Saturation 0–2
  scales ab. `Params::hints` contains `ColorHint` with pixel position, positive
  radius, display-RGB color and strength 0–1. Hints blend only ab and never L.
  Final gamut clipping can change L for out-of-gamut colors. This export fails
  the full CoreML guard, so `Colorize::load` explicitly selects
  `SessionOptions::with_execution_preference(ExecutionPreference::CpuOnly)`.
  This overrides CoreML requests for DDColor only, not other models. The runtime
  default and strict partition guard are unchanged; CPU reports still fail it.
- JPEG Artifact Removal: Strength 0–1 conditions DRUNet's blend amount on excess
  8-pixel boundary energy. This is not recovery of actual encoder quantization
  tables. Shifted/cropped JPEG grids and real grid patterns can fool it. Fixed
  DRUNet sigma is unchanged; the quality estimate conditions the blend, not the
  model's noise plane. Real q=30 synthetic PSNR regression passes.
- Photo Restoration (no face model): **denoise-only, outside the catalog**.
  Photo enhancement 0–1 performs whole-frame DRUNet denoising. GFPGAN v1.4 is
  excluded by the round-2 scope decision because of its StyleGAN2/NVIDIA
  non-commercial license lineage, not pending implementation. No face crop,
  GFPGAN inference, or feathered paste-back is claimed. Scratch reduction is
  **not implemented**; no approved scratch model was selected. Both unavailable
  controls advertise max=0 and reject nonzero values. This is not a claim that
  no Apache-2.0 scratch model exists anywhere.

See [MODELS.md](MODELS.md) for immutable provenance and the GFPGAN exclusion.
The historical local export fallback is fail-closed, outside the shipping scope,
and remains unexecuted beyond its guard. Re-exporting does not fix licensing.
No weights or Python environments are tracked.

## Verification

Keep the caller's external `CARGO_TARGET_DIR`; do not build into this repository.

```
cargo test -p ml-filters -p ml-runtime --release
cargo clippy -p ml-filters -p ml-runtime --all-targets -- -D warnings
cargo fmt --check
```

Normal tests skip real weights with an explicit SKIP message when cache is unset
or the exact hash-addressed artifact is absent. Use `--nocapture` to see skips.
Corrupt cached artifacts remain failures. Real integration, without downloads:

```
TESSERA_FILTER_MODEL_CACHE=/path/to/cache cargo test -p ml-filters --release -- --nocapture
```

The cache holds `<sha256>.onnx` files. The explicit `load` APIs may populate it
on demand, but tests check cache presence first.

Ignored benchmarks log end-to-end filter time on 4000x3000 input, excluding model
load and test fixture creation. DRUNet benchmarks request CoreML and call
`require_coreml()` on executed provider reports; partial fallback fails the probe
instead of being advertised as CoreML-only. DDColor audits CPU-only assignments
and asserts that the CoreML guard still rejects them. Skin is CPU-only by design.

```
TESSERA_FILTER_MODEL_CACHE=/path/to/cache cargo test -p ml-filters --release --test bench -- --ignored --nocapture --test-threads=1
```

See `tools/orchestrate/wp/M3-21/RESULTS.md` for measurements, scope gaps, and logs.
