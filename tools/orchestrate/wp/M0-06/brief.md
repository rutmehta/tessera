# WP M0-06 — `libraw-ffi` crate

Create `crates/libraw-ffi/` as a standalone crate (its own Cargo.toml; it will be added to the workspace at merge, so do NOT edit the root Cargo.toml — there may not be one).
- Vendor LibRaw 0.22.x source under `crates/libraw-ffi/vendor/LibRaw/` (download the release tarball from https://www.libraw.org/download, choose the CDDL-1.0 licence terms, keep LICENSE files). No zlib/jpeg optional deps needed initially; build with `cc` crate, C++17, `-DUSE_ZLIB=0`. Alternatively `pkg-config` to a Homebrew libraw if vendoring fails — but vendoring is preferred; document which one you did.
- `build.rs`: compiles vendored sources; generates bindings with `bindgen` over `libraw/libraw.h` (allowlist `libraw_*` and `LibRaw_*` items).
- Safe wrapper `src/lib.rs`: `RawFile::open(path) -> Result<RawFile>`, `.unpack()`, `.cfa_data() -> CfaImage { width, height, data: Vec<u16>, cfa_pattern: [u8;4] or XTrans 6x6, black: [f32;4], white: u32, wb_coeffs: [f32;4], color_matrix: [[f32;3];3] (cam→XYZ from LibRaw `cam_xyz`), crop: rect }`, `.embedded_preview() -> Option<Vec<u8>>` (JPEG bytes via `libraw_unpack_thumb`), `.metadata()` (make, model, iso, shutter, aperture, focal, timestamp, orientation). Error type via thiserror.
- Tests: if `fixtures/raw/` exists at the repo root (it may not yet), iterate the files, assert decode succeeds and dimensions > 1000. Otherwise generate no fixture-dependent failures (skip with a message). Add at least one test that always runs (e.g. opening a non-existent path returns an error).
- Must build on macOS arm64 with `cargo build -p libraw-ffi` from inside the crate dir and `cargo clippy -- -D warnings`.

Test command: `cd crates/libraw-ffi && cargo test && cargo clippy -- -D warnings`.
