# M5-30 implementation and verification

## Delivered

- Live TextModel and ShapeModel document layers, run-index edits, shape edits, vector masks and conversion to pixels, using normal atomic history/undo.
- Shared output-level geometry rasterization for CPU and resident rendering, model/transform/level source caching, and observable live-rasterization counters. Feathered vector masks include halos across tile boundaries and combine with raster masks using resident-compatible f32 arithmetic.
- Native persistence of source models, transforms and masks, including legacy text/polygon-mask migration.
- Real Adobe TySh/EngineData and shape path/fill/stroke/origination records, refreshed cached layer pixels, preservation of unknown source fields, and digest-protected supplemental native-source records.
- engine-api 1.5 additive types/actions/summaries/changelog and MCP schemas/dispatch/recorded Actions.
- `crates/compositor/TEXT_VECTOR.md` describes the host contract and limits.

## Scope

No commits or pushes. All Cargo commands use `/Volumes/betterSSD/tessera-cache/target/M5-30`.
The only FFI edit is the two exhaustive matches in `crates/tessera-ffi/src/document.rs` required by workspace checking. Shape temporarily maps to the existing Fill ABI; Machine B owns the new host ABI.

## Test-first evidence

- Vector mask regression initially rendered outside alpha 1.0 instead of 0.5.
- Model/history and wire tests initially failed on missing live layer/API operations.
- EngineData regression initially failed on missing Adobe parse/serialize APIs.
- Combined mask test reproduced a one-ULP CPU/resident-mask-arithmetic mismatch, then passed after alignment.
- Review regressions reproduced stale type resurrection after rasterization, discarded shape-mask settings, and discarded interior gradient-alpha stops; all passed after fixes.
- Exact star/affine bridge test exposed f64 JSON parsing precision; round-trip parsing fixed it.
- Live-shape parameter regression initially produced alpha 0 instead of 1 when AddShape supplied rectangle controls and an empty path. AddShape/EditShape now regenerate geometry from those controls. The regression passes and checks subsequent geometry changes and undo.

## Verification status

The parent agent personally ran the complete required command after the coding worker finished:

    cargo test -p compositor -p psd -p typography -p vector -p engine-api -p tessera-mcp --release && cargo clippy -p compositor -p psd -p typography -p vector -p engine-api -p tessera-mcp --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace

Exit code: **0**. Release tests: **486 passed, 0 failed, 10 ignored**, aggregated from 91 test-result lines. Strict clippy, formatting, workspace checking and `git diff --check` passed. Existing ignored tests were not changed to obtain this result.

The parent added and ran `crates/compositor/tests/live_text_resident.rs`, which requires a real Metal adapter rather than returning early. It compares CPU and resident samples exactly for live transformed text with a feathered vector mask, at U8/U16/F32 depths and levels 0–2, through a run edit, undo, and font-cache reset. It passed. The existing new shape/mask resident parity test also passed in this environment.

Canonical log: `tools/orchestrate/wp/M5-30/gate-final.log`.

Earlier worker-sandbox runs could not access Metal and failed HEIC decoding. Those were sandbox-specific observations, not host blockers: both classes of tests passed in the parent's direct run. An earlier overlapping-build run reached successful unit/integration tests but failed rustdoc dependency resolution; the final serialized gate above passed, including doc tests. `gate-parent.log` retains that earlier attempt for diagnosis.

## Remaining interoperability/host limits

See TEXT_VECTOR.md for details: system-font ownership for conversion/export; no automatic per-layer ICC conversion of text colors; no native GPU curve rasterizer; 1024-output-pixel feather halo limit; exact native PSD models supplement standard descriptors. Nonuniform strokes may differ when another application rerenders the standard PSD descriptor. Stroked Adobe shapes with nondefault primary-mask settings retain their cached pixels with a warning. Pattern shape fill export is unsupported. These are explicit limits rather than silently discarded source data.
