# Segmentation model contracts

## Reproducible acquisition (no local export)

`python3 tools/segment_models.py fetch` downloads the three immutable ONNX artifacts
below into ignored `tools/orchestrate/wp/M3-04/.cache/segment-models/`, verifies SHA-256 before atomic promotion,
and refuses corrupt existing files. No weights are committed or bundled.
Python 3.11+ is required. Optional graph/inference verification:

```sh
python3 -m venv tools/orchestrate/wp/M3-04/.cache/segment-venv
tools/orchestrate/wp/M3-04/.cache/segment-venv/bin/python -m pip install --only-binary=:all: onnx==1.19.1 onnxruntime==1.23.2 numpy==2.2.6
tools/orchestrate/wp/M3-04/.cache/segment-venv/bin/python tools/segment_models.py verify --smoke
```

These are downloaded upstream exports, **not claimed locally reproduced exports**.
Artifact bytes were independently downloaded and hashed, graphs checked with ONNX,
and contracts read from ONNX Runtime. See `models.toml` for exact URLs and hashes.
The manifest uses concrete representative shapes because its dimensions are usize;
dynamic dimensions below are not fixed graph constraints.

## U²-Net subject: `segment/u2net`

- Source: `Heliosoph/u2net-onnx`, revision `7fc34deee10329bc039c10a73b98090d0c6f5c59`, `u2net.onnx`.
- SHA-256 `8d10d2f3bb75ae3b6d527c77944fc5e7dcd94b29809d47a739a7a728a912b491`, 175997641 bytes, opset 11.
- Input **`input.1`**, float32 **[1,3,320,320]**, RGB/NCHW.
- Resize RGB to 320×320 (bilinear; deliberate aspect distortion), divide by 255,
  then channelwise `(x - [0.485,0.456,0.406]) / [0.229,0.224,0.225]`.
- Outputs **`1959`, `1960`, `1961`, `1962`, `1963`, `1964`, `1965`**,
  each float32 **[1,1,320,320]**. Use **1959**, the fused sigmoid saliency map.
  Do not apply sigmoid twice. Normalize `(v-min)/(max-min)` with a zero-range guard
  if matching rembg-style contrast stretching; bilinear-resize to original dimensions.
  These names come from the graph, not the model card's incorrect `d0..d6` names.
- License: Apache-2.0 model card and bundled license at
  https://huggingface.co/Heliosoph/u2net-onnx/blob/7fc34deee10329bc039c10a73b98090d0c6f5c59/README.md
  and https://huggingface.co/Heliosoph/u2net-onnx/blob/7fc34deee10329bc039c10a73b98090d0c6f5c59/LICENSE.md .
  The publisher identifies xuebinqin/U-2-Net via danielgatis/rembg as provenance.
  This is general saliency, not the excluded APDrawing portrait variant.

## MobileSAM: `segment/sam-encoder` + `segment/sam-decoder`

Source: `nrl-ai/samexporter-onnx-models`, revision
`5050a79cd4b912dd745fff83047c4ef6fbd97be5`, `mobile_sam/`, opset 18.

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| mobile_sam.encoder.onnx | 28157853 | `8c1494f7dc70b61b15bc7ab8e4291804d47294d881da137b099b04c1776535dd` |
| mobile_sam.decoder.onnx | 16502895 | `23c11087a0c1930d863ba37fdf0f2b0080f5a94269191f679ffd16b8415f0a38` |

### Encoder

Input **`input_image`**, float32 **[resized_H,resized_W,3]**, HWC RGB in **0..255**,
**no batch dimension**. Resize longest edge to 1024, preserving aspect:
`scale=1024/max(H,W)`, `new_H=floor(H*scale+0.5)`, likewise W.
Resize on the caller side: the embedded preprocessing **does not resize**.
The graph subtracts `[123.675,116.28,103.53]`, divides by `[58.395,57.12,57.375]`,
transposes, pads normalized values with zero on bottom/right to 1024 square,
and adds batch dimension. Do not normalize/pad a second time.
Output **`image_embeddings`**, float32 **[1,256,64,64]**; cache per source image.

### Decoder (all inputs/outputs float32)

| Name | Shape | Meaning |
|---|---|---|
| image_embeddings | [1,256,64,64] | Encoder output |
| point_coords | [1,N,2] | (x,y) in resized image coordinates |
| point_labels | [1,N] | 1 foreground, 0 background, -1 padding; 2/3 box corners |
| mask_input | [1,1,256,256] | Zeros for first pass; prior low_res_masks for refinement |
| has_mask_input | [1] | [0] initially, [1] with prior logits |
| orig_im_size | [2] | Original [H,W], before resize |
| masks | [1,1,H,W] | Full-resolution **logits**, threshold >0 for binary mask |
| iou_predictions | [1,1] | Predicted quality of selected mask |
| low_res_masks | [1,1,256,256] | Low-resolution logits for refinement |

Scale point x by new_W/original_W and y by new_H/original_H. For point-only
prompts append one dummy `(0,0)` point with label -1. For a box use top-left
label 2 and bottom-right label 3, without dummy point. The exported decoder
selects a single mask internally; it is not a three-mask output. `N` is dynamic.
Decoder already upsamples/crops padding/restores original dimensions. If a soft
mask is required, apply sigmoid to logits rather than clamping logits to [0,1].

### License and source evidence

The repository-wide metadata says `other` because it also hosts **SAM3**;
we fetch **only MobileSAM**, whose family license and model table say Apache-2.0:

- https://huggingface.co/nrl-ai/samexporter-onnx-models/blob/5050a79cd4b912dd745fff83047c4ef6fbd97be5/mobile_sam/LICENSE
- https://huggingface.co/nrl-ai/samexporter-onnx-models/blob/5050a79cd4b912dd745fff83047c4ef6fbd97be5/README.md
- https://huggingface.co/nrl-ai/samexporter-onnx-models/blob/5050a79cd4b912dd745fff83047c4ef6fbd97be5/PROVENANCE.md

Publisher records MobileSAM upstream revision `f706ad9c4eb7f219c00d9050e46328518ffb65d2`.
Source preprocessing reference (reviewed, not asserted to be the exact historical
exporter revision):
https://github.com/vietanhdev/samexporter/blob/35133ce8670e0d190ac10cc08efba9b9a443fb51/samexporter/onnx_utils.py .

Preserve upstream copyright/license notices with redistributed weights; these
are optional downloads. This follows `docs/13-licensing.md`'s permissive-license
policy, but publisher statements are not a separate legal audit of training data.
No SAM3, GPL/AGPL model, or portrait drawing weights are included.

## API and integration

`Segmenter::load(registry, options, MaskStore)` verifies the pinned model refs
and expected hashes before loading sessions. This explicit call may download.
Inference never performs network I/O. Inputs are already oriented sRGB `RgbImage`
previews. `subject(image, level)`, `sky`, `background`, `person(image, face_box,
level)` and `promptable(image, prompts, level)` return validated single-channel
f32 `MaskRaster`s. Level 0 is the input extent, level n is floor(width/2^n) by
floor(height/2^n), each clamped to one. Levels 0..=8 are supported.

`Prompts` supports normalized positive/negative clicks and boxes [left,top,right,
bottom]. Multiple boxes are decoded independently and unioned. `person` takes
the face crate's pixel [x,y,width,height] box, expands horizontally to 3 face
widths and downward to 7 face heights, clips to the image and supplies a SAM box.
This is a whole-person approximation, not part parsing or identity recognition.
Text prompts, body/face parts, and non-sky landscape classes are phase 2. There
is deliberately no text/parts API that silently substitutes a whole-body mask.
No engine-api types or renderer behavior are modified.

`MaskStore` in previews is a separate lossless, checksummed f32 disk cache (never
JPEG). Writes are atomic and have a separate byte budget with oldest-write
eviction. Corrupt/missing entries are misses. Keys include oriented RGB content
and dimensions, model/algorithm identity and version, prompt geometry/signs and
pyramid level, but no local-adjustment sliders. U2Net's native raster is also
cached across level requests. SAM retains one image embedding in memory. Supply
an application-owned `.edits/<image>/masks/` or preview mask-cache directory.

Refinement ports the private M2-08 guided-filter equations without changing the
out-of-scope pipeline crate. It uses f64 integral moments, truncated windows,
linear-sRGB luminance, radius 8 and epsilon 1e-4. Upsampling starts with nearest
samples to avoid pre-mixing across boundaries; the guide smooths the blocks.
Downsampling uses triangle filtering. Background is inverted *after* subject
refinement so the two masks sum exactly to one.

`PrecomputeJob` is a `Job` at `Priority::Score` for one oriented preview. It
precomputes subject then sky, reports progress and cooperatively checks
cancellation before work and cache publication. A running ORT call is not
interruptible. Previously completed cache entries remain valid on cancellation.

## Licensing fallbacks

The NVIDIA ADE20K SegFormer-B0 model card explicitly links to the NVIDIA
SegFormer license. Section 3.3 restricts use to non-commercial research or
evaluation, so it is excluded, not mislabeled Apache:
https://huggingface.co/nvidia/segformer-b0-finetuned-ade-512-512/blob/main/README.md
https://github.com/NVlabs/SegFormer/blob/master/LICENSE

Sky therefore uses the permitted fallback: top-connected blue excess prior
times (1 - U2Net subject). This is not semantic recognition. It misses gray skies
and sunsets and may include top-connected blue architecture or water. It is
versioned separately as `sky-blue-connected-v1` in the cache.

Self-Correction-Human-Parsing has MIT source, but the reviewed README distributes
external LIP/ATR/Pascal checkpoints without a separate verified weight-license
grant/provenance. We did not infer redistribution rights solely from a source
license. Part masks remain phase 2 pending a checkpoint-specific license audit
and ONNX/accuracy validation:
https://github.com/GoGoDuck912/Self-Correction-Human-Parsing/blob/master/LICENSE
https://github.com/GoGoDuck912/Self-Correction-Human-Parsing/blob/master/README.md

## Verification

Install downloaded models into the Rust SHA-named cache:

```sh
python3 tools/segment_models.py fetch --registry-cache tools/orchestrate/wp/M3-04/.cache/segment-registry
cargo test -p ml-segment --release -- --nocapture
cargo clippy -p ml-segment --all-targets -- -D warnings
cargo fmt --check
```

Keep `CARGO_TARGET_DIR` outside the repository. The model test uses the above
cache by default or `TESSERA_SEGMENT_MODELS`. It skips when absent, fails on a
partial/corrupt installed cache, and never fetches in tests. Offline tests cover
refinement, cache losslessness/corruption/budget/identity, prompt validation,
sky connectivity, person expansion, job priority and cancellation. Cached-model
tests execute actual U2Net and MobileSAM on synthetic images plus the Sony ARW
fixture preview. The disc IoU gate is >0.8. CPU fallback is real, not a heuristic
subject substitute. `partition_reports()` returns actual ORT provider assignments.

Observed on this Apple Silicon run: U2Net had one fused CoreML node (all executed
nodes), MobileSAM encoder used CPU (645 nodes), and decoder had 22 CoreML nodes
out of 93. MLProgram dynamic-shape compilation errors cause the encoder fallback;
we do not claim full SAM acceleration. Synthetic disc IoU was 1.0. The blue-sky
top mean was 0.99685 and bottom mean 0.000078. These are synthetic checks, not a
photographic benchmark. See `tools/segment_export.md` for the inspected upstream
export scripts and re-export procedure; no fresh local PyTorch export is claimed.
