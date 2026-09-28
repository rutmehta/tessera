# Engine recipe setter XMP parity audit

## Finding

`Engine::set_recipe_json` successfully persists an edited recipe and updates the index, but can leave its XMP develop settings at their previous values. This is a production parity defect, not an expected `set_recipe_json` contract: the catalog specification says to write standard XMP alongside the full recipe so other applications can read basic develop keys (`docs/05-catalog-storage-and-import.md:19-31,80`), and the other full-recipe writers explicitly call `selection_packet(...).with_recipe(&doc.recipe)` (`crates/tessera-ffi/src/develop.rs:1058-1066`, `agent_runs.rs:454-455`).

`set_recipe_json` changes `doc.recipe` then calls the shared `Engine::persist` (`crates/tessera-ffi/src/lib.rs:424-466`). `persist` constructs only `catalog::selection_packet(path, doc)` at line 267; that helper updates selection/metadata, preserving existing CRS properties or creating a selection-only packet. It never calls `XmpPacket::with_recipe`. Thus an exposure edit can produce JSON exposure 1.2 and XMP exposure 0 immediately after a successful setter call, before any Develop-session conflict. `XmpPacket::with_recipe` is the existing encoder for CRS develop values (`crates/sidecar/src/develop.rs:34-74`). Existing `set_recipe_json` tests assert recipe, index, and XMP selection, but not XMP develop parity (`crates/tessera-ffi/tests/api.rs:55-96`, `src/recipe_write_tests.rs:254-297`).

## Narrow repair and regression

Keep `set_selection`'s selection-only update behavior, including preservation of unrelated/foreign XMP properties. In `set_recipe_json`, build the packet from `catalog::selection_packet(path, &doc)?.with_recipe(&doc.recipe)?` before the recipe rename, then persist recipe → that packet → index under the existing destination gate. The simplest scoped API is a private `persist` packet argument or a private `persist_recipe_with_develop` variant; avoid unconditionally adding `with_recipe` to shared `persist`, which would change `set_selection` semantics.

Add a 2×2 JPEG regression that creates a valid `Recipe::edit` exposure change, invokes `set_recipe_json`, and asserts the saved JSON, XMP `to_recipe().recipe.settings.tone.exposure`, and index hash agree. Then call `set_selection` and assert the XMP develop value remains while selection changes. A foreign descriptive property preservation assertion would cover the existing `selection_packet` purpose. Run current recipe-write gate tests and tiny API tests with this regression. No current tests or builds were run for this audit.

## Scope

The recipe remains the authoritative commit and XMP/index can still fail after the recipe rename; this repair does not make three outputs a transaction. It does not turn `set_recipe_json` into CAS, fix stale client inputs, cover other writers, or change baked-output metadata policy. XMP imported from a recipe-less file remains a separate source route.
