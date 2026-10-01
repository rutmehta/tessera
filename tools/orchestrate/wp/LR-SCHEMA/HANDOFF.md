# LR-SCHEMA: conditional recipe schema 4

Branch `wp/LR-SCHEMA`, worktree `/Volumes/betterSSD/tessera-worktrees/lr-schema`,
2026-10-01. Implements the coordinator ruling "Conditional `schema_version` 4"
(docs/coordination/CODEX-BRIEF-2026-09-30.md, rulings log).

## Commits

- Base: `44db24b251522a477375d36f3bc34244ea015836` (origin/main).
- RED: `0e58d99339e24a11dfcff26fe446ef186dcf17a5`, `test(LR-SCHEMA): RED pin schema 4 write refusal and conditional version harness`.
- GREEN: `3b49a6c710ad74dcdf76839b3841bff5b6b2d344`, `feat(LR-SCHEMA): conditional schema 4 driven by one predicate list`.
- Docs: `c6262eedec677dd0ad8bc520e23c2fea351e5f1a` (first handoff).
- Review fix: `9d1888b99dd8543314804ce817bda14ea73d1c80`, `fix(LR-SCHEMA): apply review: lane-proof pins, pinned-raw ceiling, ffi tests`.
- The following `docs(LR-SCHEMA):` commit updates this file. It cannot contain its own hash.

## How other lanes use it (one line)

Add `("point_colors", |r| r.settings.color.point_colors.is_some())` to
`V4_FEATURE_PREDICATES` in `crates/engine-api/src/recipe/schema.rs`, and add a
bumped-only-when-present test in its `v4_feature_predicates` module:
`assert_bumped_only_when_present("point_colors", |r| { /* set the field */ })`.

The first entry also raises `max_writable_schema_version()` from 3 to 4, so this
build can save the schema 4 documents it writes. The newer-than-supported pins are
written as `max_writable_schema_version() + 1` and need no edit:
`recipe::tests::newer_than_writable_documents_load_but_never_write`,
`sidecar/tests/schema_v4.rs`, `sidecar/tests/roundtrip.rs`, merge
`recipe_xmp_refuses_newer_schema`, `engine-api/tests/pinned_raw.rs`
`explicit_schema_and_owner_are_required`, and tessera-ffi
`schema_just_above_writable_max_cannot_replace_a_record`. The list-dependent tests
in `schema.rs` (`writable_max_follows_the_list`,
`fixtures_require_only_base_schema_while_unused`) also stay true.

### Checklist for the first lane that registers a real predicate

1. Add the predicate line and an `assert_bumped_only_when_present` test.
2. `crates/import-lrcat/tests/schema_version.rs` asserts that every import (fixture
   plus 300-image synthetic catalog) is schema 3. If the import fixtures use the
   feature, restrict that assertion to images without it and assert 4 for the rest.
   The `import-lrcat` `golden.rs` digests then change for those fixtures. Re-pin them
   on purpose and record the reason.
3. Reload inequality: writing never mutates the caller, so once the feature is used,
   a recipe that is saved and reloaded compares unequal on `schema_version`
   (3 in memory, 4 reloaded). These tests will need `schema_version` ignored in their
   equality, or the in-memory version set to 4 before comparing, if they use the
   feature:
   - `crates/sidecar/tests/roundtrip.rs:22` (`assert_eq!(doc, read)`)
   - `crates/merge/tests/recipe.rs:35` (`assert_eq!(parsed, recipe)`)
4. Sticky bump: a recipe written as 4 stays 4 on load and re-save even after the
   feature is removed (`max(schema_version, required)`). This is intended and is
   documented in the `schema.rs` module doc.
5. `pinned_raw.rs` now accepts `3..=max_writable_schema_version()`. This lane widened
   it, so v4 raw recipes become pinnable automatically and no edit is needed.
6. tessera-ffi `get_recipe_json` (`lib.rs:488`) reads through `to_json`. A recipe
   above the writable maximum therefore cannot be read through the FFI at all, not
   only written. Recipes at schema 4 are readable once the maximum is 4. The
   consequence: a v4 document opened by a pre-v4 build shows an FFI read error,
   not a read-only view.
7. Add a tessera-ffi journal test: a `save_local_recipe` of a recipe using the
   feature stores `"schema_version": 4` in the journal envelope. FFI tests cannot
   inject a predicate, so only the real one can prove this; the no-feature
   counterpart is `local_save_keeps_legacy_envelope_schema_version`.

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
- Consequences once a feature is used: saved-then-reloaded recipes are unequal on
  `schema_version` (3 in memory, 4 reloaded), and the bump is sticky after the
  feature is removed. Both are documented in the `schema.rs` module doc.
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
| `tessera-ffi::smart_preview_store::validate_recipe` (journal ceiling) | hard-coded `RECIPE_SCHEMA_VERSION` | n/a | now `max_writable_schema_version()`; tested at max (accepted) and max+1 (rejected) |
| `engine-api::pinned_raw::PinnedRawDescriptor::new` (input check, not a write) | exact `== 3` | n/a | now `3..=max_writable_schema_version()` |
| `tessera-ffi::smart_preview` publish (`commit` path, `to_json()?`) | yes | bytes from journal | none |
| `import-lrcat` `ImportPlan` / `PlanJson` (derive) | no (recipes minted by this build) | yes (Serialize) | none |

No load refusal was added.

## Review changes (9d1888b9)

- `CONTRACTS.md` §7 is rewritten: an additive field registers one
  `V4_FEATURE_PREDICATES` line and an `assert_bumped_only_when_present` test.
  `RECIPE_SCHEMA_VERSION` stays 3.
- The pins are lane-proof (see above). `pinned_raw` is widened. `schema.rs` documents
  the consequences. Two tessera-ffi tests were added.
- Optional nits: the nested unknown member is now asserted in the load pin, and the
  spelling is "serialisation" in new text. The sidecar test still uses
  `std::env::temp_dir`, because `sidecar` has no `tempfile` dev-dependency and adding
  one would change `Cargo.lock`.

## Tests

- RED (at `0e58d993`): 5 engine-api `v4_feature_predicates` tests (stubs
  `unimplemented!`), `import-lrcat --test schema_version`, and
  `merge --test recipe::recipe_xmp_refuses_newer_schema` failed. The pins
  `schema_4_documents_load_but_never_write` (renamed in review to `newer_than_writable_documents_load_but_never_write`), `serialisation_bytes_are_pinned`
  (6 digests recorded on main code) and `sidecar --test schema_v4` passed.
- GREEN: all pass. The pinned digests and the existing goldens
  (`import-lrcat` `golden.rs` synthetic-catalog digest and PlanJson byte-identity,
  `hash_is_stable_across_releases`, sidecar roundtrip byte equality) are unchanged.
- `cargo test --locked -p tessera-ffi --release --lib smart_preview`: 37 passed at GREEN, and 39 after the review fix.
- `cargo test --locked -p merge --release --test recipe`: 2 passed.

## Gates (after review fix 9d1888b9, worktree, sequential)

Env: `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/lr-schema CARGO_BUILD_JOBS=4 TMPDIR=/Volumes/betterSSD/tmp/`.
`fixtures/raw` is symlinked to the main checkout.

| Command | Exit | Result |
| --- | --- | --- |
| `cargo test --locked -p engine-api -p import-lrcat -p sidecar --release` | 0 | 237 passed, 0 failed, 0 ignored (45 summaries), 163.9 s |
| `cargo clippy --locked --workspace --all-targets --release -- -D warnings` | 0 | 24.5 s |
| `cargo fmt --all -- --check` | 0 | |

Additional runs: `cargo test --locked -p tessera-ffi --release --lib smart_preview`
gave 39 passed. `cargo test --locked -p merge --release --test recipe` gave 2 passed.
At GREEN `3b49a6c7` the same three gates also exited 0 (237 passed).
No Swift gates were run.
