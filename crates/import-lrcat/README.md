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

## Plan fields for hosts (M2-13b)

`ImportPlan::roots` keeps the `AgLibraryRootFolder` rows so a host can relocate
a moved drive (the app maps each root to its new location before writing).
`ImportedImage::{rating, pick, color_label}` keep the source selection columns
so the host can preview the Lightroom → Tessera selection mapping (docs/06
§2.1) and let the user rename or drop colour labels. Both are `serde(default)`,
so older serialized plans still load.

## Previews.lrdata (`previews`)

Lightroom's standard previews are read only for the fidelity comparison.
`PreviewIndex::open(catalog)` copies `<stem> Previews.lrdata/previews.db` to
temporary storage and opens the copy read-only (as for the catalog). Its
`ImageCacheEntry` table maps `imageId` (the catalog image id) to `uuid` and
`digest`; the pyramid is `<uuid[0]>/<uuid[0..4]>/<uuid>-<digest>.lrprev`.

An `.lrprev` file is a sequence of sections, each introduced by a header:

| Bytes | Field |
| --- | --- |
| 0–3 | magic `AgHg` |
| 4–5 | header length, u16 big-endian (32 in practice) |
| 6 | version (u8) |
| 7 | kind (u8) |
| 8–15 | payload length, u64 big-endian |
| 16–23 | padding length after the payload, u64 big-endian |
| 24–(header length) | NUL-padded ASCII name |

The payload follows the header, then the padding. The `header` section is
Lua-like text describing the levels; `level_1`, `level_2`, … each hold one
baseline JPEG, smallest first. `parse_lrprev` returns every section,
`jpeg_levels` the JPEG levels, and `PreviewIndex::jpeg(id, min_edge)` the
smallest level whose long edge reaches `min_edge` (else the largest).
`jpeg_icc_profile` reassembles an embedded ICC profile so the host can convert
the preview to sRGB before comparing. This layout follows long-standing
third-party extractors; it is verified here only against `write_lrprev`, not
against files written by Lightroom.

## Synthetic fixture (`--features fixture`)

`fixture::write(dir)` writes `Catalog/Fixture.lrcat`, `Catalog/Fixture
Previews.lrdata` and JPEG originals under `Photos/2026/{wedding,portraits}`.
The catalog records its root as `/Volumes/Old Drive/Photos/`, so the photos
must be found by relocating that root. It contains six photos (one original
missing on disk), one virtual copy, picks/rejects/stars 0–5, colour labels
(`Red`, `Client`, `Blue`), a keyword tree with a synonym and a duplicated name
(`Paris`), a collection set with a collection and a smart collection, a second
collection, an unsupported smart rule (`labelColor`), a stack, a face, GPS,
history steps and an unknown develop key. Its "Lightroom previews" are this
module's own approximation of the edits, not Adobe renders. The CLI exposes it
as `tessera import lrcat --make-fixture <dir>`.

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
    cargo clippy -p import-lrcat --all-targets --all-features -- -D warnings
    cargo fmt --check
