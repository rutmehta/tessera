# Remove: pinned big-LaMa ONNX

No training or export is performed by Tessera. No weights are committed. We use
the unmodified `lama_fp32.onnx` export from **Carve/LaMa-ONNX**, derived from
advimman/big-lama. Do not substitute the slower `lama.onnx` opset-18 graph.

## Provenance and licence

- Export repository revision: `c3c0c9e468934d62e79c329e35d82dd09ff8c444`.
- Immutable artifact: https://huggingface.co/Carve/LaMa-ONNX/resolve/c3c0c9e468934d62e79c329e35d82dd09ff8c444/lama_fp32.onnx
- SHA-256: `1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6`.
- Size: **208044816 bytes**. Opset 17, float32.
- The pinned [publisher model card](https://huggingface.co/Carve/LaMa-ONNX/blob/c3c0c9e468934d62e79c329e35d82dd09ff8c444/README.md)
  explicitly identifies big-lama provenance and declares **Apache-2.0 weights**.
- The [Git LFS pointer](https://huggingface.co/Carve/LaMa-ONNX/raw/c3c0c9e468934d62e79c329e35d82dd09ff8c444/lama_fp32.onnx)
  supplies the expected SHA-256 and size. The complete downloaded file was also
  hashed locally, not just trusted from CDN metadata.
- [Exporter source](https://github.com/Carve-Photos/lama/blob/f5fb39a18022c34a71bf9a47a6ec393c804b49ca/export_LaMa_to_onnx.ipynb)
  and its custom Fourier implementation are covered by the fork's
  [Apache-2.0 licence](https://github.com/Carve-Photos/lama/blob/f5fb39a18022c34a71bf9a47a6ec393c804b49ca/LICENSE),
  copyright 2021 Samsung Research. Original project: https://github.com/advimman/lama.
- This is publisher licence/provenance evidence, not a claim to have audited
  training dataset rights. Preserve Apache-2.0 licence and upstream notices if
  redistributing weights. The repository does not redistribute them.

The manifest stores size, licence and resolution rules in comments, matching
existing models.toml conventions (ModelSpec denies unknown fields). The adapter
pins both revision and expected SHA-256. ml-runtime verifies downloaded AND
cached bytes, with atomic cache population. A mismatched cache is an error, not
an excuse to silently use PatchMatch or redownload.

## Exact graph and image contract

- `image`: float32 `[1,3,512,512]`, RGB display-sRGB **[0,1]**.
- `mask`: float32 `[1,1,512,512]`, binary, **1 = replace**.
- `output`: float32 `[1,3,512,512]`, RGB display-sRGB **[0,255]**.
- Batch is symbolic `batch` in the file and pinned to one before EP assignment.
- Spatial inputs are **fixed 512**, not arbitrary multiples of eight. Output
  spatial labels are symbolic in the artifact, but the actual output is 512.
- The exporter applies the mask and composites original pixels itself. Tessera
  zeros removed pixels as well, divides the output by 255, and decodes to linear
  light. The generic `from_session` adapter retains the previous stride-eight,
  normalized-output contract for caller-verified models/fixtures. It is not the
  entry point for the pinned Carve export.

`Remove::apply` accepts finite bounded linear sRGB, not HDR or camera primaries.
It dilates coverage in canvas pixels, bounds the long working edge to 512 while
preserving aspect ratio, conservatively pools mask coverage (thin wires survive),
and box-averages display RGB. Small images are not enlarged. The ONNX adapter
edge-pads to 512 and crops before bilinear reconstruction to canvas resolution.
Only covered pixels are published. Original alpha and unselected pixels remain
bit-identical. New-layer output retains the existing CAF coverage contract.

For non-None colour adaptation, local RGB mean/contrast are matched to the
16-pixel exterior ring with bounded gain (0.5..2) and mean shift (+/-0.1 linear).
This is photometric harmonization, not retraining or fabricated texture. Flat
predictions cannot gain texture from this operation. High/VeryHigh currently
use the same bounded correction as Default; only None disables it.

Paste-back solves a discrete gradient-domain problem in the eight-pixel inner
boundary band: original exterior and model interior are fixed Dirichlet values,
model gradients guide inner edges, and zero normal gradient guides crossings
(no gradients from the removed object's source pixels). 64 Gauss-Seidel sweeps
bound the work; this is a finite-iteration boundary blend, not a claim of exact
full-hole Poisson convergence. Soft coverage is blended afterward in linear light.
Cancellation is checked throughout preprocessing/blending and before/after ORT;
an in-flight ORT run itself is not interruptible through this adapter.

## Loading and automatic selection

- `OnnxInpainter::load(&registry, options)` explicitly resolves/downloads missing
  weights. This is the opt-in installation path.
- `OnnxInpainter::load_local` only accepts a verified, already populated registry
  cache, despite retaining its legacy method name. It never imports or fetches.
- `<dyn Remove>::auto(&registry, options)` constructs a reusable `AutoRemove`:
  cached model -> ONNX; missing -> CPU PatchMatch. Corrupt/incompatible weights
  and inference failures propagate. Explicit Backend::Onnx never silently falls
  back to PatchMatch. Backend::Cpu bypasses an available model.
- The free `remove(..., model, ...)` remains the injection API and does not
  discover a global cache; application code should own/reuse `AutoRemove`.
- Default macOS options request CoreML MLProgram / All compute units, with ORT
  CPU fallback. LaMa has substantial mixed-provider work, so its session has a
  six-thread intra-op CPU budget. Other runtime loaders retain one thread.
  Runtime CPU workers sleep instead of spinning while CoreML is busy.
  A CPU-executed ONNX model is still BackendUsed::Onnx, not CpuPatchMatch.
- `partition_report()` reports actually executed provider assignments.
  `require_coreml()` still rejects mixed execution. Partial CoreML acceleration
  must NOT be described as full ANE execution. `fallback_reason()` reports an EP
  initialization failure, not ordinary per-node CPU partitioning.

## Reproducing real-model tests

From repository root, retain the caller's external CARGO_TARGET_DIR. On the
M3-20 worktree the expected value is `/Volumes/betterSSD/tessera-cache/target/M3-20`.

```sh
# Explicit download-on-demand through ml-runtime, then quality and timing tests.
TESSERA_REMOVE_MODELS=1 cargo test -p filters --release --test remove_models -- --nocapture

# Offline test reuse, with a user-selected content-addressed model cache.
TESSERA_REMOVE_MODEL_CACHE=/absolute/cache cargo test -p filters --release --test remove_models -- --nocapture

# Default cache: tools/orchestrate/wp/M3-20/.cache (gitignored).
# Missing weights print SKIP without network access. Present corrupt weights fail.
cargo test -p filters -p ml-runtime --release && cargo clippy -p filters -p ml-runtime --all-targets -- -D warnings && cargo fmt --check
```

Manual acquisition used to independently check publisher bytes:

```sh
mkdir -p tools/orchestrate/wp/M3-20/.cache
curl -fL --retry 2 'https://huggingface.co/Carve/LaMa-ONNX/resolve/c3c0c9e468934d62e79c329e35d82dd09ff8c444/lama_fp32.onnx' \
  -o tools/orchestrate/wp/M3-20/.cache/1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6.onnx
shasum -a 256 tools/orchestrate/wp/M3-20/.cache/*.onnx
```

The quality regression uses an occluding red square over a coloured sinusoidal
texture plus illumination gradient. It checks boundary MAE <0.015, local RGB
mean/std distance versus the same default CpuPatchMatch input, exact exterior
and alpha preservation, and actual cached Auto selection. These are synthetic
regressions, not a claim of universal superiority over PatchMatch on photographs.

The macOS performance test runs a complete 1024-square Remove operation at
512 working resolution after session creation and one warm-up, asserts <2 s,
and demands an executed CoreML partition. It includes dilation, resampling,
colour adaptation, boundary blending and paste-back, but excludes model
installation/compilation. Real-model tests use a mutex to avoid measuring CoreML
while another LaMa test saturates the CPU. Do not run competing benchmarks.

For provider/thread diagnosis (raw ONNX inference, not the full Remove pipeline):

```sh
cargo run -p ml-runtime --release --example lama_profile -- \
  tools/orchestrate/wp/M3-20/.cache/1faef5301d78db7dda502fe59966957ec4b79dd64e16f03ed96913c7a4eb68d6.onnx 6 all
```

## Distraction stub

`DistractionDetector` produces separate wire and people coverage planes and has
an explicit `remove` convenience method consuming their validated union through
any `dyn Remove`. `CpuDistractionDetector` uses two-sided contrast plus elongated
connected components for narrow horizontal/vertical/diagonal structures. It
cannot identify semantic wires reliably in arbitrary scenes. Selected
`ml_faces::Face` boxes are expanded 50% on each side and clipped to the canvas.
They are face-box proxies, **not full-body segmentation**; no face weights are
fetched and no faces are inferred inside this hook. Suggestions should be
reviewed before removal. Tests exercise the wire-mask-to-PatchMatch pipeline,
face expansion/clipping, flat/broad negative controls and cancellation.
