# M1-13 handoff

Implemented the culling session, sidecar-backed selection actions, global undo/redo,
basket/library persistence, burst/dHash grouping, pluggable scoring, derived status,
and review-only defect sweep. Added index migration 004 for export logs and quality
scores plus public image-info/score/export APIs. No engine-api or sidecar changes.

## Verification

Executed successfully with
`CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M1-13`:

```
cargo test -p cull -p index --release && cargo clippy -p cull -p index --all-targets -- -D warnings && cargo fmt --check
```

- 30 tests passed, 0 failed, 1 existing index performance benchmark ignored.
- Clippy with warnings denied passed; workspace formatting check passed.
- Vendored LibRaw C++ build warnings are emitted by the existing dependency.
- Independent static review passed after fixing same-stem destination collision
  handling, redo cursor restoration, and undo of newly created basket albums.
- Failure-injection test verifies rollback of all group sidecars/index changes
  after the final SQLite selection update fails.
- `git diff --check` passed. Changes are confined to the allowed paths. No local
  `target/` directory and no commit was made.

Full command output: `verification.log` in this directory.
API/semantics documentation: `crates/cull/README.md`.

## Explicit boundaries

- Atomicity is per file, with compensating rollback for runtime failures, not a
  crash-atomic multi-file/SQLite transaction. Recipe selection is authoritative;
  opening reconciles index selection before applying filters.
- Existing `.edits/<stem>.json` naming cannot distinguish same-stem RAW+JPEG pairs.
  Mutations fail safely on indexed destination collisions; the shared sidecar
  contract was not changed under this work package's path restrictions.
- Basket membership stays in library.json, not per-image sidecars (docs/06 §4.2).
  Published/exported are historical flags; portable publish state remains future
  work. Album membership is orthogonal to the edit/export/publish phase.
- dHash grouping uses connected components and O(n²) hash comparisons. Missing
  preview bytes do not imply duplicates; decoding errors are exposed for review.

RESULT: PASS
