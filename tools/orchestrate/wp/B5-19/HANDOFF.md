# B5-19 handoff — Auto-Align / Auto-Blend Layers and Photomerge into layers

Branch `wp/B5-19` (Machine B), base c15dee24. Reviewed by A at af14ca25; the review fixes below are commits on
top (no rebase).

## Commits

- `test(B5-19): RED …` — brief copy, `crates/tessera-ffi/tests/document_stack_ui.rs`,
  `apps/mac/Tests/TesseraCoreTests/DocumentStackTests.swift`.
- `feat(B5-19): …` — FFI `crates/tessera-ffi/src/document/stack.rs` (+ registration block in `document.rs`),
  TesseraCore `Document/Stack/*`, app `Document/Stack/*`, menu hooks, regenerated bindings.
- `docs(B5-19): …` — this file and `ACCEPTANCE-STEPS.md` (steps 440–459; merge into apps/mac/ACCEPTANCE.md at
  integration).

## A's review of af14ca25: blocker fixes

Commits on top of af14ca25: `test(B5-19): RED review blockers …`, `fix(B5-19): bound Photomerge memory …`,
`docs(B5-19): handoff …` (this file).

1. **Memory / cancel.** `MAX_STACK_MEGAPIXELS = 200` (all layers or photos of one stack; ~50 B/source px at peak ⇒
   ~10 GB; exported as `stack_max_megapixels()`, mirrored in `StackCommandRules.maxMegapixels`). Photomerge
   resolves every source and reads its size from the header only (PNG/JPEG via `image::ImageReader::into_dimensions`,
   TIFF via the `tiff` decoder, library RAW via LibRaw open + metadata) and refuses over-budget stacks before any
   decode: "Photomerge is limited to 200 megapixels in total; these have N megapixels. Use fewer or smaller
   images." Sources whose header cannot be read (library HEIC / linear DNG) are counted right after their decode
   and refused before the next one. Auto-Align / Auto-Blend check the same budget from the layer extents (in
   `problem()`, so `stack_eligibility` reports it too). Photomerge sources are now library ids or JPEG / PNG / TIFF
   paths only (no `.tessera-doc` / PSD: no header size, and they carry layers); the sheet's Add Files… offers
   those three types.
   Cancel: `photomerge_into_layers` / `photomerge_document` take `cancel: Arc<CancelFlag>` (the existing
   export `CancelFlag`); checked before and after each source's decode (library renders also get the token via
   `io::open_image`, compositor flattening via `render_level_rgba_with_cancel`) and once more before the edit /
   new document. A cancelled call returns "Photomerge was cancelled" with nothing changed. Swift: the busy sheet
   has Cancel (`stack-busy-cancel`) for Photomerge only; Auto-Align / Auto-Blend keep the no-cancel note (engine
   ask 2).
   Tests: `photomerge_refuses_over_the_pixel_budget_from_headers_before_decoding` (3 PNGs whose IHDR claims
   10000×8000 but hold 1 px), `stack_eligibility_refuses_layers_over_the_pixel_budget` (sparse 12000×9000 layers),
   `cancelled_photomerge_changes_nothing`; Swift `testCancelledPhotomergeThrowsTheCancelMessageAndOpensNothing`.
2. **Colour profiles: convert.** Each source's composite is converted (lcms2, relative colorimetric + BPC, the
   same `io::convert` flat export uses) from its own profile (embedded ICC, untagged = sRGB, library renders =
   sRGB) into the target: the open document's profile for `photomerge_into_layers`, and for
   `photomerge_document` the first photo's profile, which the new document takes (no longer hard-coded sRGB). A
   target profile without embedded ICC bytes is refused with a clear error. Tests (synthesized 16-bit PNGs with
   an embedded Display P3 profile): `photomerge_document_takes_the_first_photos_profile_and_converts_the_rest`
   (P3 + untagged sRGB ⇒ P3 document, P3 values unchanged) and
   `photomerge_into_an_srgb_document_converts_display_p3_photos` (matches an lcms P3→sRGB conversion; fails on
   the old unconverted ingest).
3. **Reposition: withheld.** Root cause is in the engine: `merge::layers::align_prepared` routes Reposition to
   `rigid()` → `alignment::align_global`, the HDR-bracket phase correlation (±12 × 0.25° search, scored on the
   whole frame), which does not find a 140 px shift with 100 px overlap and settles near identity; the other modes
   use feature registration. Not fixable in the FFI, and `merge` is A's. `StackAlignMode::Reposition` is removed
   from the FFI enum and `StackAlignLayout` (sheets never offer it). `reposition_is_withheld_while_the_engine_
   misregisters_it` calls `merge::layers::align_layers` directly and pins the bug (Collage spans 380 px,
   Reposition < 340); when A fixes the engine it fails and says to re-enable Reposition.
   PSD layer-kind assertion restored, tightened and explained: Photomerge / Auto-Align layers are smart objects
   (the alignment transform stays editable), and `prepare_rasterized_psd_copy` rasterizes smart-filter stacks
   only, so a filter-less smart object is written as a PSD smart object and reopens as `SmartObject`. The RED
   test's `kind == Pixel` expectation was wrong; the test now asserts `SmartObject` both after native reopen and
   after the PSD round trip (plus masks and composite).

Non-blocking items:
- Swift 4-channel rule: `presentAlign` / `presentBlend` now also ask the engine (`stackEligibility`), which
  checks channels; the menu enablement itself still uses the pure rules (`LayerRecord` has no channel count).
- Source transparency is still dropped (`merge::LinearImage` is RGB-only; engine ask 7).
- Library sources merged into the *current* document are resolved on that document's engine (unchanged; a new
  Photomerge document uses the library's engine). Only differs when a document belongs to another engine than
  the library selection; left as is.

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

## Gates after the review fixes (Machine B, this worktree)

- `cargo test --locked --release -p tessera-ffi --no-fail-fast`: 437 passed, 0 failed (document_stack_ui 16/16).
  One earlier full run hit `smart_preview_thumbnail::tests::hdr_saved_offline_recipe_keeps_policy_and_renders_sdr_
  thumbnail_without_mutation` ("close the active Smart Preview editor"), unrelated to B5-19; it passed on the rerun.
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`:
  clean.
- `apps/mac/build-ffi.sh`: bindings regenerated (committed).
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (714 tests, 3 skipped, 0 failures; DocumentStackTests 11/11).

## Gates at af14ca25 (before the review fixes)

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

1. **Reposition mis-registers** (engine, `merge::layers`): two 240×180 crops 140 px apart align correctly with
   Auto / Perspective / Collage (canvas ~380 px, offset 140) but Reposition gives a 251 × 189 canvas and offset 1
   (`rigid()` → `alignment::align_global`, see blocker 3). Reposition is withheld from FFI and UI until fixed;
   `reposition_is_withheld_while_the_engine_misregisters_it` goes red when it is.
2. **Cancellation** for `align_layers` / `blend_layers` (and CAF inside `blend_document_layers`), e.g. a
   `&CancellationToken` in `DocOp::{AutoAlignLayers, AutoBlendLayers, Photomerge}`: today Photomerge cancels only
   while reading photos, Auto-Align / Auto-Blend not at all.
3. **Progress callback** for the same (align per pair, blend per level).
4. **Photomerge without blending** ("Blend Images Together" off): `DocOp::Photomerge` always blends; an
   `Option<BlendOptions>` would let the sheet offer it (the checkbox is shown on and disabled).
5. **Lens profiles → `LensCorrection`**: no library lens profile mapping exists, so the vignette / distortion
   toggles are disabled with an explanation; FFI already accepts explicit per-layer calibrations.
6. `DocumentSession::edit` holds the session lock through align / blend, so the render thread cannot present
   frames meanwhile (the modal busy sheet covers it). Computing the op outside the lock would need an engine split
   (prepare / install).
7. **Alpha in `merge::LinearImage`** (or a per-source coverage mask) so Photomerge keeps source transparency.
8. **Peak memory of align / blend**: the 200 MP budget assumes ~50 B/source px (12 B `LinearImage` + 16 B layer
   copies + history). Reusing the `LinearImage` buffers for the layers, or f16 intermediates, would allow a larger
   budget.
