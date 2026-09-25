# SQLite catalog index

`Index::open(path)` opens a per-user, rebuildable catalog with WAL,
`synchronous=NORMAL`, a 256 MiB mmap ceiling, foreign keys, versioned migrations,
FTS5 and R*Tree. Bundled SQLite supports R*Tree in this build; no fallback is
needed. The `gps` virtual table is kept in sync by image triggers.

The public API returns `engine_api::error::EngineResult`. Image IDs and selection
values use engine-api types without contract modifications.

## Scanning

`Scanner::new(&sidecars, &metadata).scan(&mut index, root)` (or `Index::scan`)
returns the number of inserted/refreshed images. Paths are canonicalized, symlink
entries are not followed, image size and nanosecond mtime detect source changes.
An unchanged second scan produces zero SQLite row changes. Image updates are
transactional. Provider and traversal errors propagate rather than silently
being recorded as empty metadata.

Embedded JPEG/TIFF/DNG metadata is read with kamadak-exif independently of the
optional MetadataProvider. Unsupported/corrupt EXIF does not prevent file
indexing. Providers can supply RAW metadata and override embedded values.
Capture dates use sortable ISO-like local timestamps (no invented timezone).

SidecarReader receives the image path. The scanner fingerprints both
`image.ext.xmp`, `image.xmp`, and `.edits/image.json` so sidecar-only changes
refresh the index. SidecarData carries caption, keywords, flattened values,
optional selection and optional recipe hash. An absent selection leaves existing
catalog selection intact. The no-op hooks permit indexing before sidecar/RAW
integration. IDs currently derive deterministically from canonical paths.
The scanner upserts files; it does not prune disappeared files or track renames.

## Queries

All filters are ANDed. `text` is an FTS5 MATCH expression (including its query
syntax), not interpolated SQL. Folder scope is a canonical absolute directory
and includes descendants, with literal wildcard characters escaped. Dates are
inclusive bounds using sortable strings. `limit=0` means 100, `offset=0` starts
at the first match; ordering is capture time then ImageId.

Keyword filters include descendants through the closure table. Keywords have
unique names; parent assignment is immutable, and missing parents/cycles are
rejected. Tagging also refreshes searchable keyword text. Facets ignore paging
and count camera/lens/decision and directly attached keyword values across all
matching images. Missing camera/lens values use the empty string bucket.

## Verification

    cargo test -p index
    cargo clippy -p index --all-targets -- -D warnings
    cargo fmt --check
    cargo test -p index -- --ignored --nocapture

Keep CARGO_TARGET_DIR outside the repo as required by the work-package setup.
The ignored benchmark inserts 100,000 images, ten camera values, seven lens
values, caption variants, selections, and keyword links. Its text search matches
10,000 images and returns a 100-image page; its camera-filtered facets count
10,000 images. Both paths warm up before the <100 ms assertion. The dev profile
optimizes bundled SQLite's C library so the normal ignored-test command measures
an optimized SQLite build, without requiring a release build of the workspace.
RAW fixture coverage skips explicitly when `fixtures/raw` is absent.
