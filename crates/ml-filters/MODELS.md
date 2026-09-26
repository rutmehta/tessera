# Neural filter model provenance and contracts

## Status

- **Registered:** `filters/ddcolor`, DDColor **paper-tiny**, Edge Tools fp16
  export with fp32 I/O. Apache-2.0 publisher declaration and upstream evidence.
- **Excluded by round-2 scope, not registered:** GFPGAN v1.4 due to its
  StyleGAN2/NVIDIA non-commercial lineage. The reviewed ONNX publisher explicitly
  declares `license: other`; upstream includes non-allowlisted third-party terms.
  A generic Apache-2.0 repository tag is insufficient to establish the requested
  Apache/MIT/BSD-only scope. No dummy GFPGAN entry or invented digest is present.

This document records acquisition and provenance. The implemented adapter
contracts and current limitations are in README.md; executed Rust quality and
CoreML measurements are in `tools/orchestrate/wp/M3-21/RESULTS.md`.
The existing Remove conventions in `../filters/training/README.md` were read
but not modified.

## DDColor paper-tiny

| Field | Verified value |
| --- | --- |
| Registry ID | `filters/ddcolor` |
| Publisher | `edgetools/ddcolor` |
| Revision | `4755ae9f1f7a35a9e7693b96c2a88f3432cb6ab0` |
| File | `ddcolor-tiny-fp16.onnx` |
| SHA-256 | `2653da00dc15e54a45e5200b61dbf82ee9ceaf56b02bb9b9657569ac775e82e6` |
| Bytes | 135444402 |
| Graph | ONNX opset 17; fp16 weights, fp32 boundary |
| Input | `input`: float32 `[1,3,512,512]` |
| Output | `output`: float32 `[1,2,512,512]`, Lab **a,b**, not RGB |

### Immutable evidence

- [Export model card and Apache-2.0 declaration](https://huggingface.co/edgetools/ddcolor/blob/4755ae9f1f7a35a9e7693b96c2a88f3432cb6ab0/README.md)
- [Download](https://huggingface.co/edgetools/ddcolor/resolve/4755ae9f1f7a35a9e7693b96c2a88f3432cb6ab0/ddcolor-tiny-fp16.onnx)
- [Authoritative Git LFS pointer](https://huggingface.co/edgetools/ddcolor/raw/4755ae9f1f7a35a9e7693b96c2a88f3432cb6ab0/ddcolor-tiny-fp16.onnx)
- [Upstream architecture license](https://github.com/piddnad/DDColor/blob/2adb63f2656ac41cbdf7b894cddd94121a3faf13/LICENSE)
- [Upstream checkpoint repository](https://huggingface.co/piddnad/ddcolor_paper_tiny/tree/cf9fd99c1d7472689ec7413441c1b799a51866a3):
  `pytorch_model.bin` LFS SHA-256
  `8a1277bc90a1bfbb6d2d83933a9a6bc821931879ca93e26e4fcec12165d41fce`,
  also identified by the export publisher. This is **not** the ONNX hash.

The publisher identifies `convnext-t`, `MultiScaleColorDecoder`, two output
channels, Spectral last norm, 100 queries, three scales, nine decoder layers,
upstream revision `2adb63f2656ac41cbdf7b894cddd94121a3faf13`, opset 17,
and `keep_io_types=True` float16 conversion. No artistic variant is substituted.
This is publisher provenance evidence, not an independent audit of dataset rights.
Preserve Apache-2.0 license and applicable attribution notices when redistributing.

### Image contract

1. Operate on **display-sRGB**, not linear RGB. Convert finite normalized RGB
   `[0,1]` to CIE Lab using OpenCV float conventions: D65, L `[0,100]`, a/b in
   approximately `[-127,127]`, not byte-offset Lab.
2. Save full-resolution source L. Resize source to 512 square, obtain its L,
   set a=b=0, convert Lab back to RGB `[0,1]`, and pack float32 NCHW.
   Do not add external ImageNet normalization.
3. Run `input` -> `output`; output planes are a and b, not normalized RGB.
4. Bilinearly resize a/b to the original size, combine with **original L**,
   convert Lab to display-RGB, gamut-clamp, then decode to linear light if needed
   by the compositor. Preserve original alpha outside the neural graph.

### Executed verification

Complete downloaded bytes match the LFS hash and size. ONNX 1.19.0 checker
passes; names, types, dimensions, and opset above were read from the graph.
ONNX Runtime 1.22.1 CPU execution with two intra-op threads on an RGB grayscale
horizontal `[0,1]` ramp returned finite float32 `[1,2,512,512]`, range
`[-3.8998513,75.15192]`. One measured inference was 4.279 s; this is a smoke test,
not a performance or quality acceptance claim. No CoreML claim is made.

## GFPGAN v1.4: excluded from M3-21

The coordinator's round-2 decision excludes GFPGAN from this package. Restoration
is denoise-only and not listed in the shipped catalog. The inspection evidence
and guarded exporter below are historical research, not a path to activation
under the current scope. No approval or license workaround is implied.

The following artifact was downloaded **for inspection only** and is not an
approved runtime dependency:

| Field | Inspected candidate |
| --- | --- |
| Publisher | `HowToSD/GFPGAN-ONNX` |
| Revision | `ae7b761a0dd6da7ccd7d1f02cc584a2f1df39620` |
| File | `GFPGANv1.4.onnx` |
| SHA-256 | `15c36ba1a8304e077a9d99954427eac3fb604bb790636dd5e34e72cdeb0830d8` |
| Bytes | 340357025 |
| Graph | opset 16; float32; ONNX checker passed |
| Input | `input`: float32 `[batch,3,512,512]` |
| Output | `output`: float32 `[batch,3,512,512]` |

The full downloaded hash matches authoritative Hub LFS metadata. This hash is
real but **does not imply license approval**. No GFPGAN inference was performed.

- [Pinned candidate card](https://huggingface.co/HowToSD/GFPGAN-ONNX/blob/ae7b761a0dd6da7ccd7d1f02cc584a2f1df39620/README.md)
  says `license: other`, identifies the official v1.4 checkpoint, and explicitly
  notes third-party conditions and potential commercial-use limitations.
- [Pinned upstream license](https://github.com/TencentARC/GFPGAN/blob/7552a7791caad982045a7bbe5634bbf1cd5c8679/LICENSE)
  says Apache-2.0 **except** third-party components; the bundled terms include
  NVIDIA noncommercial, DFDNet CC-BY-NC-SA, and MPL text. Their applicability to
  the exact clean graph/weights needs a scoped review, not automatic rejection
  of every GFPGAN use and not blanket Apache approval either.
- [Upstream inference contract](https://github.com/TencentARC/GFPGAN/blob/7552a7791caad982045a7bbe5634bbf1cd5c8679/gfpgan/utils.py):
  align each face with five landmarks to 512 square; RGB NCHW, normalize display
  RGB by `(x - 0.5) / 0.5` to `[-1,1]`. Output RGB uses the same nominal range;
  clamp to `[-1,1]`, then `(y+1)/2`. Inverse-warp and blend into the source image.
  Face detection, alignment, masking, and paste-back are **not** this model.
- [Clean architecture](https://github.com/TencentARC/GFPGAN/blob/7552a7791caad982045a7bbe5634bbf1cd5c8679/gfpgan/archs/gfpganv1_clean_arch.py):
  `GFPGANv1Clean`, out_size=512, channel_multiplier=2, input_is_latent=True,
  different_w=True, sft_half=True. The fallback exporter disables randomized
  noise and returns only the restored image, with names `input`/`output` and
  fixed `[1,3,512,512]`; these are the **planned local export** contract, not a
  claim to have generated a second ONNX file.

### Export fallback, not a licensing workaround

`training/export_gfpgan.py` provides the local export path requested when a
pre-export cannot be approved. It refuses to run before a documented review,
checks the clean upstream revision and checkpoint hash, loads `params_ema`
strictly, exports opset 17, checks ONNX, compares CPU ORT with PyTorch, and only
then publishes a local ONNX and provenance JSON. It never edits the registry.
The checkpoint digest is independently sourced from the [pinned mirror LFS metadata](https://huggingface.co/aimi-models/editor-tools/raw/042586f69a17179a23617d8cdf92400083b726cf/face-restore/GFPGANv1.4.pth):
`e2cd4703ab14f4d01fd1383a8a8b266f9a5833dacee8e6a79d3bf21a1b6be5ad`.
Mirror metadata does not establish additional license rights.

**Only the fail-closed CLI guard has been executed/tested.** Full export and
PyTorch parity remain blocked and unvalidated; the export environment and
GFPGAN/BasicSR/torchvision compatibility need to be pinned and tested after
license clearance. Re-exporting does not remove upstream restrictions. Approval
must resolve the exact weights and graph's third-party scope before a production
entry can be added. Never reuse the inspected candidate's hash for a new export.
