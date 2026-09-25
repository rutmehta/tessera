# import-lrcat

`import(path) -> EngineResult<ImportPlan>` reads a Lightroom catalog without
writing it or its photos. `inspect(path) -> EngineResult<Summary>` validates the
same plan and returns counts. `plan.library.write(path)` explicitly writes an
atomic serde `library.json` document. No sidecars are written by inspection.

## Safety and representation

- Every catalog is copied to temporary storage before SQLite opens it, including
  its WAL if present. SQLite opens the copy with URI `mode=ro` and read-only
  flags. This also handles Lightroom lock files without touching them. Changes
  detected during copying fail with a retry/close-Lightroom error. The source
  must remain quiescent during the copy; this is not an online SQLite backup API.
- Missing core tables produce named `EngineError::Decode` errors. Optional
  metadata tables missing in older catalogs are listed in `plan.report`.
  Additional columns are accepted.
- Virtual copies retain the master's actual photo path and receive separate
  recipe identities and display names. Identity is derived from the canonical
  catalog path plus local image id; moving the catalog changes those identities.
- Collections use catalog-local ids scoped to the exported library. Nested sets
  retain parent links, keyword trees retain synonyms, and smart albums contain
  a data-only `SavedSearch` AST. Lua is never evaluated.
- Current CRS edits use the engine-api `CrsKey` table and `Recipe::edit`, so every
  imported recipe validates against history replay. Curves normalize Adobe's
  0–255 coordinates. Supported mask geometry and AI mask kinds are translated.
  Unknown keys, unsupported structures/resources and invalid values are retained
  in `Recipe.unknown` with diagnostics, not silently discarded. Mask XML is
  retained even when translation succeeds. PV1/2 imports carry a warning.
- Historical steps and snapshots are preserved as complete source rows on each
  image and in recipe extension fields. They are not falsely represented as
  replayable native edits. Faces retain region/cluster columns and keyword-face
  links, stacks retain source metadata and membership, and GPS is exposed as a
  latitude/longitude pair.

## Verification boundary

There is no real catalog fixture. `tests/make_fixture.rs` creates a synthetic
SQLite catalog with Lightroom table and column names. Tests cover paths,
selections, virtual copies, hierarchy/synonyms, smart rules, curves/masks, GPS,
faces, stacks, history/snapshots, JSON round trips, additional columns, missing
core tables, and committed WAL reads with byte-for-byte source preservation.
Parser tests cover malformed/unsupported data and non-executable Lua parsing.

Real Lightroom schema variants and render equivalence remain unverified.
Resource-backed edits (DCP profiles, arbitrary retouch/Look/LensBlur payloads,
AI pixel blobs) are not resolved or rendered by this crate. Inspect diagnostics
before persisting an import.

Run with `CARGO_TARGET_DIR` outside the repository:

    cargo test -p import-lrcat --release
    cargo clippy -p import-lrcat --all-targets -- -D warnings
    cargo fmt --check
