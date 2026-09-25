# Local SigLIP embeddings

`Siglip` produces L2-normalized `[f32; 768]` image/text vectors. RGB previews are
resized to 224×224 with Catmull–Rom bicubic filtering, without cropping, then
normalized with mean/std 0.5. The matching SentencePiece tokenizer JSON handles
normalization and EOS; queries are lowercased, truncated and right-padded to 64
with EOS id 1. There is no attention-mask input in this export. Text is used
verbatim, without an implicit prompt template.

## Weights, provenance and installation

The weights originate from Google's Apache-2.0
[SigLIP base patch16-224](https://huggingface.co/google/siglip-base-patch16-224).
The [Xenova export](https://huggingface.co/Xenova/siglip-base-patch16-224/tree/4649052661e53c7000355844105f8a1792088239)
identifies the Google checkpoint as its source and converts it to ONNX; its
model card does not declare a separate license. The original Apache-2.0 license
continues to apply to these converted weights. No restricted CLIP weights are
used. The model card and original license provenance should accompany any
redistribution. Weights are not committed.

Both towers are registered in `crates/ml-runtime/models.toml` with immutable
revision URLs, byte sizes (comments) and SHA-256 hashes. The downloaded bytes
were locally hashed and the graph interfaces inspected with ONNX. No local
export is necessary. Matching tokenizer:

- URL: https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/4649052661e53c7000355844105f8a1792088239/tokenizer.json
- Size: 2398744 bytes
- SHA-256: `4a17c975210be5ab4c36b47d8dae4eefb866dbfb1e676e394aad85dc30a3ae08`

Install explicitly (Python 3.11+ standard library, no model code executed):

```sh
python3 tools/fetch_siglip.py --cache /path/to/model-cache
export TESSERA_SIGLIP_CACHE=/path/to/model-cache
```

The script verifies sizes/hashes and atomically publishes downloads. Runtime
model resolution also verifies hashes. `Siglip::load(&registry, tokenizer_path,
options)` may download missing ONNX towers, but never fetches a tokenizer.
The integration test checks the cache first and never downloads anything.
Corrupt cached models are errors, not offline skips.

## Runtime and CoreML

All inference uses `ml-runtime::Session::run_tensors`, including named i64 token
inputs and arbitrary-rank outputs. The selected output is `pooler_output`, not
mean-pooled token/patch hidden states. Inputs are statically specialized with
`Session::load_with_dimensions`: vision `[4,3,224,224]`, text `[1,64]`. Arbitrary
image batch lengths are split into bounded groups of four; the last is padded,
and its extra outputs discarded.

On macOS, SigLIP selects CoreML's `NeuralNetwork` format, overriding the format
field in its supplied options while preserving `coreml` and `compute_units`.
This specific export fails Apple's MLProgram shape compiler even when bounded.
The generic runtime's default remains unchanged. The verified local hardware
run executed vision with 27 CoreML/26 CPU optimized nodes and text with 25
CoreML/25 CPU nodes. These are provider assignments, not a claim of full ANE
execution. Unsupported ops and initialization failures retain CPU fallback.
`SessionOptions::cpu()` explicitly disables CoreML.

`Siglip::partition_reports()` exposes actual executed-node assignments. Run the
hardware audit to require at least one CoreML node in each tower:

```sh
TESSERA_REQUIRE_SIGLIP_COREML=1 cargo test -p ml-embed --release --test siglip -- --nocapture
```

## Storage, search and grouping

- `VectorIndex` abstracts insert/get/cosine search. `SqliteVectorIndex` stores
  normalized little-endian f32 BLOBs in `embedding` inside
  `<index_dir>/embeddings.sqlite` (WAL). Keys are full-width image ids and model
  versions. `embedding_models` guards dimension consistency per version.
- Exact scanning uses contiguous slices and eight independent accumulation
  lanes. `HnswVectorIndex` uses MIT-licensed `hnsw_rs` cosine distance, then exact
  rescoring of returned candidates. `AutoVectorIndex` promotes above 50,000
  vectors in a model partition. Top-k HNSW is approximate, not a global recall
  guarantee.
- SQLite is the durable HNSW source of truth; the graph is rebuilt on open or
  replacement. There is no separate graph snapshot to become inconsistent.
  Use one writer per model and reopen HNSW readers after a background job.
- `SemanticIndex::open(siglip, index_dir)` pins both query inference and vector
  storage to `MODEL_VERSION`. `search_text(query, k)` returns `(ImageId, cosine)`.
  It implements `index::SemanticSearch`. Pass it to
  `Index::search_with_semantic(&Query { semantic: Some(...), .. }, &mut search)`.
  Catalog facets apply before pagination. Combining `text` and `semantic`
  performs equal-weight RRF (constant 60) over the BM25/vector union.
- For exact filtered results, catalog integration requests all vector candidates,
  which intentionally takes the exact scan path rather than dropping valid
  facet matches behind an ANN cutoff. Direct bounded `search_text` uses HNSW
  when the partition exceeds the threshold. This first implementation does not
  promise subsecond filtered retrieval at million-image scale.
- Build `EmbeddingGrouping::from_index(&vectors, &ids)`, then call
  `session.set_grouping_strategy(Box::new(strategy))` and `session.regroup(...)`.
  With both vectors present, an edge requires cosine ≥ 0.92 AND either capture
  proximity or dHash distance ≤ 6. Without vectors it conservatively requires
  time AND dHash. Disabling near-duplicates restores time-only bursts. Groups
  are connected components (transitive), not all-pairs cliques. The strategy
  snapshots vectors; rebuild it after embedding updates. No AI grouping changes
  keep/reject decisions. The old cull default remains available without ML.

## Background work

`EmbedFolderJob::new(catalog_path, index_dir, folder, Box::new(siglip))` implements
`engine_api::jobs::Job` at `Priority::Score`. Submit to a scheduler or call
`jobs::blocking_run`. It scans already-catalogued folder descendants, processes
unembedded ids in bounded batches of eight, and skips rows for the current
model/preprocessing version. RAW previews use `previews`; JPEGs honor EXIF
orientation. Cancellation is checked around decoding/inference/persistence.
Errors stop the job; prior writes survive, allowing a retry after repairing a
bad preview. Model upgrades use distinct partitions, never mixed vectors.

Only catalogued images are considered. Image content/edit invalidation and
orphan-vector garbage collection are not implemented here; callers must refresh
embeddings when reusing an id for changed pixels. No engine-api changes.

## Verification

```sh
cargo test -p ml-embed -p ml-runtime -p index -p cull --release
cargo clippy -p ml-embed -p ml-runtime --all-targets -- -D warnings
cargo fmt --check
```

Always preserve the work package's external `CARGO_TARGET_DIR` on macOS.
Offline tests exercise vector persistence/model partitions, 1k-vector exact/ANN
top-five agreement, promotion at 50,001 rows, grouping through real JPEGs and
CullSession, job scheduling/batching/cancellation, and hybrid facets/RRF. Cached
model tests cover colour blocks vs gradients, batched/single consistency,
nine-image chunk order, and all five RAW fixture previews. The query `a cube`
must rank `sony-arw.ARW` first, including the real semantic/catalog path.
