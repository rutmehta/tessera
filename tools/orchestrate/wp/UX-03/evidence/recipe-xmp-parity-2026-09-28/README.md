# Recipe setter XMP parity gate (2026-09-28)

Scope: `Engine::set_recipe_json` must publish its Develop settings into XMP while `set_selection` retains selection-only behavior. This is an engine test and source receipt, not a full application or export validation. All commands used the external `/Volumes/betterSSD/tessera-cache/target/main` target, two Cargo jobs and two Rayon workers. Each gate has a raw log, direct-result JSON, and a pre-gate byte archive plus hash manifest.

| Frozen source | Gate | Direct result |
| --- | --- | --- |
| `e5c5dee7` tests only | `cargo test --release -p tessera-ffi --lib recipe_write_tests::` | exit 101: 7 passed, 1 failed. New parity test observed XMP exposure `0.0` versus expected `1.25` after JSON assertion passed. Its later index assertion did **not** execute. Selection-retention control passed. |
| `736a9d8b` first product draft | same focused gate | exit 101 at compile: hidden `merge.rs` caller still used three-argument `persist`; no tests executed. |
| `e5a0883e` corrected product | same focused gate | exit 0: 8 passed, 0 failed. Original selection-only `persist` remains for merge and `set_selection`; recipe setter supplies a prepared Develop XMP packet to `persist_with_packet`. |
| `3bd2e341` merged with main `ba452b55` owner guard | recipe-write module plus eight individually selected Develop owner cases | all exit 0: 8 recipe-write and 8 owner tests passed. |
| `3bd2e341` | public `tessera-ffi --test api`, crate format check, strict release Clippy all targets | all exit 0: 3 API tests passed; format and lint passed. |

The combined source manifest was rechecked after gates: all 15 recorded input hashes, HEAD, and clean status matched. Net Rust diff against merged main is `crates/tessera-ffi/src/lib.rs` and `src/recipe_write_tests.rs`; Cargo files are unchanged. The prior compile failure and RED remain in this receipt. The source archive in each gate directory stores actual input bytes; its SHA-256 is in `manifest.json`.

The original recipe JSON remains the authoritative commit, followed by XMP and index; this patch does not make the three outputs atomic, turn `set_recipe_json` into CAS, change selection-only writer behavior, or change baked-export metadata. FFI archive/binding regeneration and full Swift/app validation are separate follow-up gates.
