# B5-18 — Camera Raw Filter sheet (acceptance steps 420–439)

Base: origin/main aec3738e. ID check: board.json uses B5-01…B5-14 (+b/c/v variants); B5-15/16/17 are live in worktrees; B5-18/19/20 are unused anywhere (board, wp/, branches, worktrees).

## Engine readiness: READY NOW (no Machine A work required)
- `crates/filters/src/camera_raw.rs` (+ `camera_raw_gpu.rs`): Develop renderer over document-linear RGBA. Params `{"settings": DevelopSettings, "amount": 0..1}` (`deny_unknown_fields` on outer object; `DevelopSettings` is `#[serde(default)]`, so `{"settings":{}}` = neutral). Validates every tone/color/detail/WB key against `CrsKey` domains and `pipeline_cpu::validate_settings`. AI mask components -> explicit `Unsupported`. Needs document ICC in `FilterContext` (missing ICC is rejected). Level-aware (renders the given filter level). Tests: `crates/filters/tests/camera_raw.rs` (goldens sRGB/P3, domains, AI-mask refusal, stack edit/undo/native round-trip, HDR parity), `camera_raw_gpu.rs`.
- `compositor_adapter.rs` routes `camera_raw` CPU and GPU (`camera_raw_gpu::supports`).
- engine-api `DocumentToolCall::CameraRawFilter` / action `camera_raw_filter`; MCP `documents/filters.rs` already calls it.

## FFI: already exposed (document/filters.rs, shared hotspot)
- `RasterFilterOperation::CameraRaw` + `apply_raster_filter`, or `apply_filter(layer, {"id":"camera_raw","params":…})` — one history node; destructive on pixel layers (inside selection), appended smart filter on smart objects.
- `preview_filter`, `preview_smart_filter` (re-edit), `filter_detail`, `cancel_filter`, `smart_filters`/`set_smart_filter` all accept it (adapter id).
Known limitations to design around / record:
1. `camera_raw` is an `adapter_id`, so `full_resolution()` forces every preview to level 0 over the WHOLE canvas through the CPU `Spec::run` path (`submit_preview` ~L2438, apply pre-validation ~L2628). Slider-drag previews on 24 MP layers will be seconds. Optional narrow fix (B-side, document/filters.rs): exclude `camera_raw` from `full_resolution`/`Spec::run`'s level-0 gate since the renderer is level-aware — only after B5-13 and B5-15 (both edit filters.rs, unmerged) land; otherwise keep FFI untouched and coalesce previews latest-wins.
2. Smart object + active selection -> "retouch smart filters require no active selection" error (adapter rule). Sheet must disable Smart apply with a selection or explain.
3. No FFI accessor for a neutral/default settings JSON or a slider schema — not needed: Swift reuses `TesseraCore/Develop/DevelopControls.swift` (`DevelopControl.path/range/default/patch`) and sends `{"settings":{…patched…},"amount":…}`.
4. No AI masks, no local masks UI in scope (engine refuses AI masks; brush/gradient locals out of scope).

## Files
Create:
- `apps/mac/Sources/TesseraCore/Document/CameraRaw/CameraRawDraft.swift` — draft settings dict built from DevelopControl patches, amount, JSON encode/decode (re-edit parses existing smart filter `filter_json`), latest-wins preview token.
- `apps/mac/Sources/Tessera/Document/CameraRaw/CameraRawSheet.swift` (+ `CameraRawSelfTest.swift`) — sheet: Basic (WB temp/tint, exposure…blacks, texture/clarity/dehaze, vibrance/saturation), Curve (parametric), HSL, Color Grading, Detail, Effects; before/after, zoom-fit/1:1 detail pane via `filterDetail`, Amount, Cancel/OK; busy + cancel; error from `filterError()`.
Touch (small delimited hooks): `Tessera/Document/Filters/FilterMenus.swift` (Filter ▸ Camera Raw Filter… ⇧⌘A), `Filters/SmartFilterRows.swift` (double-click camera_raw row opens sheet in re-edit), `App/AppCommands.swift`/`KeyRouter.swift` only if the shortcut needs routing. Reuse existing `DocumentFiltersBackend` (no protocol change needed). Optional: `crates/tessera-ffi/src/document/filters.rs` narrow level fix (see 1).
Do NOT touch: retouch.rs, channels.rs (B5-17a/b), Inspector/DevelopPanels.swift (Machine A Develop UI; reuse components read-only; if a panel view must be generalized, raise NEEDS.md).

## Tests first (RED)
- `crates/tessera-ffi/tests/document_camera_raw_ui.rs`: neutral `{"settings":{}}` is identity within tolerance; exposure +1 doubles linear; out-of-domain value / unknown outer key rejected with no history; pixel layer apply = one undoable node; smart object apply appends one `camera_raw` smart filter, re-edit via `preview_smart_filter`+`set_smart_filter(Params)` replaces (no duplicate); selection+smart object refused; cancel leaves history unchanged; native save/reopen round-trip of the smart filter; AI-mask component refused.
- `apps/mac/Tests/TesseraCoreTests/DocumentCameraRawTests.swift`: draft JSON patches match `DevelopControl.path`, clamping, re-edit parse round-trip, amount encoding, reset-to-neutral, latest-wins token.
- Self-test screenshots for steps 420–439 (sheet open, each panel, before/after, re-edit, cancel, 1440-pt layout).

## Gates
    WP=B5-18; WT=$ROOT/.worktrees/$WP; export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/$WP MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0
    cargo test --locked --release -p filters --test camera_raw --test camera_raw_gpu
    cargo test --locked --release -p tessera-ffi
    cargo test --locked --release -p tessera-ffi --test document_camera_raw_ui -- --nocapture
    cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings
    cargo fmt --all -- --check
    bash tools/orchestrate/swift-gate.sh          # needs fixtures/raw symlink
    (cd apps/mac && swift test --jobs 2 --filter DocumentCameraRawTests)
    xcodebuild … build; apps/mac/Support/make-app.sh debug; codesign --verify --deep --strict
Record feature-test counts; Metal skips = unverified GPU.

## Parallelism
Disjoint from B5-19 and B5-20 except shared hotspots (FilterMenus.swift also hooked by B5-20; generated bindings; ACCEPTANCE.md). No FFI API change if limitation 1 is deferred, so no binding regeneration conflict. Conflicts with UNMERGED B5-13 (FilterMenus, SmartFilterRows, filters.rs) and B5-15 (filters.rs): start Swift work now, integrate after those merge. No overlap with B5-17a (retouch) / B5-17b (channels).
