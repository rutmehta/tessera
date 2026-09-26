# ml-caption

Local SigLIP keyword suggestions, Florence-2-base-ft captions/alt text and OCR.
No cloud calls, Python processes, remote model code, or downloaded weights in the
repository. `engine-api` is unchanged.

## Use

Install the pinned model manifest as `APP_DIR/models.toml`, then explicitly fetch:

```
python3 tools/fetch_siglip.py --cache APP_DIR/models
python3 tools/fetch_florence.py --cache APP_DIR/models
cargo run --release -p tessera-cli -- --app-dir APP_DIR ml keywords image.jpg
cargo run --release -p tessera-cli -- --app-dir APP_DIR ml caption image.jpg
cargo run --release -p tessera-cli -- --app-dir APP_DIR ml ocr image.jpg
```

Each inference command accepts `--manifest PATH`, `--cache DIR`, `--store`,
`--coreml`, and `--partition-report`. No model download is implicit in the CLI.
`--store` requires an already-indexed image and makes generated captions/OCR
searchable through `tessera ls --query TEXT`; it does not overwrite human captions.
Running individual commands retains other task outputs and their provenance.
The library loaders explicitly resolve models and may download on cache miss.

`ml keywords` supports `--top N`, `--vocabulary FILE`, `--temperature FLOAT`,
`--midpoint FLOAT`, `--accept`, and `--write-xmp` (requires `--accept`). The bundled
vocabulary contains 2,080 distinct original Apache-2.0 photographic concepts.
Scores are independent sigmoid((cosine - midpoint)/temperature), NOT a softmax
across mutually exclusive labels. The defaults are heuristic, not probabilities
validated on a held-out photographic corpus. Change temperature/midpoint for
user-specific validation data; the vocabulary hash, prompt version and calibration
are included in provenance. Text features are computed once per loaded instance.

`KeywordModel::suggest_keywords` and `suggest_batch` run the actual pinned SigLIP
image/text towers. `Florence::caption` generates one short sentence and separate
more detailed alt text. `Florence::ocr` generates text plus four-corner location
tokens. OCR returns normalized displayed-image bounding boxes. Its confidence is
the geometric mean of generated token probabilities for the whole sequence,
shared across regions, not a calibrated per-region detection probability.
Malformed, incomplete or over-length outputs are errors, never invented results.
Generation uses deterministic full-prefix greedy decoding with forced BOS/EOS
termination, capped at 96/192/256 new tokens for caption/alt/OCR. It intentionally
does not claim parity with upstream three-beam decoding. Dense documents exceeding
the cap return an error. No KV cache: correctness-first, relatively slow on CPU.

## Mapping and writes

`map_keyword(&Library, label)` is read-only: normalized exact name first, then
synonym, ambiguity errors rather than guesses. Unmatched concepts are proposed
under `Suggested`. `accept_keywords` is a separate explicit bulk operation.
`WritePolicy::IndexOnly` is default. The caller saves the mutated library with
`Library::write`; the CLI does this. `WritePolicy::Xmp` atomically writes only XMP
through `sidecar`, merges keywords/hierarchical paths, and preserves other XMP
metadata, selection and unknown XML. Originals are never written. File, library
and database writes cannot form a distributed transaction: failures propagate;
retry is idempotent. All requested images and XMP packets are preflighted first.

Index schema v6 stores model suggestions/provenance independently from accepted
keywords. FTS combines imported captions with generated captions and OCR. Rescans
rebuild FTS without losing model output. Explicitly accepted local tags survive
sidecar rescans even when XMP export was not requested. Prune/deletion cascades
model data and local acceptance rows. Suggestions are never silently accepted.

`UnderstandingJob` implements the existing `Job` trait at `Priority::Score`.
Submit it to `jobs::ThreadPoolScheduler` with explicit indexed image IDs and an
already-loaded `UnderstandingModel`; construction does no I/O. Four-image batches
bound preview memory; SigLIP runs batched, Florence decodes sequentially inside
the batch. The job yields to interactive pressure, checks cancellation around
decode/inference/writes, validates result cardinality and version, and skips
already-persisted matching versions when retried. It never accepts or writes XMP.
The engine does not automatically schedule it; callers opt into scheduling.

## Weights, licensing and ONNX export contract

SigLIP base-patch16-224: Google Apache-2.0 weights, Xenova ONNX export; existing
`ml-embed` hashes and tokenizer contract are reused unchanged.

Florence: Microsoft `Florence-2-base-ft`, MIT; we consume the already-exported
`onnx-community/Florence-2-base-ft` MIT artifacts at immutable revision
`e88a44eaf3791a35eae0c5a47b3dbcd36e67eb6f`. The upstream model card and configuration:

- https://huggingface.co/onnx-community/Florence-2-base-ft/tree/e88a44eaf3791a35eae0c5a47b3dbcd36e67eb6f
- https://huggingface.co/microsoft/Florence-2-base-ft

This work did not re-export/train weights. The consumed export is split into four
standalone ONNX graphs (no external-data files). ONNX metadata identifies PyTorch
2.3.0 for vision/token embedding and 2.3.1 for encoder/decoder; opsets are 13 for
vision and 14 for the others. Weights are fp16, graph boundary tensors fp32/int64.
This is the export contract, inspected from the actual downloaded graphs:

| File | Inputs | Required output |
|---|---|---|
| vision_encoder_fp16.onnx | pixel_values [B,3,768,768] | image_features [B,577,768] |
| embed_tokens_fp16.onnx | input_ids int64 [B,S] | inputs_embeds [B,S,768] |
| encoder_model_fp16.onnx | attention_mask int64 [B,E], inputs_embeds [B,E,768] | last_hidden_state [B,E,768] |
| decoder_model_fp16.onnx | encoder_attention_mask int64 [B,E], encoder_hidden_states [B,E,768], inputs_embeds [B,D,768] | logits [B,D,51289] plus 24 present-cache outputs |

Image input is RGB, bicubic resize without crop to 768 square, /255, ImageNet
mean [0.485,0.456,0.406] and std [0.229,0.224,0.225], planar NCHW. Vision features
are prepended to prompt token embeddings. Task tokens are expanded to the exact
English prompts from pinned `preprocessor_config.json`. Tokenizer JSON is pinned
and its embedded BART normalization/BOS/EOS behavior is preserved. The decoder
starts [2,0], terminates on 2, and consumes the complete generated prefix.

Hashes below were checked against downloaded bytes by `tools/fetch_florence.py`:

| Artifact | Bytes | SHA-256 |
|---|---:|---|
| vision_encoder_fp16.onnx | 183930536 | a7abcd77199c5d0089cf985ede4dd8089acd84f30fb3fb1462d5930345c688b3 |
| embed_tokens_fp16.onnx | 78780290 | da2607930eea5e21e4a2bd5fd069de550f1acc30316a4e8f824551a95232ba39 |
| encoder_model_fp16.onnx | 86747414 | 0d1d929f282963e983b8ac5ac4957f19a8fa48233eab41166951b769e5cf2fd2 |
| decoder_model_fp16.onnx | 194198478 | ce583853b630f230eaa1ef201e35001cdda968c84749d67c18ce3707171cfa0c |
| tokenizer.json | 2297961 | d69dcdb2323e124ac4f800cb9863ddccea0d7bb11e16125e8df3bd60f2f8aeac |

The registry pins each graph's ID, revision, URL and hash. The loader rejects
custom manifests whose hashes differ from this preprocessing/export contract.
Any future re-export requires new hashes, version, shape/tokenization validation,
CPU/cached integration tests and a fresh CoreML partition audit.

Bundled test font: Roboto Regular, Apache-2.0, Google `googlefonts/roboto` revision
`38062f4b4a0be4346d07a928408da21602545e9e`, `src/hinted/Roboto-Regular.ttf`.
The complete upstream license is in `tests/data/LICENSE-Roboto`.

## Verification

```
cargo test -p ml-caption -p index -p tessera-cli --release
cargo clippy -p ml-caption -p tessera-cli --all-targets -- -D warnings
cargo fmt --check
```

Cached integration tests use `TESSERA_FLORENCE_CACHE` / `TESSERA_SIGLIP_CACHE`, or
`tools/orchestrate/wp/M3-13/cache` by default. Missing artifacts print `SKIP offline`
and return without network access; corrupted cached artifacts fail. To see skips,
use `-- --nocapture`. Both real model tests were exercised during implementation,
not just skipped. `TESSERA_CAPTION_COREML=1` opts into the hardware audit.
See `tools/orchestrate/wp/M3-13/COREML.md` for measured partitions and limitations.
