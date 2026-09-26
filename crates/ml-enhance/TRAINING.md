# CFA denoise training (M3-16, phase 2a)

## Status and limits

This is an opt-in, small Bayer CFA denoiser, not a DeepPRIME-equivalent model.
It outputs four denoised CFA planes. The existing exact MHC demosaic remains
unchanged. Direct RGB output (phase 2b) is not implemented.

The Rust implementation currently uses host tensors. ORT partitions convolutions
into CoreML and leaves Resize/Slice on CPU on the tested Mac. CoreML's dynamic
shape compilation also emits an unbounded-dimension diagnostic. A CoreML node
assignment is NOT evidence of a GPU-resident end-to-end path. The requested
resident CFA-to-demosaic handoff is **not implemented**, so M3-16 is not fully
accepted even if all CPU/reference gates pass. This needs a resident-buffer
contract across the runtime and GPU backend rather than pretending host copies
are resident. No engine-api changes were made.

## Code and data provenance

`tools/train_cfa_denoise.py`, `tools/export_cfa_denoise.py`, and their Python tests
are original MIT code (full license in `tools/orchestrate/wp/M3-16/LICENSE.training`).
No pretrained weights or third-party training implementation are incorporated.
PyTorch is BSD-style, NumPy BSD, ONNX Apache-2.0, onnxconverter-common MIT, rawpy
MIT. rawpy's underlying LibRaw decoder is LGPL/CDDL, as is the project's existing
raw decoder dependency; it is not vendored GPL training code. This is not a claim
that every transitive binary dependency is permissively licensed.

The existing `fixtures/fetch.sh` identifies the five samples as CC0 raw.pixls.us
entries: Canon EOS M50 (2663), Sony NEX-6 (782), Nikon D800 (750), Fujifilm X-E2S
(745), and Leica M9 (752). They are third-party CC0 samples, not photographs we
claim to have taken. Source bytes are SHA-256 recorded in `training.json`.
The X-Trans RAF is explicitly skipped by this Bayer model. Rust routes X-Trans
through the existing bounded-linear-sRGB DRUNet adapter after demosaic.

Additional raw files in `fixtures/raw` are discovered automatically. Unknown
filenames require `--allow-user-raws`, an explicit assertion that they are owned
or permissively licensed. Do not publish weights trained on unlicensed images.
The code supports CR3, CR2, ARW, NEF, RAF, DNG, and RW2 via rawpy.

## Training method

* Read visible CFA without demosaic or white balance. Subtract each site's black
  level and divide by its white-minus-black range. Rotate Bayer patterns to RGGB
  and pack R/G1/G2/B separately. Rust uses the same counterclockwise rotations
  and supports odd sensor extents with same-phase padding and exact unpacking.
* Sample 32x32 packed crops from disjoint top/bottom image regions, separated by
  a guard band. Bottom-region crops are held out. Noise replicas of a crop never
  cross the split. Reject clipped and negative training crops. Originals are
  **noisy proxies**, not clean ground truth. The small fixture set is not an
  ISO-calibrated low-ISO corpus; no claim of laboratory-clean targets is made.
* Select low-texture patches by coarse block-mean spread. Fit per-site
  `variance = shot * mean + read` by least squares. Coefficients are nonnegative;
  insufficient intensity range is reported, not replaced with an invented ISO
  calibration. Texture biases these estimates upward. The test recovers known
  Poisson-Gaussian parameters within 10% on genuinely flat synthetic patches.
* Inject actual Poisson counts plus independent Gaussian read noise, without
  clipping. Half of real-data training steps use fitted camera profile shapes
  with sampled noise strength; alternate steps retain a known reference severity.
  A quarter of steps use adjacent, same-colour subimage pairs from original
  sensor samples (Neighbor2Neighbor-style self-supervision). This is a basic
  adjacent-pair loss, not a reproduction of the paper's full regularized method.
* A one-level residual U-Net uses 12/24 features, 3x3 convolutions, stride-two
  downsampling and nearest-neighbour upsampling. Eight input channels contain
  four CFA planes and four sigma maps derived from the noisy signal. Output is
  four linear CFA planes. No global pooling, attention, or batch normalization.
  Conservative halo is 16 packed pixels, alignment 2. Amount is only a blend,
  never a change in the conditioning noise calibration.
* Adam, 1200 steps, batches of eight, MPS by default (CPU fallback). Procedural
  smoke training uses 300 steps and a separate fixed seed for held-out fields
  containing both smooth structure and hard edges. Its >=3 dB gate is not evidence
  of general camera quality. Use a larger, independently captured test set before
  enabling this by default.

## Reproduction

From the worktree root, preserve the externally exported CARGO_TARGET_DIR.
Weights, environments, and transient reports stay in the ignored artifacts tree.

    python3 -m venv tools/orchestrate/wp/M3-16/.venv
    tools/orchestrate/wp/M3-16/.venv/bin/pip install -r tools/orchestrate/wp/M3-16/requirements.txt
    export PYTHONDONTWRITEBYTECODE=1
    tools/orchestrate/wp/M3-16/.venv/bin/python tools/train_cfa_denoise.py --output tools/orchestrate/wp/M3-16/artifacts
    tools/orchestrate/wp/M3-16/.venv/bin/python tools/export_cfa_denoise.py tools/orchestrate/wp/M3-16/artifacts/cfa.pt --output tools/orchestrate/wp/M3-16/artifacts --manifest crates/ml-runtime/models.toml

Export pins opset 17 and uses the legacy TorchScript exporter deliberately;
its deprecation warning is expected. Both fp32 and fp16-weight variants retain
fp32 IO. ONNX checker validates both. The exporter updates only its marked
section in models.toml, using relative local paths and actual SHA-256 hashes as
both version and digest. MPS training is not promised bit-reproducible across
machines; re-export registers the newly produced digest, never a fake fixed hash.
Missing local weights fail closed, with no network fallback. Weights are not
source-controlled. A clean checkout must train/export before resolving these
production-local entries.

## Integration

Enable image-core's `ml-denoise` feature. Construct `MlCfaDenoise` with a registry,
SessionOptions, a generated ModelRef and measured CfaNoise in canonical RGGB
order. Inject it using the existing `with_post_demosaic_denoise` method (the
legacy trait/method name is retained). Select Neural with the CFA model ID,
its exact digest version and `joint_demosaic=false` for phase 2a.

Bayer dispatches to `denoise_raw` after highlight reconstruction, before MHC.
X-Trans dispatches to the pinned RGB fallback. Missing backends/models error,
rather than silently applying an unrelated denoiser. The CPU full renderer uses
the same raw barrier. The tiled renderer caches Denoise as F32 sensor tiles;
model, Amount, calibration, mask extent/samples and adapter revision enter its
cache key. Demosaic edits reuse CFA output. Tone/WB edits reuse downstream caches.

`with_mask(width, height, samples)` takes an M2-08 raster already sampled at
full-sensor pixel centres, before crop/warp. Packing uses the exact same rotation
for the mask; no 2x2 averaging. This does not add recipe mask persistence or a new
mask rasterizer. Zero amount and unselected CFA sites preserve source bits.
Inference uses halo tiles; no blending of overlapping model outputs is needed.

## Tests and evidence

    cargo test -p ml-enhance -p pipeline-cpu -p image-core --release
    cargo clippy -p ml-enhance -p pipeline-cpu -p image-core --all-targets -- -D warnings
    cargo fmt --check
    cargo test -p ml-runtime --test registry_validation --release
    cargo test -p image-core --features ml-denoise --test ml_cfa --test cfa_denoise --release
    cargo test -p image-core --features ml-denoise --test ml_cfa_local --release -- --ignored
    cargo clippy -p image-core --features ml-denoise --all-targets -- -D warnings

`cfa_model` trains a fresh tiny CPU checkpoint during the normal cargo suite,
exports both variants, measures ONNX PSNR over 24 held-out crops, checks tiled
against whole inference, and obtains executed-provider reports from ml-runtime.
It requires the Python environment above (override with TESSERA_TRAIN_PYTHON).
It errors instead of silently skipping when dependencies are absent. The optional
local-adapter test uses the real fixture-trained checkpoint, exercising all four
Bayer patterns, odd sensor extents and exact unselected mask sites.

Recorded results and remaining acceptance gaps are in
`tools/orchestrate/wp/M3-16/VERIFICATION.md`. Off-path goldens are not regenerated.
