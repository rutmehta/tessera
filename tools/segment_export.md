# MobileSAM ONNX export recipe

M3-04 consumes pinned upstream **encoder + decoder ONNX exports**, not fabricated
or locally regenerated weights. The production download/verification script is
`tools/segment_models.py`; exact hashes and immutable URLs are registered in
`crates/ml-runtime/models.toml`. It performs ONNX graph checks plus real CPU
inference when invoked with `verify --smoke`.

The exporter is MIT-licensed `vietanhdev/samexporter`. Its documented export
entrypoints, inspected at revision `35133ce8670e0d190ac10cc08efba9b9a443fb51`, are:

- `samexporter/export_encoder.py` (`python -m samexporter.export_encoder`)
- `samexporter/export_decoder.py` (`python -m samexporter.export_decoder`)
- `convert_mobile_sam.sh`

Source: https://github.com/vietanhdev/samexporter/tree/35133ce8670e0d190ac10cc08efba9b9a443fb51

For a future re-export, in a separate Python 3.11+ environment with the pinned
exporter and its dependencies installed, use the Apache-2.0 MobileSAM checkpoint
from upstream revision `f706ad9c4eb7f219c00d9050e46328518ffb65d2`:

```sh
python -m samexporter.export_encoder \
  --checkpoint original_models/mobile_sam.pt \
  --output output_models/mobile_sam.encoder.onnx \
  --model-type mobile --use-preprocess
python -m samexporter.export_decoder \
  --checkpoint original_models/mobile_sam.pt \
  --output output_models/mobile_sam.decoder.onnx \
  --model-type mobile --return-single-mask
```

Do not enable quantization without a new registry version and accuracy test.
Do not assume these commands reproduce the published bytes: exporter/PyTorch/
ONNX versions and simplification affect the graph. This work package verified
the published artifacts, not a fresh PyTorch export. Before promoting a new
export: compute actual SHA-256, record checkpoint/exporter/environment provenance,
check all input/output names against the README contract, test portrait and
landscape images, positive/negative clicks and boxes, then run the Rust synthetic
IoU and provider profiling tests. Never overwrite the existing pinned version
with a different hash and never commit weights.

Runtime preprocessing is HWC RGB 0..255 with longest-edge resize to 1024. The
encoder embeds normalization/padding, not resize. The decoder's `masks` are
logits. The Rust implementation requests a bounded encoder-resolution mask and
then refines it to the requested pyramid level. See the crate README for the
complete tensor contract and model-specific Apache-2.0 license links.
