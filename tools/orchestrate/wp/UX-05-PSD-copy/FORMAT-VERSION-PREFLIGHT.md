# Native document future-version preflight: test-first checkpoint

Branch: `codex/format-version-preflight`, based on `origin/main` `7306b4d3`.
Checkpoint: `crates/compositor/tests/roundtrip.rs` adds two regression cases and a test-only helper that rewrites a valid container's compressed manifest. **UNRUN**: no Cargo build or test was started because Resource owns the compiler slot.

## Contract

- A valid current v1 `.tessera-doc` remains byte-round-trip compatible (existing `round_trip_preserves_model_and_pixels` covers this).
- A future-version manifest returns the structured `EngineError::SchemaVersion { document: "tessera-doc", found, supported }` before decoding the current version's layer graph. The regression pairs version `FORMAT_VERSION + 1` with an unknown `MKind` tag so eager graph deserialization cannot accidentally satisfy it.
- A malformed current-v1 graph still returns a `Decode` error, rather than being mislabeled as a future schema.

## Minimal implementation plan after review

1. Preserve the container/trailer checks and manifest decompression. Deserialize a tiny header (format tag and version only) from the manifest JSON, validate the format tag, and return `SchemaVersion` when version exceeds `FORMAT_VERSION`.
2. Only for supported versions, deserialize the full `Manifest` and continue the existing graph/chunk validation unchanged. Avoid `serde_json::Value` graph conversion in production and do not alter version-1 serialization.
3. Once the compiler slot is assigned, run the two new focused tests first and confirm the future-version case fails specifically as `Decode` on the preflight baseline; then run the compositor roundtrip test target and the required broader gate.

No RAW node/schema, UI, or BDocument changes are part of this checkpoint.
