# B5-19 — Auto-Align / Auto-Blend / Photomerge into layers (acceptance steps 440–459)

Base: origin/main aec3738e. ID unused (see B5-18 brief).

## Engine readiness: READY NOW (Machine A optional follow-ups only)
- `crates/merge/src/layers.rs`: `align_layers(&[LinearImage], &AlignOptions)` (modes Auto/Perspective/Cylindrical/Spherical/Collage/Reposition; reference, seed, vignette_removal, geometric_distortion, per-input `LensCorrection`), `blend_layers` (`BlendOptions`: Panorama/StackImages, seamless_tones, fill_transparent, pyramid_levels, seed).
- `crates/compositor/src/edit.rs`: `DocOp::AutoAlignLayers{ids,options}`, `DocOp::AutoBlendLayers{ids,options,fill}`, `DocOp::Photomerge{images:Vec<(String,LinearImage)>,align,blend,fill}` — one atomic history node, extends canvas, retains sources, transform stages and editable masks. `fill: Option<ContentAwareFill>` = `fn(&Raster,&[f32],u64)->EngineResult<Raster>`; FFI must supply a non-capturing adapter over `filters::caf::fill` (pattern: `crates/tessera-ffi/src/merge.rs:552`).
- Tests on main: `compositor/tests/photomerge.rs` (source/transform/mask retention + atomic history; layered PSD export keeps masks, rasterizes transforms), `photomerge_panorama.rs` (fixed hang; passes in 0.23 s per MACHINE-A.md), `merge_validation.rs`, `lens_merge*.rs`.
- Decode: `tessera-ffi/src/merge.rs::load_linear(&PhotoSource)` is `pub(crate)` → reusable from document/stack.rs for library images.
Constraints to surface in UI: only ROOT-LEVEL RGBA PIXEL layers (`"alignment currently requires pixel layers"`); 1..128 images; `AlignOptions`/`BlendOptions` are not serde (FFI defines uniffi mirror records).
Optional Machine A requests (NOT blocking; NEEDS.md): (a) cancellation token through `align_layers`/`blend_layers` (currently uncancellable — long panoramas can only be abandoned, UI must show indeterminate busy and not claim Cancel); (b) progress callback.

## FFI missing (B creates `crates/tessera-ffi/src/document/stack.rs`)
- Records: `StackAlignOptions{mode, reference_index, vignette_removal, geometric_distortion, seed}`, `StackBlendOptions{mode: Panorama|StackImages, seamless_tones, content_aware_fill, seed}`.
- `DocumentSession::auto_align_layers(ids, options) -> DocumentUpdate`; `auto_blend_layers(ids, options) -> DocumentUpdate`; both via `self.edit(DocOp::…, label)`; lock/kind/nesting validation before edit (nothing changes on error).
- `DocumentSession::photomerge_into_layers(sources: Vec<String> /*image ids or paths*/, align, blend)` for the open doc; and `#[uniffi::export] impl Engine { fn photomerge_document(sources, align, blend) -> Arc<DocumentSession> }` (new Untitled doc; needs `register_document` visibility from document.rs — registration-only hook).
- lens corrections: out of scope unless trivially mapped from library lens profiles (otherwise geometric_distortion requires explicit calibration → disable toggle with explanation).
- Registration: `document.rs` `mod stack;` + reexports only.

## Swift files
Create: `apps/mac/Sources/TesseraCore/Document/Stack/DocumentStackBackend.swift` (+ Engine/Stub adoption extensions `EngineDocumentBackend+Stack.swift`, `StubDocumentBackend+Stack.swift`), `StackOptions.swift` (pure model; may reuse `TesseraCore/Photo/PhotoMergeSettings.swift` read-only); `apps/mac/Sources/Tessera/Document/Stack/AutoAlignSheet.swift`, `AutoBlendSheet.swift`, `PhotomergeSheet.swift` (source picker from library selection / files, layout mode, blend/vignette/CAF toggles), `StackSelfTest.swift`.
Hooks: `App/AppCommands.swift` (Edit ▸ Auto-Align Layers…, Auto-Blend Layers…; File ▸ Automate ▸ Photomerge…), menu enablement from Layers selection (≥2 root pixel layers). Do not edit `Photo/PhotoMergeSheet.swift` (library merge, Machine A/Photo tree).

## Tests first (RED)
- `crates/tessera-ffi/tests/document_stack_ui.rs`: two overlapping crops of one fixture align to known offset (±1 px) and canvas grows; ids not root/pixel/locked → error, history unchanged; auto-blend Panorama creates masks on each layer (sum≈1 in overlap) and one undo restores; StackImages picks sharper source per region; CAF fill fills transparent corners only when requested; photomerge_into_layers = exactly one history node with N named layers; photomerge_document returns Untitled doc with N layers; native save/reopen and layered PSD export keep masks.
- `apps/mac/Tests/TesseraCoreTests/DocumentStackTests.swift`: option mapping, menu enablement rules, stub backend behaviour.

## Gates
    cargo test --locked --release -p merge -p compositor --test photomerge --test photomerge_panorama --test merge_validation
    cargo test --locked --release -p tessera-ffi ; cargo test --locked --release -p tessera-ffi --test document_stack_ui -- --nocapture
    cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings ; cargo fmt --all -- --check
    bash tools/orchestrate/swift-gate.sh ; (cd apps/mac && swift test --jobs 2 --filter DocumentStackTests)
    xcodebuild build ; make-app.sh debug ; codesign --verify --deep --strict
Watch for a hang like the historical photomerge_panorama stall: run with a timeout and sample on no progress.

## Parallelism
Fully parallel with B5-18 and B5-20: new stack.rs + new Swift Stack dirs. Shared only: document.rs registration line, AppCommands.swift hook, generated bindings, ACCEPTANCE.md — serialize at integration and regenerate bindings from combined Rust source. No overlap with B5-17a/b, B5-12b, B5-13, B5-15 files (except document.rs registration).
