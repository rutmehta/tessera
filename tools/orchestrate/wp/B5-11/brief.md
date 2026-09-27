# WP B5-11 — Live shapes, Pen/Direct Selection and vector masks (Opus, Machine B; planned by GPT-6 Astra)

This brief is GPT-6 Astra's plan section for B5-11, verbatim below, plus the coordinator's notes at the end. The shared contract section ('Contracts fixed for both source editors') in astra-plan.md next to this file is mandatory; read it first.

## B5-11 — Live shapes, Pen/Direct Selection + vector masks

Brief: Read vector/{README.md,src/lib.rs,src/geometry.rs,src/edit.rs,src/stroke.rs,src/fill.rs,src/transform.rs}, compositor/{TEXT_VECTOR.md,src/edit.rs,src/text_vector.rs,src/psd/vector.rs}, and B5-10 source-preview contract. Add document/vector.rs: shape_layer, add_shape_layer, set_shape_layer(interactive), shape_hit_test(document_point,include_stroke), edit_shape_path(command_json,interactive), boolean_shape_paths, vector_mask, set_vector_mask(interactive). Delegate geometry to Path editing/boolean helpers; inverse affine for filled/stroked hits, Stroke::outline for stroke, honor fill rule; do not call this visible-alpha hit testing. Add DocLayerKind::Shape and Swift LayerKindTag.shape, repair shape-as-Fill dispatch and keep source content_rev thumbnail semantics. Canvas supports Rectangle/rounded rectangle, Ellipse, Polygon/Star, Line; Pen click/drag/close, direct anchor/handle drag, insert/delete anchor, and shape affine handles. Inspector edits live parameters, fill/stroke/dash/cap/join/alignment and mask enabled/density/feather. Arbitrary path/boolean edits clear live_shape so Add/EditShape cannot regenerate over them. Masks stay in document coordinates on source movement; an explicit linked-vector-mask gesture submits EditShape + transformed SetVectorMask in ONE Batch. Use B5-10 conversion/cancel plumbing. Solid/gradient fills are editable; preserve imported pattern paints but explicitly flag the engine's missing PSD pattern-fill export, never claim lossless export.

Tests: new document_vector_ui.rs and DocumentVectorTests.swift, at least 12 named cases in each. Check real Shape summary dispatch, all live primitives/regeneration, custom path persistence, inverse-affine hits with skew/translation, dash-only stroke hits, even-odd holes, anchor splitting and handles, boolean one-node history, bounds/thumbnail revisions, masks coexisting with raster masks, density endpoints/feather/disabled, fixed mask during move versus explicit Batch linking, invalid inputs/locks, native/PSD editability, exact conversion/undo. PSD assertions cover standard tags and cached pixels, including tvMk supplemental-mask restoration and the documented external raster-mask fallback.

Allowed: document/vector.rs; tests/document_vector_ui.rs; document.rs only module/reexports, Shape enum/kind mapping and shape/mask pending cases (do not remove live revision arm); tessera-ffi/Cargo.toml vector dependency and corresponding Cargo.lock edge. New apps/mac/Sources/{Tessera,TesseraCore}/Document/Vector/**; DocumentVectorTests.swift. Small hooks in Tessera/Document/{DocumentView.swift,DocumentViewport.swift,PropertiesPanel.swift,LayersOutline.swift,Tools/DocumentTools.swift,Tools/ToolsPalette.swift,Tools/ToolOverlayView.swift}, Tessera/App/{AppCommands.swift,KeyRouter.swift}, TesseraCore/Document/{DocumentBackend.swift,DocumentKeyMap.swift,EngineDocumentBackend.swift,StubDocumentBackend.swift,StubDocumentModel.swift,StubCompositor.swift,Tools/EditorTools.swift}, limited to enum exhaustiveness/adoption/routing. Common paths below. No render.rs, engine-api, typography or compositor edits.

Conflicts: same B5-07 Properties/outline/menu hooks and B5-09 palette/input hooks; no styles.rs/retouch.rs edits. B5-10 owns common source edit/cancel/conversion and layout fix. Develop isolated code in parallel, serialize final enum/palette/properties hooks and binding generation on the combined base.

Sol acceptance 360–379:
360. Shape tools/shortcuts appear and produce Shape rows, not Fill rows.
361. Drag rectangle/rounded rectangle, edit independent corner radii.
362. Ellipse with Shift/Option constraints, correct center/proportions.
363. Polygon/star sides and inset update live geometry.
364. Line with stroke-only paint remains selectable on its outline.
365. Solid and gradient paint controls update; paint remains document-anchored when moved.
366. Stroke alignment, width, cap/join/miter and dash/offset are visible and preserved.
367. Pen click/drag builds cubic handles and closes a path.
368. Direct Selection moves anchors and mirrored/independent handles.
369. Insert/delete anchors; original primitive no longer regenerates over custom edits.
370. Combine/subtract/intersect/exclude produce correct fill-rule holes and one undo entry each.
371. Fit/zoom/pan/skew hit tests and overlays track geometry, including stroke-only hits.
372. Affine handle drag is one undo step; Esc leaves source and history unchanged.
373. Add vector mask alongside a raster mask without replacing it.
374. Toggle mask, density and feather; pixels/undo match controls.
375. Move source with mask fixed, then explicit linked-mask gesture; one atomic undo restores both.
376. Locked content/position and invalid open-path inside/outside strokes give clear errors.
377. Convert to pixels, effects/masks apply once, undo restores live shape.
378. Native and PSD reopen retain editable controls; supplementary shape mask fallback explained; pattern export limitation explicit.
379. 1440-pt inspector, B5-07 styles and B5-09 Remove still work; screenshot evidence.


## Coordinator notes (Opus, Machine B)
- Base: current main (includes M5-30, B5-06, B5-08, B5-09). B5-07 (layer styles) is finished and merging; B5-10 (Type tool; owns shared `convert_to_pixels`, source-preview cancellation and the inspector layout fix in document.rs and shared Swift files) is running on a sibling branch. Build your vector module, models and tests first. For conversion and cancellation, write your code against the contract in astra-plan.md and put a small local helper in vector.rs, delimited `// B5-11 temporary: replace with B5-10 convert_to_pixels`, so the coordinator can swap in B5-10's generic call at merge. Edit shared hot files only inside `// B5-11 begin` / `// B5-11 end` blocks, never restructuring around other packages' blocks.
- Driving the app: launch your own build with `open -n <this worktree>/apps/mac/build/Tessera.app --args …`, record its PID, send events only to that PID (System Events `process whose unix id is <pid>`), confirm it is frontmost before every keystroke, never confirm delete/trash alerts, use scratch folders only, and quit only your PID. Other Tessera instances from other worktrees may be running.
- Gate (from the worktree root): `export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-11" MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2`; `git diff --check`; `cargo test --locked --release -p vector -p compositor -p psd -p tessera-ffi`; `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `(cd apps/mac && ./build-ffi.sh && swift build --jobs 2 && swift test --jobs 2)`; `(cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-11" -jobs 2 build)`; `(cd apps/mac && Support/make-app.sh debug)`.
- ACCEPTANCE.md steps 360–379 in a new section `## AD. Shapes, Pen and vector masks (B5-11)`. Common paths: apps/mac/ACCEPTANCE.md, apps/mac/DESIGN.md (additions), regenerated bindings under apps/mac/Sources/CTesseraFFI/** and TesseraFFI/** (never hand-edited), tools/orchestrate/wp/B5-11/**.
- Engine source/API changes are out of scope; write tools/orchestrate/wp/B5-11/NEEDS.md for Machine A.
