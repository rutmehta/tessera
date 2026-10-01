# LR-SCHEMA: conditional recipe schema 4

Branch `wp/LR-SCHEMA`, worktree `/Volumes/betterSSD/tessera-worktrees/lr-schema`,
2026-10-01. Implements the coordinator ruling "Conditional `schema_version` 4"
(docs/coordination/CODEX-BRIEF-2026-09-30.md, rulings log).

## Commits

- Base: `44db24b251522a477375d36f3bc34244ea015836` (origin/main).
- RED: `0e58d99339e24a11dfcff26fe446ef186dcf17a5`, `test(LR-SCHEMA): RED pin schema 4 write refusal and conditional version harness`.
- GREEN: `3b49a6c710ad74dcdf76839b3841bff5b6b2d344`, `feat(LR-SCHEMA): conditional schema 4 driven by one predicate list`.
- The `docs(LR-SCHEMA):` commit that adds this file. It cannot contain its own hash.

## How other lanes use it (one line)

Add `("point_colors", |r| r.settings.color.point_colors.is_some())` to
`V4_FEATURE_PREDICATES` in `crates/engine-api/src/recipe/schema.rs`, and add a
bumped-only-when-present test in its `v4_feature_predicates` module:
`assert_bumped_only_when_present("point_colors", |r| { /* set the field */ })`.

The first entry also raises `max_writable_schema_version()` from 3 to 4, so this
build can save the schema 4 documents it writes. That lane must update the pin
`recipe::tests::schema_4_documents_load_but_never_write` (and
`sidecar/tests/schema_v4.rs`, `merge` `recipe_xmp_refuses_newer_schema`) to use
schema 5 for "newer than supported". `real_list_is_empty` must also be deleted then.

## API (engine-api `recipe::schema`, re-exported from `recipe`)

- `RECIPE_SCHEMA_VERSION_V4 = 4`. `RECIPE_SCHEMA_VERSION` stays 3.
- `required_schema_version(&Recipe) -> u32`: 4 if any predicate matches, else 3.
- `v4_features_used(&Recipe) -> Vec<&'static str>`: names of the matching predicates.
- `max_writable_schema_version() -> u32`: 3 while the list is empty, then 4.
- `Recipe::ensure_writable()`: the single write check (stored `schema_version` >
  writable max gives `EngineError::SchemaVersion`). `to_json` calls it.
- `Serialize for Recipe` is now hand-written. It uses a borrowed mirror struct with
  the same members, order and `flatten`, and exhaustive destructuring, so a new field
  will not compile until it is added. It writes `max(schema_version, required)` only
  when required > 3. Otherwise it writes the stored version unchanged, which keeps
  in-memory legacy versions (for example `schema_version = 1` in
  `sidecar/tests/schema_upgrade.rs`) byte-identical. The caller's struct is never
  mutated.
- The predicate list is the private `const V4_FEATURE_PREDICATES` (empty). A
  `cfg(test)` thread-local override (`schema::test_override::with`) proves the
  harness with a test-only predicate and is not compiled into non-test builds.

## Write/serialise paths enumerated

Checked means the stored version is refused before any byte is written. Bumped means
the written version comes from content.

| Path | Check | Bump | Change |
| --- | --- | --- | --- |
| `Recipe::to_json` (engine-api) | yes | yes (Serialize) | now via `ensure_writable` |
| raw serde `serde_json::to_*(&recipe)` / envelopes embedding a Recipe | no (projection) | yes | bump closed by hand-written Serialize |
| `sidecar::Sidecar::write_recipe` (`to_vec_pretty(document)`) | yes (`to_json()?` first) | **was missing** | closed by Serialize |
| `sidecar::XmpPacket::with_recipe` / `from_recipe` | yes | n/a (XMP CRS, no version) | none |
| `cull::persistence`, `agent`, `style-profile::library`, `tessera-mcp::console`, `tessera-ffi` `lib.rs:311`, `lrcat.rs:1608`, `develop.rs:1295`, `agent_runs.rs:480` | via `write_recipe` | via Serialize | none |
| `tessera-ffi::set_recipe_json` | yes (`to_json()?`) | via write path | none |
| **`merge::recipe_xmp`** (native recipe in merged DNG XMP) | **was missing** | yes | now `ensure_writable()` first |
| **`tessera-ffi::smart_preview::save_local_recipe`** (journal) | yes (store `validate_recipe` + `to_json`) | **was missing**: copied only process_version/source_kind/settings/history/ids, so a v4 feature would have been journaled and published as v3 | now copies the bumped `schema_version` when `required_schema_version > 3`. Otherwise the envelope bytes are untouched |
| `tessera-ffi::smart_preview_store::validate_recipe` (journal ceiling) | hard-coded `RECIPE_SCHEMA_VERSION` | n/a | now `max_writable_schema_version()` |
| `tessera-ffi::smart_preview` publish (`commit` path, `to_json()?`) | yes | bytes from journal | none |
| `import-lrcat` `ImportPlan` / `PlanJson` (derive) | no (recipes minted by this build) | yes (Serialize) | none |

No load refusal was added.

## Things to know

- `engine-api/src/pinned_raw.rs:89` requires an explicit `"schema_version": 3` in
  pinned-raw descriptors. A schema 4 recipe cannot be pinned until that contract is
  widened. This lane did not change it.
- `tessera-ffi::get_recipe_json` (`lib.rs:488`) reads through `to_json`, so the FFI
  cannot read a newer-schema recipe either. This is existing behaviour and was not
  changed.
- The save_local_recipe bump has no test: ffi cannot inject a predicate. The first
  real-predicate lane should add an ffi test that a journaled edit using its feature
  carries `"schema_version": 4`.

## Tests

- RED (at `0e58d993`): 5 engine-api `v4_feature_predicates` tests (stubs
  `unimplemented!`), `import-lrcat --test schema_version`, and
  `merge --test recipe::recipe_xmp_refuses_newer_schema` failed. The pins
  `schema_4_documents_load_but_never_write`, `serialisation_bytes_are_pinned`
  (6 digests recorded on main code) and `sidecar --test schema_v4` passed.
- GREEN: all pass. The pinned digests and the existing goldens
  (`import-lrcat` `golden.rs` synthetic-catalog digest and PlanJson byte-identity,
  `hash_is_stable_across_releases`, sidecar roundtrip byte equality) are unchanged.
- `cargo test --locked -p tessera-ffi --release --lib smart_preview`: 37 passed.
- `cargo test --locked -p merge --release --test recipe`: 2 passed.

## Gates (GREEN, worktree, sequential)

Env: `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/lr-schema CARGO_BUILD_JOBS=4 TMPDIR=/Volumes/betterSSD/tmp/`.
`fixtures/raw` is symlinked to the main checkout.

| Command | Exit | Result |
| --- | --- | --- |
| `cargo test --locked -p engine-api -p import-lrcat -p sidecar --release` | 0 | 237 passed, 0 failed, 0 ignored (45 summaries), 138.8 s |
| `cargo clippy --locked --workspace --all-targets --release -- -D warnings` | 0 | 73.2 s |
| `cargo fmt --all -- --check` | 0 | |

No Swift gates were run.
