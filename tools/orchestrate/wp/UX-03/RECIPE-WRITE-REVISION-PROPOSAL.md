# Internal recipe write revision: test-first checkpoint

Status: **UNRUN / proposal only**. This checkpoint contains tests and test-only module wiring, not the `recipe_write` implementation. It was prepared from `origin/main` `b00968af` after the rendered-export metadata merge. The focused RED must be observed and recorded before production edits.

## Narrow first slice

Add an internal, process-wide gate keyed by the **resolved recipe sidecar destination** (`Sidecar::paths(image).recipe`), then use it in exactly `Engine::set_selection` and `Engine::set_recipe_json`. Both currently hold only their own `Engine` catalog mutex while reading and persisting a recipe, XMP, and rebuildable index. Two `Engine` instances can therefore overlap on the same destination. Resolve the image ID to a path under the catalog mutex, release it, acquire the destination gate, reacquire the catalog mutex, verify that ID still resolves to the same image path, then read and persist. Keep the destination guard through recipe/XMP/index completion and release it before notifying listeners. A path change fails rather than silently writing to a different source. Do not hold the catalog mutex while waiting for the gate.

The internal `RecipeRevision` records the full **raw** recipe sidecar bytes and the selected XMP packet path/presence/bytes. Hash these with a domain-separated BLAKE3 digest and retain the destination gate state plus its monotonic epoch. `Recipe::recipe_hash()` is a render cache key and excludes selection, history, IDs, and unknown fields; it cannot serve as a write revision. The raw digest observes unknown envelope bytes even though current `RecipeDocument` serialization does **not** preserve unknown envelope members. This slice does not claim lossless persistence of them.

Use a weak-value lock table only if each retained `RecipeRevision` holds a strong `Arc` to its key state. Otherwise pruning and recreation can reset the epoch while an old token survives. A write guard advances the epoch conservatively on every participating attempt, including an error or same-byte write. Check overflow rather than wrapping. A later atomic compare-and-write must compare key identity, epoch, and current raw bytes **while the destination guard is held**; this patch does not expose that operation to batch Apply.

The key resolves the existing destination path. `.edits/<stem>.json` is shared by same-stem image extensions. `catalog::document` rejects a sidecar whose recipe `image_id` belongs to another image, so this patch serializes the collision but does not redefine its persistence format. Resolve an existing `.edits` symlink when forming the key; if it does not yet exist, use the canonical image parent plus `.edits/<stem>.json`. External symlink replacement and external-process writes remain outside the in-process guarantee.

## Proposed regressions

`crates/tessera-ffi/src/recipe_write_tests.rs` supplies five tiny tests: raw future-envelope/XMP bytes and selected-path changes; retained-token weak-key ABA after table churn; same-stem destination and existing image-ID rejection; and one held-gate serialization case for each of the two Engine writers. The JPEGs are 2×2. The worker tests signal start and have bounded channel waits; no sleeps, RAW decoding, GPU, or large catalog fixture.

First focused RED command after the compiler slot is released:

```sh
CARGO_BUILD_JOBS=2 RAYON_NUM_THREADS=2 cargo test -p tessera-ffi --lib recipe_write_tests -- --nocapture
```

The expected initial failure is unresolved `crate::recipe_write`; preserve direct exit, log, source hashes, and toolchain version. After a reviewed implementation, rerun the five focused tests, existing adjacent `tessera-ffi` recipe/selection tests, and strict crate validation within a bounded timeout. Do not broaden to a full application gate in this slice.

## Explicit remaining bypasses

Develop's open-session writer, Agent and MCP edits, Culling's multi-image transaction, Lightroom import, merge publication, XMP-only metadata writes, and external processes do not use this first gate. A legacy `set_recipe_json` call may still contain stale settings; serializing its read/write does not make its input a CAS request. Pending Develop saves also remain a batch race. Do not enable batch Apply UI or claim every recipe writer is protected until those owners and a durable run-specific revert contract are addressed separately.
