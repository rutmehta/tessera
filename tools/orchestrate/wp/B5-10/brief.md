# WP B5-10 — Editable Type tool, Character/Paragraph inspector, 1440-pt inspector fix (Opus, Machine B; planned by GPT-6 Astra)

This brief is GPT-6 Astra's plan section for B5-10, verbatim below, plus the coordinator's notes at the end. The shared contract section ('Contracts fixed for both source editors') in astra-plan.md next to this file is mandatory; read it first.

## B5-10 — Editable Type tool + Character/Paragraph inspector

Brief: Read typography/{README.md,src/model.rs,src/layout.rs,src/geometry.rs,src/render.rs}, compositor/{TEXT_VECTOR.md,src/edit.rs,src/render/live.rs,src/resident/mod.rs,src/psd/text.rs}, tessera-ffi/document.rs and the existing Tools input/overlay/KeyRouter paths. Add document/text.rs with text_layer(layer), available_text_fonts(), add_text_layer(name,parent,index,model_json,transform), set_text_layer(layer,model_json,transform,interactive), edit_text_runs(layer,start_run,end_run,runs_json), and layout_text(model_json) returning run/UTF-8 cluster/origin/advance/angle plus line source ranges/baselines/overflow. Implement shared convert_to_pixels and narrowly scoped source-preview cancellation in document.rs. Replace the Type placeholder: click creates point text, drag creates an area, click existing text resumes editing, drag-selection/caret, resize handles, Character/Paragraph controls, explicit Apply/Esc, and source-preserving move/affine handles. An NSTextInputClient/KeyOwningControl owns composition and text commands so T/V/X/D/Q, digits, Space, Delete and Cmd-A/C/V/Z cannot become tool/layer commands. Share the font snapshot with resident and CPU renderers; keep B5-07 style fallback intact. Assign the pre-existing 1440-pt inspector clipping defect to this package: reproduce with all panels expanded, fix intrinsic sizing/scroll containment instead of widening the minimum window, and regress B5-06/07/08 panels. Point/paragraph caret rules and source limitations above are mandatory.

Tests: new crates/tessera-ffi/tests/document_text_ui.rs and apps/mac/Tests/TesseraCoreTests/DocumentTextTests.swift, plus DocumentInspectorLayoutTests.swift. Minimum 12 named Rust and 12 Swift text cases, with layout coverage at 1440 pt and adjacent widths. Cover insertion IDs/order, whole model vs run edits, insertion/deletion/mixed styles, invalid run bounds, grapheme/UTF-8/UTF-16 mapping, ligatures and bidi, marked-text cancel/commit, one-node typing group, no-op/cancel, locks and affine validation, exact conversion undo/masks/styles, CPU/resident rendering, native + PSD reopen and post-conversion stale TySh removal. Fonts in deterministic tests use the existing bundled OFL fixtures without committing more font binaries; conversion roundtrips use an installed font because conversion creates a system renderer.

Allowed: crates/tessera-ffi/src/document/text.rs; tests/document_text_ui.rs; document.rs only module/reexports, text pending state/lifecycle, generic conversion/cancel; document/render.rs only delimited font injection at every CPU/resident/fallback construction after B5-07 relinquishes it (no style-routing rewrite); crates/tessera-ffi/Cargo.toml only typography path dependency; Cargo.lock only corresponding local dependency edge. New apps/mac/Sources/{Tessera,TesseraCore}/Document/Text/**. Shared Swift hooks only in Tessera/Document/{DocumentViewport.swift,DocumentView.swift,DocumentController.swift,DocumentWorkspace.swift,PropertiesPanel.swift,Tools/DocumentTools.swift,Tools/ToolsPalette.swift,Tools/ToolOverlayView.swift}, Tessera/App/{KeyRouter.swift,AppCommands.swift}, TesseraCore/Document/Tools/EditorTools.swift (Type no longer placeholder), and backend adoption files. For the reproduced layout bug only: DocumentControls.swift, LayersPanel.swift, AdjustmentEditors.swift and Shell/ContentView.swift's document-inspector sizing, no Develop/library redesign. Common generated/docs paths below.

Conflicts: B5-07 owns render.rs style fallback, PropertiesPanel/LayersPanel/AppCommands. B5-09 owns ToolsPalette/DocumentTools/DocumentView/KeyRouter hooks and the stale-outline fix. Start isolated text work at first slot; defer hot-file edits to coordinator integration after the owner finishes. Preserve the two M5-30 live revision/kind matches; B5-10 leaves Shape reporting unchanged for B5-11.

Sol acceptance 340–359:
340. Real-engine app at 1440-pt width: expanded Properties/Layers/Channels/History fully reachable, no clipped right edges.
341. Type key/palette click and canvas click creates editable point text.
342. Drag creates area text and wraps inside its bounds.
343. Select text on canvas, insert/delete across mixed-style runs.
344. Change selected font, size, weight/italic and color; untouched runs unchanged.
345. Change tracking, leading and baseline shift with one undo per completed control edit.
346. Change alignment/indents/spacing; preserve run styles and point/area geometry.
347. Resize area box and observe wrap/overflow, no bitmap stretching.
348. At fit/100%/200% with pan/rotation, caret/selection aligns to engine glyph positions.
349. Ligature, combining-mark and non-BMP input: no split/corrupt cluster.
350. Mixed RTL/LTR caret and replacement follows visual layout without source corruption.
351. IME marked text, commit and cancel; one undo for committed composition only.
352. Type T/V/X/D/Q, digits, Space/Delete and use text clipboard/select-all: no editor shortcut leakage.
353. Apply a typing group then undo/redo: one group, exact text/style restoration.
354. Cancel a draft and switch documents: no stale caret, preview or history node.
355. Pixel/all locks reject content edits; position lock only rejects affine movement.
356. Convert styled/masked text to pixels, compare appearance, undo restores editable text.
357. Save/reopen native document; text remains editable (history is not promised persisted).
358. Save/reopen PSD; mixed text stays editable; converted layer does not resurrect TySh.
359. Missing font, vertical/path/warp caret and unsupported color-space limitations are explicit; screenshot inspector at 1440 pt again.


## Coordinator notes (Opus, Machine B)
- Base: current main (includes M5-30, B5-06, B5-08). B5-07 (Layer Style inspector; owns document/render.rs style fallback, PropertiesPanel, LayersPanel/LayersOutline, AppCommands hooks) is still running on a sibling branch; B5-09 (Remove/neural; ToolsPalette, DocumentTools, DocumentView hooks, outline-generation fix) is finished and pushed but not yet on main. Do your isolated text work first. You may then edit the hot shared files on your branch, but only inside blocks delimited `// B5-10 begin` / `// B5-10 end`, never restructuring code around another package's blocks; the coordinator resolves overlaps at merge. For render.rs, add only the delimited font-snapshot injection at each renderer construction, leaving any style-routing code untouched.
- Gate (from the worktree root): `export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-10" MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2`; `git diff --check`; `cargo test --locked --release -p typography -p compositor -p psd -p tessera-ffi`; `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `(cd apps/mac && ./build-ffi.sh && swift build --jobs 2 && swift test --jobs 2)`; `(cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-10" -jobs 2 build)`; `(cd apps/mac && Support/make-app.sh debug)`.
- ACCEPTANCE.md steps 340–359 in a new section `## AA. Type tool and text layers (B5-10)`. Common paths: apps/mac/ACCEPTANCE.md, apps/mac/DESIGN.md (additions), regenerated bindings under apps/mac/Sources/CTesseraFFI/** and TesseraFFI/** (never hand-edited), tools/orchestrate/wp/B5-10/**.
- Engine source/API changes are out of scope; write tools/orchestrate/wp/B5-10/NEEDS.md for Machine A.
