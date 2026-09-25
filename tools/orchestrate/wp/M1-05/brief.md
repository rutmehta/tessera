# WP M1-05 — pipeline-cpu crate: reference operators + golden tests

Read crates/engine-api/CONTRACTS.md first and use its types; do not modify engine-api (if a contract change is unavoidable, stop and print RESULT: FAIL with the reason so it can be reviewed). The workspace root Cargo.toml lists this crate already; add dependencies via [workspace.dependencies] where the version is pinned there. Everything must pass `cargo test -p <crate>` and `cargo clippy -p <crate> --all-targets -- -D warnings` and `cargo fmt --check`. Fixtures: `fixtures/raw/` has one CR3, ARW, NEF, RAF, DNG each (run `bash fixtures/fetch.sh` if missing). Tests that need fixtures must skip with a message when the directory is absent.

Implement `crates/pipeline-cpu`: the exact scalar f32 reference for the Milestone 1 stages (docs/04 §3, docs/07 §2, §4, §5). Operate on `engine_api::tile::Tile` planes; add a simple `Image` helper (planes + dims) for tests. Stages, in `StageId` order:
1. Linearize: already done by raw-decode; provide the inverse for tests.
2. Highlight handling: simple clip at 1.0 for now, with the channel-propagation reconstruction as a second selectable mode.
3. Demosaic: bilinear and RCD (port from the public RCD algorithm; cite source) for Bayer; for X-Trans use a 3×3 mean per colour (placeholder, documented). Output RGB f32 in camera space.
4. CameraProfile stage (this is the contract's order: CameraProfile then WhiteBalance, do not change engine-api): camera RGB → XYZ using the camera matrix from raw metadata (LibRaw gives cam_xyz = XYZ→camera; invert it, normalise rows so camera white maps sensibly), → linear Rec.2020. The stage may read `WhiteBalanceSettings` as an input to pick/interpolate the matrix by illuminant later; for now a single matrix.
5. WhiteBalance stage: chromatic adaptation in the working space. Compute the scene white from the as-shot multipliers (camera-space white = 1/multipliers pushed through the matrix) or from the temperature/tint settings, then CAT16-adapt scene white → D65 with `engine_api::color::ChromaticAdaptation`. Neutral setting must render a grey card neutral (test).
6. Exposure/contrast/highlights/shadows/whites/blacks: implement in a log-ish luminance domain with the parameter ranges from `DevelopSettings` (read the struct); document the formulas in `OPERATORS.md`. Not Adobe's exact maths, but monotone, artefact-free, and neutral at zero.
7. Display transform: a sigmoid tone mapper (darktable-style, with contrast and skew parameters) from scene-linear Rec.2020 to display-linear, then sRGB OETF and 8-bit dither.
- `render(settings: &DevelopSettings, source: &CfaImage or RGB) -> Rgb8Image` entry point.
- Golden tests: for each fixture in `fixtures/raw`, render at 1/8 scale with default settings, and compare against `fixtures/golden/<name>.png` if present (max abs error ≤ 2/255, else fail). If the golden is missing, write it and print a notice (first run creates goldens; commit them, they are small). Also unit tests: WB neutralises a grey patch, exposure +1 doubles linear values before the tone stage, sigmoid maps 0→0 and is monotone.


Dependency note: `raw-decode` on main now provides real CFA data and camera matrices (M1-01); if any metadata you need is missing from `raw_decode::RawMetadata`, add it there (allowed path) rather than stubbing.
