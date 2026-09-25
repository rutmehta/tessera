# WP M3-03 — Image embeddings, duplicate grouping, natural-language search (`ml-embed`)

Read docs/09 §1 (embeddings, duplicates), docs/05 §2 (embeddings store), crates/ml-runtime (registry, session), crates/index (schema, FTS, query API), crates/cull (grouping, dHash), crates/previews.
Licensing (docs/13): weights must be Apache-2.0/MIT. Use **SigLIP** base patch16-224 (Apache-2.0) exported to ONNX; if a ready ONNX export exists on Hugging Face under a permissive licence, register it in `models.toml` with URL/size/sha256; otherwise write `tools/export_siglip.py` (transformers + onnx, documented) and register the produced file's hash from a local run — do not commit weights. Also the matching text tower for NL search.
Implement `crates/ml-embed`:
- `embed_image(preview) -> [f32; 768]` (or the model's dim), batched inference through ml-runtime with CoreML; `embed_text(query)`.
- Vector store: start with SQLite (`embedding` table with BLOB + a brute-force cosine scan using SIMD-friendly loops) behind a `VectorIndex` trait; add an HNSW option via the `hnsw_rs` crate (MIT) for > 50k images; persist under the index dir.
- Near-duplicate grouping: combine capture-time proximity, dHash (from cull) and embedding cosine ≥ 0.92 into one grouping that replaces cull's time-only grouping via its existing trait/hook.
- NL search: `search_text(query, k)` returning image ids with scores; integrate into `index::Query` as an optional `semantic` term combined with facets (rank fusion: RRF between FTS and vector).
- Background job: `EmbedFolderJob` at `Priority::Score` that embeds any un-embedded image and stores the vector + model version.
- Tests: synthetic images (colour blocks vs gradients) yield higher self-similarity than cross-similarity; brute-force and HNSW agree on top-5 for 1k random vectors; grouping test with generated near-duplicates; if the model is available in the cache, an integration test embeds the five fixture previews and asserts "a cube" (text) ranks sony-arw (Rubik's cube) first; skip if offline.
`cargo test -p ml-embed -p index -p cull --release`, clippy -D warnings, fmt. Do not modify engine-api.
