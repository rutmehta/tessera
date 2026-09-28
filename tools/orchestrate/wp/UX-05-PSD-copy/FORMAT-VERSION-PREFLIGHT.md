# Native document future-version preflight: test-first checkpoint

Branch: `codex/format-version-preflight`, based on `origin/main` `7306b4d3`.
Checkpoint: `crates/compositor/tests/roundtrip.rs` adds two regression cases and a test-only helper that rewrites a valid container's compressed manifest. This was committed first as an **UNRUN** test checkpoint (`ff0533cd`) before implementation.

## Contract

- A valid current v1 `.tessera-doc` remains byte-round-trip compatible (existing `round_trip_preserves_model_and_pixels` covers this).
- A future-version manifest returns the structured `EngineError::SchemaVersion { document: "tessera-doc", found, supported }` before decoding the current version's layer graph. The regression pairs version `FORMAT_VERSION + 1` with an unknown `MKind` tag so eager graph deserialization cannot accidentally satisfy it.
- A malformed current-v1 graph still returns a `Decode` error, rather than being mislabeled as a future schema.

## Minimal implementation plan after review

1. Preserve the container/trailer checks and manifest decompression. Deserialize a tiny header (format tag and version only) from the manifest JSON, validate the format tag, and return `SchemaVersion` when version exceeds `FORMAT_VERSION`.
2. Only for supported versions, deserialize the full `Manifest` and continue the existing graph/chunk validation unchanged. Avoid `serde_json::Value` graph conversion in production and do not alter version-1 serialization.
3. Once the compiler slot is assigned, run the two new focused tests first and confirm the future-version case fails specifically as `Decode` on the preflight baseline; then run the compositor roundtrip test target and the required broader gate.

## Execution record

The expected RED was observed before production changes: on `ff0533cd`, the future-v2/unknown-kind input returned `Decode` (`unknown variant future_layer_kind`) rather than `SchemaVersion`; direct exit 101. The v1 unknown-kind control returned the expected `Decode`; direct exit 0.

The header preflight was committed as `fb81f279` before GREEN. The future-version test then passed, and the roundtrip target passed 4/4. The first `cargo fmt -p compositor -- --check` returned 1 for two test-only line wraps; rustfmt corrected those lines. The final roundtrip 4/4 and strict `cargo clippy -p compositor --all-targets -- -D warnings` passed using the exact production source hash and formatted-test hash below, before the test-format-only commit `26845eed`. The second fmt check passed. There were no production changes after `fb81f279`.

Raw command logs, direct exit files, and before/after source hashes are checked in under [`evidence/format-preflight-2026-09-27/`](evidence/format-preflight-2026-09-27/). They were copied unchanged from `/tmp/tessera-format-version-preflight-red/`. `head.before-green.txt` records `1ed91805`, before the production change was committed; `source-hashes.implementation.txt` froze the implementation bytes at that point. The subsequent GREEN used the same `format.rs` bytes committed in `fb81f279` and the test file after the two rustfmt-only line-wrap changes. The final tested source hashes are:

- `crates/compositor/src/format.rs`: `12d0b5055023d7748a26eba36c36b4d93bcb74cd9bebf8b3e8b434d95cd6e82c`
- `crates/compositor/tests/roundtrip.rs`: `5f1c391f53d9c363303e435332e1e214fbf1097e358871478e5159df49c2e64b`

No RAW node/schema, UI, or BDocument changes are part of this checkpoint.
