# B5-19 handoff — Auto-Align / Auto-Blend Layers and Photomerge into layers

Branch `wp/B5-19` (Machine B), based on origin/main aec3738e.

## Commits

- `test(B5-19): RED …` — brief copy, `crates/tessera-ffi/tests/document_stack_ui.rs`,
  `apps/mac/Tests/TesseraCoreTests/DocumentStackTests.swift`.
- `feat(B5-19): …` — FFI `crates/tessera-ffi/src/document/stack.rs` (+ registration block in `document.rs`),
  TesseraCore `Document/Stack/*`, app `Document/Stack/*`, menu hooks, regenerated bindings.
- `docs(B5-19): …` — this file and `ACCEPTANCE-STEPS.md` (steps 440–459; merge into apps/mac/ACCEPTANCE.md at
  integration).

## What landed

FFI (`document/stack.rs`, all uniffi-exported):
- Records `StackAlignOptions{mode, reference_index, vignette_removal, geometric_distortion, lens_corrections, seed}`,
  `StackBlendOptions{mode: Panorama|StackImages, seamless_tones, content_aware_fill, seed}`, `StackLensCorrection`,
  `StackEligibility`; `default_stack_align_options()`, `default_stack_blend_options()`.
- `DocumentSession::{stack_eligibility, auto_align_layers, auto_blend_layers, photomerge_into_layers}` and
  `Engine::photomerge_document` (new "Untitled" doc: "New Document" origin + one "Photomerge" node).
- Validation before the edit (2..128 unique top-level, unlocked, unclipped layers; RGBA pixel for align, pixel or
  smart object for blend; reference index; calibration count) with sheet-ready messages; engine re-validates
  atomically. CAF adapter = non-capturing fn over `filters::caf::fill` with the op's seed.
- Photomerge sources: a 32-hex string is a library image id (rendered developed via `io::open_image`), anything
  else a path (`io::open_path`: tessera-doc/PSD/JPEG/PNG/TIFF). Each source's composite goes in as-is (document
  encoding, same as Auto-Align reads existing layers) — not `merge.rs::load_linear`, whose camera-space linear
  RGB would not match the document's colours. RAW files by path are therefore not accepted (library ids are).

Swift: `TesseraCore/Document/Stack/{StackOptions, DocumentStackBackend, EngineDocumentBackend+Stack,
StubDocumentBackend+Stack}.swift`; `Tessera/Document/Stack/{DocumentStack, StackMenus, AutoAlignSheet,
AutoBlendSheet, PhotomergeLayersSheet, StackSelfTest}.swift`. `PhotomergeSheet` lives in
`PhotomergeLayersSheet.swift`: a file named `PhotomergeSheet.swift` collides with `Photo/PhotoMergeSheet.swift`
on the case-insensitive FS (same object file, link failure). Shared hooks (one line each): `AppCommands.swift`
(File menu `StackFileMenuItems`, Edit menu `StackEditMenuItems`), `Shell/ContentView.swift` (`.stackSheets(model)`).

## Gates (Machine B, this worktree)

- `cargo test --locked --release -p tessera-ffi --test document_stack_ui`: 10/10 pass (0.9 s; no hang).
- `cargo test --locked --release -p tessera-ffi`: 431 passed, 0 failed.
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`: clean.
- `swift test --filter DocumentStackTests`: 10/10 pass.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE FAILED** on exactly one test, 713 executed:
  `DevelopRecoveryAdmissionBehaviorTests.testSupersededFolderCallbackSettlesOnceAndMayReenterOpenFolder`
  ("Condition did not settle", line 521). It fails deterministically (3/3 runs), and it fails the same way on a
  clean origin/main aec3738e worktree, so B5-19 did not cause it. Every other test passes, including all 10
  DocumentStackTests.
- Not run: `xcodebuild build`, `make-app.sh debug`, `codesign --verify` (integration), on-screen checks.

## Remaining on-screen checks

ACCEPTANCE-STEPS.md 440–459, in particular: sheet layout and busy sheet (443), Photomerge of real photos
(441–448), selection-rule disabling in the Edit menu (449), seam quality (451, 453), focus stacking on real
brackets (454), PSD copy in Photoshop (457), the self-test (459, `--nonactivating`).

## Findings / asks for Machine A (NEEDS.md, none blocking)

1. **Reposition mis-registers** (engine): two 240×180 crops 140 px apart (`document_stack_ui.rs` texture) align to
   the right offset with Auto / Perspective / Collage (canvas 380–381 × 180–182, offset 140) but Reposition gives a
   251 × 189 canvas and offset 1. Tests use Collage; Reposition is exposed as-is.
2. **Cancellation** for `align_layers` / `blend_layers` (and CAF inside `blend_document_layers`): the UI shows an
   indeterminate, uncancellable busy sheet. A token would allow a Cancel button.
3. **Progress callback** for the same (align per pair, blend per level).
4. **Photomerge without blending** ("Blend Images Together" off): `DocOp::Photomerge` always blends; an
   `Option<BlendOptions>` would let the sheet offer it (the checkbox is shown on and disabled).
5. **Lens profiles → `LensCorrection`**: no library lens profile mapping exists, so the vignette / distortion
   toggles are disabled with an explanation; FFI already accepts explicit per-layer calibrations.
6. `DocumentSession::edit` holds the session lock through align / blend, so the render thread cannot present
   frames meanwhile (the modal busy sheet covers it). Computing the op outside the lock would need an engine split
   (prepare / install).
