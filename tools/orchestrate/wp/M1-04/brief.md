# WP M1-04 — previews crate

Read crates/engine-api/CONTRACTS.md first and use its types; do not modify engine-api (if a contract change is unavoidable, stop and print RESULT: FAIL with the reason so it can be reviewed). The workspace root Cargo.toml lists this crate already; add dependencies via [workspace.dependencies] where the version is pinned there. Everything must pass `cargo test -p <crate>` and `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo fmt --check`. Fixtures: `fixtures/raw/` has one CR3, ARW, NEF, RAF, DNG each (run `bash fixtures/fetch.sh` if missing). Tests that need fixtures must skip with a message when the directory is absent.

Implement `crates/previews` per docs/05 §2.2 and docs/08:
- Content-addressed store under a given cache dir: key = BLAKE3(file bytes) + orientation + recipe_hash; levels 1/8, 1/4, 1/2 (and 1/1 on request) as JPEG (quality 90, `zune-jpeg` for decode, `jpeg-encoder` or `image` crate's encoder for encode; JPEG XL encode is deferred until the libjxl binding exists, leave a `Codec` trait with a Jpeg impl).
- `PreviewStore::get(key, level) -> Option<Bytes>`, `put`, LRU eviction by total bytes with a configurable cap, and an `ensure(image, level, source: &dyn Fn(level) -> RgbImage)` that fills missing levels from the largest available one (downscale with a proper box/Lanczos filter, not nearest).
- Embedded-preview fast path: `from_embedded_jpeg(bytes, orientation)` that decodes, applies EXIF orientation, and stores the pyramid without touching the raw pipeline.
- Tests: pyramid dimensions, eviction respects cap, orientation 6 rotates correctly (assert pixel positions), a 45 MP synthetic image builds its pyramid in < 400 ms in release (ignored test, printed).
