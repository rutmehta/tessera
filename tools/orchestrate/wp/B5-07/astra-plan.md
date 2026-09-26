# Machine B next wave implementation plan

Goal: expose already integrated layered-editor functionality without competing with Machine A's M5-29 engine/FFI work. Planning only; no jobs launched or implementation gates executed.

## Dispatch and fences

- B5-06 extended adjustments and B5-07 layer styles run in parallel. Start B5-08 channels when either frees a slot. Default to two Opus leaves; a third is optional only after a disk/build-cache check. Every leaf has its own worktree and `wp/B5-NN` branch. Machine B pushes branches only; Machine A merges, preferably 06, 07, 08, 09.
- B5-09 Remove/neural UI starts only after M5-29 merges and its actual generated Swift signatures are inspected. It may overlap remaining B5-08 isolated-file work, not its shared tool integration.
- Do not wait for the entire B5-v report to begin isolated models/views/tests. Before shared-shell integration and final acceptance, triage its findings. Crash, persistence/data loss, blank viewport, broken undo or coordinate mapping takes precedence; have A land the relevant fix, then rebase. Cosmetic findings need not stop independent work. One Sol verification session at a time, after each integration batch.
- M5-29's broad allow-list includes every `document/**` file and FFI test. Reserve the NEW leaf files below with A explicitly. Until M5-29 is merged/frozen, do not edit its existing document.rs, document/{filters,tools,render,io}.rs, lib.rs, Cargo.toml, Cargo.lock or generated bindings. Independent Swift/new-module implementation can proceed; registration, generated ABI and final linked gates wait. Do not assume a broad allow-list means those files are actually disjoint.
- Coordinator integrates small `// BEGIN B5-NN` / `// END B5-NN` delegation/registration blocks, one package at a time. Keep feature implementation in the named new directories. Follow existing separate `DocumentToolsBackend`/`DocumentFiltersBackend` protocol + EngineDocumentBackend extension pattern. Do not enlarge the central DocumentBackend protocol. Rebase later branches on updated A main and regenerate bindings, never hand-merge generated Swift/C headers.
- Shared files are not a blanket feature-edit permission. Only the explicitly named hooks below, plus module registration in `crates/tessera-ffi/src/document.rs`, may be changed. `lib.rs` already has `pub use document::*`; no edit needed. None of these packages needs engine-api/MCP changes, a new dependency, Cargo.lock edits, export changes, or an engine-crate rewrite. If an engine prerequisite is discovered, report it to A rather than working around it with fake UI state.
- With roughly 15 GB free, serialize the entire heavy gate including archive copy/binding generation. Use one external Cargo target cache for this wave, `CARGO_INCREMENTAL=0`, and no overlapping builds using that cache. Swift scratch output stays in each worktree; Xcode gets per-package derived data. Keep only active build products; remove only completed task-owned build caches after logs/evidence are saved, never source/user caches. If a cold build cannot fit, use verified external storage or pause, not four independent Cargo targets.

## Common scope and contracts

For each package, allow its `tools/orchestrate/wp/B5-NN/**` (brief, acceptance, fixtures, results and screenshot evidence), its named FFI/test files, and its two new Swift directories. Generated-only changes are allowed in `apps/mac/Sources/CTesseraFFI/**` and `apps/mac/Sources/TesseraFFI/**` during the serialized post-M5-29 integration. No edits to `tools/orchestrate/board.json`, STATUS, engine crates, or unrelated app files by leaves.

All new APIs listed below are proposed DocumentSession additions, not claims that they exist today. Reuse existing return types (`DocumentUpdate`, errors) and existing compositor JSON formats. No pixels across UniFFI: bounded thumbnails use surface handles. Mutations use DocumentSession's checked document/history path, not direct mutable document state. Preserve unrelated properties, IDs and source pixels. Interactive edits commit one history node per gesture; cancellation leaves document/history unchanged. Capture document identity/history head for asynchronous preparation and reject stale results. Disable conflicting tools while a preview/job owns the session.

Acceptance is engine-backed, never a stub-only pass. Each package adds Rust integration coverage and Swift bridge/controller tests, tests invalid inputs and persistence, uses Theme tokens/accessibility IDs, builds on Machine B's Xcode 26.3 / Swift 6.2.4, and provides numbered Sol steps with screenshots and a per-step verdict. Screenshot verification uses one session and copied fixtures/temporary app data, not the owner's library.

## B5-06 — Complete adjustment layers and Image > Adjustments

Read `crates/compositor/src/adjust.rs`, `adjust/{statistics,lookup,icc,presets,shadows,hdr}.rs`, `crates/tessera-ffi/src/document.rs` and `document/filters.rs`, and `apps/mac/Sources/TesseraCore/Document/DocumentAdjustments.swift`. Expose Brightness/Contrast, Vibrance, Color Balance, Black & White, Photo Filter, Gradient Map, Selective Color, Desaturate, Equalize, Auto Tone/Contrast/Color, Match Color, Replace Color, Color Lookup, Shadows/Highlights and HDR Toning in both adjustment-layer properties/creation and destructive adjustment sheets. Reuse `add_layer(NewLayer::Adjustment)`, `set_adjustment_json`, `preview_adjustment`, `clear_preview`, `apply_adjustment`, and `commit`. New `adjustments.rs` holds only bridge preparation: `prepare_auto_adjustment`, `prepare_equalize_adjustment`, `prepare_match_color_adjustment`, `load_color_lookup_adjustment`, returning canonical adjustment JSON through the engine constructors; add HDR histogram preparation if the chosen method requires it. Compute/freeze real histograms and Lab statistics, keep LUT contents embedded for reopen, explicitly convert tagged Match Color samples to sRGB instead of misusing the untagged-only constructor, and reject unsupported sources. Test every exposed kind, frozen analysis after source edits, malformed LUTs/JSON, one-node drags, cancel and reopened values. Sol creates Vibrance, tests source-selected Match Color and a loaded LUT, previews/cancels a destructive adjustment, applies Shadows/Highlights and HDR Toning, undoes/redoes, saves/reopens native and checks supported PSD variants. HDR Toning remains native-only with an explicit PSD limitation, not silent flattening.

Allowed new paths:
- `crates/tessera-ffi/src/document/adjustments.rs`
- `crates/tessera-ffi/tests/document_adjustments_ui.rs`
- `apps/mac/Sources/Tessera/Document/Adjustments/**`
- `apps/mac/Sources/TesseraCore/Document/Adjustments/**`
- `apps/mac/Tests/TesseraCoreTests/DocumentAdjustmentsExtendedTests.swift`

Coordinator-only hooks: `TesseraCore/Document/DocumentAdjustments.swift` dispatch to new models; `Tessera/Document/PropertiesPanel.swift` editor dispatch; `LayersPanel.swift` and `App/AppCommands.swift` creation menu; `DocumentView.swift` sheet attachment; `Document/Filters/FilterMenus.swift` Image menu only. Those Swift paths are relative to `apps/mac/Sources/`. Keep the old model/stub behavior unchanged by using a separate extended-adjustment backend/model, not copying the engine into StubCompositor. Add a single controller JSON delegation only if required in `Tessera/Document/DocumentController.swift`.

Conflicts: module registration/generated bindings with M5-29 and B5-07/08; PropertiesPanel with B5-07; DocumentView/AppCommands and FilterMenus with B5-09. Non-overlapping delimited hook blocks, integrated serially. No Camera Raw filter work here.

Gate, from this package's worktree root:

    export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-next"
    export CARGO_INCREMENTAL=0
    cargo test -p compositor -p color-mgmt -p psd -p tessera-ffi --release && cargo clippy -p compositor -p color-mgmt -p psd -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace && (cd apps/mac && ./build-ffi.sh && swift build && swift test) && (cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-06" build)

## B5-07 — Layer Style inspector and shared lighting

Read `crates/compositor/src/document.rs` (`LayerProps.styles`), `render/styles.rs`, `edit.rs` (`SetProps`, `SetGlobalLight`), `blend.rs`, `psd.rs`, `tests/style_psd.rs`, and DocumentSession's `edit_props`/history implementation. Add proposed `layer_styles_json`, `set_layer_styles_json(layer,json,interactive)`, `global_light`, `set_global_light` calls in `styles.rs`; use `edit_props` so changing effects cannot wipe masks, blend settings or other properties. Build an fx inspector with add/remove/enable/duplicate effects, shadows/glows, bevel, satin, stroke, color/gradient/pattern overlays, scale and shared light. Preserve the engine's effect-rank ordering rather than suggesting arbitrary stack reordering. Preserve but do not offer functioning controls for metadata-only contour/jitter. Preview supported controls on the existing scratch path, commit one history node on release; avoid a cancelable modal that would require rewriting M5-29 preview machinery. Test every supported effect type, repeated effects, scale/light invalidation, unrelated-prop preservation, lock handling, one-node gestures and native/PSD round trips. Sol applies a shadow and stroke, sets fill opacity to zero (effects remain), edits shared light on two styled layers, undoes/redoes, and saves/reopens.

Allowed new paths:
- `crates/tessera-ffi/src/document/styles.rs`
- `crates/tessera-ffi/tests/document_styles_ui.rs`
- `apps/mac/Sources/Tessera/Document/Styles/**`
- `apps/mac/Sources/TesseraCore/Document/Styles/**`
- `apps/mac/Tests/TesseraCoreTests/DocumentStylesTests.swift`

Coordinator-only hooks: `apps/mac/Sources/Tessera/Document/PropertiesPanel.swift` fx inspector mount. No need to change LayerNode/LayerPropsRecord or make sweeping outline changes: query styles via the side protocol.

Conflicts: shared module/generated fence with M5-29 and B5-06/08; PropertiesPanel mount with B5-06. New implementation files otherwise disjoint. Global light can initially commit as a single explicit Apply operation rather than adding new pending-edit state to document.rs.

Gate:

    export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-next"
    export CARGO_INCREMENTAL=0
    cargo test -p compositor -p psd -p tessera-ffi --release && cargo clippy -p compositor -p psd -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace && (cd apps/mac && ./build-ffi.sh && swift build && swift test) && (cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-07" build)

## B5-08 — Persistent alpha/spot Channels panel

Read `crates/compositor/src/{channels,edit,format,psd}.rs`, `crates/selection`, and `crates/tessera-ffi/src/document/tools.rs:2060-2094`. Existing FFI saved selections are only a session-local name map, despite persistent document channels existing in the engine. Add a new ID-based `channels.rs` facade: proposed `document_channels`, `save_selection_channel`, `load_selection_channel(id,op)`, `rename_document_channel`, `delete_document_channel`, `set_spot_channel`, `channel_thumbnail`. Drive `AddChannel`, `RenameChannel`, `DeleteChannel`, `EditChannel` and `SetSelection`, combining selections with existing selection semantics. Make the app's Save/Load Selection delegate to persistent IDs, allowing duplicate names without ambiguity; do not maintain two app-visible channel stores or rewrite the existing Rust tools module. UI lists alpha/spot channels with bounded thumbnails, rename/delete, selection save/load and spot color/solidity. Spot color is labeled preview metadata; it must not pretend to affect the RGB composite. Do not add a full isolated-channel canvas compositor this wave. Test create/rename/delete/load history, add/subtract/intersect, duplicate names, stale IDs, dimensions/invalid metadata and both native/PSD reopen. Sol saves a marquee, deselects, closes/reopens, loads it, creates a spot channel, changes metadata, undoes and verifies PSD persistence with unchanged RGB output.

Allowed new paths:
- `crates/tessera-ffi/src/document/channels.rs`
- `crates/tessera-ffi/tests/document_channels_ui.rs`
- `apps/mac/Sources/Tessera/Document/Channels/**`
- `apps/mac/Sources/TesseraCore/Document/Channels/**`
- `apps/mac/Tests/TesseraCoreTests/DocumentChannelsTests.swift`

Coordinator-only hooks: `apps/mac/Sources/Tessera/Document/DocumentView.swift` Channels section; `Document/Tools/ToolsMenus.swift` and `Document/Tools/ToolsSheets.swift` persistent Save/Load delegation. Make `apply_selection` `pub(super)` only if required, as a one-line change in Rust `document/tools.rs` AFTER M5-29 merges; no other changes to that file. Existing legacy FFI calls remain ABI-compatible and app-inactive.

Conflicts: M5-29 owns tools.rs and generated/root FFI files, so any helper visibility change waits. Channels/tool menu hooks with B5-09 and DocumentView with B5-06. No selection/brush engine changes.

Gate:

    export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-next"
    export CARGO_INCREMENTAL=0
    cargo test -p compositor -p selection -p psd -p tessera-ffi --release && cargo clippy -p compositor -p selection -p psd -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace && (cd apps/mac && ./build-ffi.sh && swift build && swift test) && (cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-08" build)

## B5-09 — Remove and neural filters UI over merged M5-29

Hard prerequisite: M5-29 merged and its real DocumentSession methods/records inspected; do not invent a competing FFI contract or infer generated method names from MCP tools. Read its changed `crates/tessera-ffi/src/document*.rs` files, generated `TesseraFFI.swift`, `crates/filters/src/{remove,distraction,compositor_adapter}.rs`, `crates/ml-filters` catalog/trait, and existing Swift filter/tool backends. Consume the delivered remove_object/remove_distractions operations and the exact `neural/skin_smoothing`, `neural/colorize`, `neural/jpeg_artifact_removal` filters. Add a Remove mask mode (reuse selection geometry, do not paint source pixels), explicit PatchMatch/LaMa/Auto choice, busy/cancel/result handling, and neural parameter sheets with smart-filter versus destructive destination. Show the actual backend/fallback and missing-weight Unsupported errors; no implicit downloads, no pretend neural success, and no inference on the main actor. Expose distraction suggestions only if the delivered API supports preview/review before application; otherwise keep that control disabled with an honest explanation and escalate the missing contract to A. The current detector is a geometric/face-box suggestion hook, not semantic wire/person segmentation. Rust smoke tests exercise only published M5-29 APIs on deterministic PatchMatch and weights-missing paths; Swift tests cover dispatch, head/target changes during jobs, cancellation/errors, destination selection and single-step undo. Sol removes a fixture object with PatchMatch, verifies undo/redo and reopen, cancels another job, and exercises each neural missing-weight message. Test successful neural output only with approved weights already present; report it as unverified otherwise.

Allowed paths:
- `apps/mac/Sources/Tessera/Document/Retouch/**`
- `apps/mac/Sources/TesseraCore/Document/Retouch/**`
- `apps/mac/Tests/TesseraCoreTests/DocumentRetouchTests.swift`
- `crates/tessera-ffi/tests/document_retouch_ui.rs` (new smoke test only)
- No new or edited Rust FFI implementation: Machine A owns it.

Coordinator-only hooks: `apps/mac/Sources/Tessera/Document/DocumentView.swift` overlay/sheet attachment; `Document/Tools/ToolsPalette.swift` Remove activation; `Document/Tools/DocumentTools.swift` event delegation only if selection reuse needs it; `Document/Filters/FilterMenus.swift` Filter submenu only; `Document/Filters/SmartFilterRows.swift` edit dispatch for new neural IDs only. No wholesale rewrite of the existing tools or filter state machine.

Conflicts: depends on M5-29 instead of overlapping its implementation. Serial tool-shell integration after B5-08. FilterMenus has a distinct section from B5-06 but is still coordinator-only. If actual M5-29 API lacks cancellation, stale-target protection or required destination semantics, block that feature for A; do not silently substitute undo-as-cancel.

Gate:

    export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-next"
    export CARGO_INCREMENTAL=0
    cargo test -p filters -p ml-filters -p tessera-ffi --release && cargo clippy -p filters -p ml-filters -p tessera-ffi --all-targets -- -D warnings && cargo fmt --check && cargo check --workspace && (cd apps/mac && ./build-ffi.sh && swift build && swift test) && (cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-09" build)

## Deliberately deferred

Text/vector are not merely awaiting Swift controls: `typography/README.md` says the engine is independent of compositor and changed no compositor/engine-api files; `vector/README.md`/NEEDS.md describe standalone geometry, and `compositor/src/document.rs` still calls VectorMask a non-rendered placeholder. Request an A-owned document integration contract first, covering editable persistence, native history ops, font/source retention and rendering, rather than disguising raster proxies as editable vector layers. No engine follow-up is launched by this planning-only response.

Advanced transforms, Liquify/CAF/move, Camera Raw panels, Photomerge and AWA/Vanishing Point are later UI waves. M5-29 delivering their API does not require packing all their interaction designs into B5-09. Prioritize real on-screen and persistence completeness over a menu of unfinished tools.

## Handoff

After each branch is rebased, regenerated, gated and reviewed by Opus, push only `wp/B5-NN` and report the commit, exact gate logs, allowed-path diff and numbered Sol evidence to Machine A. Machine A alone merges to main. A successful compile is not a Sol acceptance verdict; report unavailable/manual steps explicitly.
