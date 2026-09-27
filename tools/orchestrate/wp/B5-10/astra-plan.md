# Tessera Machine B: B5-10 through B5-13 Implementation Plan

**Goal:** Ship editable text, editable vector shapes/masks, advanced transforms, then Liquify/content-aware move in the layered macOS editor.

**Architecture:** Retain engine-api 1.5 and existing engine sources. Thin DocumentSession extensions own validated commands and session-local previews; AppKit owns canvas input/IME/overlays, SwiftUI owns inspectors, and TesseraCore holds testable models. New API names below are proposed, not existing methods.

**Tech stack:** Rust/UniFFI, AppKit/SwiftUI, Xcode 26.3 and Swift 6.2.4 (verified). Planning only: no code, workers, builds, commits, pushes or merges executed.

## Priority, scheduling and ownership

1. B5-10 Editable Type tool and Character/Paragraph inspector.
2. B5-11 Live shapes, Pen/Direct Selection and vector masks.
3. B5-12 Non-destructive warp, perspective, puppet and content-aware scale.
4. B5-13 Liquify workspace and Content-Aware Move/Extend.

Text and vector are TWO packages. IME, bidi and run surgery are a different risk domain from Bézier geometry, live construction and masks; neither should gate the other's feature implementation. They share coordinate/history conventions, not a combined mega-editor.

Start B5-10 when either B5-07 or B5-09 releases a slot; never exceed two concurrent Opus agents. Start B5-11 at the next free slot. It can build its independent vector module/model/tests while B5-10 works, but integrate shared ABI/UI hooks in order B5-10 then B5-11; B5-11's final gate includes the B5-10 shared conversion/cancel plumbing. Then start B5-12 and B5-13 as slots free, with B5-13 based on merged B5-09. No extra implementation agents for shared glue.

All branches are wp/B5-10 through wp/B5-13, worktrees /Users/rutmehta/Developer/lightroom/.worktrees/<id>. Base on refreshed Machine A main containing ff86316/M5-30 plus merged B5-06/B5-08. Running B5-07/B5-09 are behind that base, so reconcile their additions rather than accepting their older generated bindings. Machine B pushes branches and appends READY.md; Machine A alone merges to main. Re-run gates after integrating current main and shared changes.

Disk policy: budget to the supplied ~13 GB free, even though this inspection reported 20 GiB. Keep only two active Cargo caches. Reassign/rename a finished agent's warm cache to the next package's uniquely named target directory; do not copy it or touch it before its cargo processes exit. Serialize expensive Cargo/Xcode/link gates, no universal builds, no workspace-wide test builds, no model downloads. Remove only retired, reproducible build output under coordinator control after preserving evidence; never clean another running agent's cache/worktree. Per-WP DerivedData, per-worktree SwiftPM output.

## Contracts fixed for both source editors

Read crates/compositor/TEXT_VECTOR.md in full. Existing source pointers: document.rs:107 (FFI kind enum), :767 (live revision match), :783 (shape currently reported as Fill); edit.rs:50-115 and :368-513 (live DocOps); DocumentTools.swift:300 (Type placeholder), :746 (pixel-only Free Transform).

- Use existing FFI TransformMatrix with compositor row-major [a,b,c,d,e,f], mapping (ax+by+c, dx+ey+f). Swift AffineTransform2D/CoreGraphics and vector::Affine use different layouts: perform explicit conversion and test translation/skew, not identity only. Geometry is local L0 pixels; vector masks and vector paint sampling are document-space.
- Models travel as the existing strict serde JSON schema, never invented PSD descriptors. Read records return model JSON + transform + revision. Mutations return DocumentUpdate; additions select Applied.created, not an assumed next ID. Parents use None for root; insertion indexes are bottom-first, unlike top-first layer rows.
- One successful DocOp is one history node; Batch is atomic. Pointer previews and typing drafts are scratch/host state, not committed per event. Commit the net operation once; Esc discards the draft with zero committed-history change. Validate and check expected base revision before mutation. Test failed operations without a pending unrelated edit, and separately test the existing rule that unrelated edits/save/undo flush pending edits.
- B5-10 alone owns shared convert_to_pixels(layer), source-preview cancellation and its lifecycle hooks in document.rs. Text/shape-specific implementation remains in text.rs/vector.rs. B5-11 extends only its own keyed pending cases. Cancellation must not discard a different subsystem's stroke/style/filter draft; only one active gesture owns a session scratch at a time. Rebuild previews from the captured base so relative run edits cannot be replayed against the wrong run indexes. Do not repeatedly replace Pending::Text with relative EditTextRuns operations: use the final full draft for preview and one correctly derived run splice for final commit.
- Content edits/conversion respect pixel/all locks. Position lock rejects affine changes but permits local source edits. Singular/nonfinite affines and invalid models fail with no partial change. ConvertToPixels preserves ID, properties/styles, both masks, and document depth; effects apply once, undo restores exact live source. Never treat Layer::raster()==None as an empty layer or silently rasterize for brush/filter tools.
- Caret/selection/IME are host state. Use typography::TextRenderer layout with the SAME system-font database snapshot as CPU/resident rendering, installed through their set_text_renderer hooks. No AppKit/CoreText re-layout as the caret oracle. UTF-16 NSRange, UTF-8 layout clusters and run indexes are three different domains; convert explicitly. Split affected runs only at valid grapheme/shaping-cluster boundaries; EditTextRuns takes a half-open RUN-index range. Preserve bidi visual ordering, ligatures, paragraph/text-box/warp/path data. Marked IME text never enters history until one composition commit.
- Point and paragraph text get canvas caret interaction in B5-10. Warped/path sources retain their data/rendering and can use a clearly labeled source-text editor; disable direct canvas caret placement on them until correct rotation/warp mapping is implemented. Do not silently offer flat hit testing. Vertical composition and dictionary hyphenation remain explicitly unavailable. Use installed fonts only; missing fonts are errors. Custom font discovery in a UI-only renderer is forbidden because conversion/export/nested renderers do not inherit it.
- No implicit live-text ICC/transfer conversion exists, and F32 is not an encoding. Verify sRGB sample-space creation/editing; expose a clear limitation for non-sRGB/linear combinations rather than silently claiming color-managed live text. Vector fill colors must be converted by the host to the document sample space. Unsupported color interpretation must be reported, not guessed from depth.
- Source render costs are CPU layout/path rasterization plus upload, with conservative full-canvas damage. Coalesce events and measure complete preview latency; do not promise native GPU curves or quote cached transform GPU time as gesture latency.

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

## B5-12 — Smart warp, perspective, puppet + content-aware scale

Brief: Read transform/{TRANSFORM.md,src/op.rs,src/free.rs,src/warp.rs,src/perspective.rs,src/puppet.rs,src/seam.rs}, compositor/src/{edit.rs,resident/TRANSFORM.md,render/smart_filters.rs}, tessera-ffi/document/{tools.rs,filters.rs}, and existing FreeTransformMath.swift/ToolsMenus.swift. Add document/transform.rs: transform_stage(layer,index), begin_advanced_transform(layer,index?), preview_advanced_transform(token,transform_json), commit_advanced_transform(token), cancel_advanced_transform(token), warp_preset/split helpers, puppet_mesh_from_layer(layer,density,expansion), and content_aware_scale_from_channel(layer,target_w,target_h,amount,channel_id?). Data-heavy alpha/protection stays Rust-side. Map to versioned TransformOp and AddTransform/SetTransform, not vector::content_aware_scale (unsupported) or destructive B5-04 raster Free Transform. For a pixel target, offer explicit conversion on Apply and build wrapper + stage as one Batch; cancel creates no smart object. Retain source/masks/styles exactly once. Existing smart stage re-edit preserves enabled/blend/order. Canvas has Bézier net/splits/presets, joined perspective quads, puppet pins/rotation, scale handles and dimensions/protect-channel controls. Preview is coalesced off-main, commit one history node; fixed child canvas/clipping and native-only nonlinear PSD limitations are shown. Basic text/shape affine edits stay live via B5-10/11; nonlinear operations never silently flatten them: require explicit smart-object conversion or report unsupported. Finish/discard any text composition before switching to transform tools.

Important integration gate: filters.rs currently parses generic filter specs; reserved compositor `transform` nodes must NOT be fed through its ordinary Spec parser/baker, overwritten on re-edit, or dropped when unrelated filters change. Allow narrowly scoped passthrough/stack-preservation integration, routing transforms to the existing compositor. Native GPU Free/Warp/Perspective/Puppet routing exists (M5-23); content-aware scale is CPU fallback. Do not repeat the stale TRANSFORM.md claim that all automatic routing is CPU.

Tests: new document_transform_ui.rs and DocumentTransformTests.swift (>=12 named each), existing transform/compositor tests. Verify pixel-to-smart atomicity/Cancel, source retained, mask/style placement, transform stack with ordinary + retouch filters, source->child coordinates, pin constraints/mesh validation, protect alpha channel sampled at matching mip, all interpolation choices, locks, stale token rejection, per-gesture history, native reopen, explicit PSD refusal/rasterized-export option, and actual CPU/Metal comparisons. No implicit skin detector. No performance pass inferred from cached GPU-only timings.

Allowed: document/transform.rs; tests/document_transform_ui.rs; document.rs module/reexports and session-local transform state only; document/tools.rs only capability dispatch/shared helper visibility, preserving existing pixel path; document/filters.rs only reserved-transform passthrough/preservation and reusable atomic smart-wrapper helper (also preserve raster/vector masks once); tessera-ffi/Cargo.toml transform path dependency + lock edge. New apps/mac/Sources/{Tessera,TesseraCore}/Document/Transforms/**; DocumentTransformTests.swift. Small hooks in Tessera/Document/{DocumentView.swift,DocumentViewport.swift,Tools/DocumentTools.swift,Tools/ToolsPalette.swift,Tools/ToolOverlayView.swift,Tools/ToolsMenus.swift,Filters/SmartFilterRows.swift}, Tessera/App/AppCommands.swift and EngineDocumentBackend.swift/StubDocumentBackend.swift adoption. No render.rs or engine crate modifications; raise NEEDS.md for engine gaps. Common paths below.

Conflicts: B5-09 Tools/SmartFilterRows hooks and retouch stack behavior, B5-07 global menu/registration only. Do not touch retouch.rs or styles.rs or replace style fallback. Integrate filters.rs passthrough with B5-09 before final gate; B5-13 consumes the same stack preservation behavior.

Sol acceptance 380–399:
380. Edit > Transform exposes Warp, Perspective Warp, Puppet Warp and Content-Aware Scale.
381. Pixel-to-smart opt-in preserves source, properties and masks; cancel leaves Pixel kind/history untouched.
382. Drag Bézier control points with responsive overlay and real image preview.
383. Add horizontal/vertical splits without a jump in the current warp.
384. Warp presets/bend, zero bend restores identity.
385. Create linked perspective quads; drag shared edge without cracks.
386. Invalid crossing/degenerate quads rejected, previous preview intact.
387. Puppet mesh, add/move/delete pins and rotate a pin.
388. Puppet density/expansion/rigid options and invalid-limit errors are honest.
389. Content-Aware Scale handles/numeric dimensions and Amount=0 versus 1 change output.
390. Pick saved alpha protection channel; no phantom automatic skin detection.
391. Zoom/pan maps handles to correct child/document coordinates.
392. Apply one gesture, undo/redo exactly once.
393. Cancel long/stale preview, switch document, no late mutation.
394. Reopen transform stage and edit without replacing neighboring filters/order/blend/enabled state.
395. Position/all locks reject geometry; unrelated color filtering still obeys existing lock semantics.
396. Basic affine text/shape transform remains editable; nonlinear conversion requires explicit consent.
397. Save/reopen native stage with masks and source intact.
398. PSD cannot silently lose nonlinear transform; display limitation and explicit rasterized export path if provided.
399. Real Metal/CPU render checks, end-to-end latency notes, 1440-pt UI and B5-07/B5-09 regression evidence.

## B5-13 — Liquify workspace + Content-Aware Move/Extend

Brief: Read filters/src/{liquify.rs,liquify_gpu.rs,caf.rs,compositor_adapter.rs}, tessera-ffi/document/filters.rs (especially full_resolution, filter_target, write_pixels, apply_filter), and the MERGED B5-09 retouch.rs. New document/liquify.rs exposes begin_liquify(layer,stage_index?), liquify_brush_points(token,tool,brush,points), liquify_mesh(token), preview_liquify(token), commit_liquify(token,destination), cancel_liquify(token). Keep the Mesh and freeze plane in Rust; reuse Mesh::apply_brush and original-base renders, inverse displacement means output samples source(x+dx,y+dy), not forward geometry. New document/content_aware.rs exposes begin_content_aware_move(layer,mode), preview_content_aware_move(token,dx,dy,fill_json,seam), commit_content_aware_move(token), cancel_content_aware_move(token); freeze the selection mask in Rust and reuse caf::move_or_extend. App gets a Liquify canvas sheet/workspace with all ten supported brush actions, size/density/pressure/rate, mesh/freeze overlay, reconstruct/reset, before/after and Apply/Cancel; content-aware tool drags a selection with Move/Extend mode and integer L0 offset. Re-edit a Liquify smart filter without appending duplicates; B5-09 Remove/Neural/CAF remains untouched. One Apply is one DocOp/Batch; cancellation or stale source never commits. No automatic model download or implied face-aware functionality; optional five-landmark face controls only if explicitly wired and tested, not part of required scope.

Critical correctness: generic apply_raster_filter with the original selection is NOT a sufficient Content-Aware Move wrapper. apply_filter crops to the original selection and write_pixels clips by it again, losing moved destination pixels. Compute from an immutable snapshot, then install source+destination/affected result in one revision-checked op (optionally selection update in the same Batch), preserving unrelated selection/history and alpha-lock policy. Do not temporarily clear the live selection. For smart output, store explicit frozen operation mask, do not rely on the current active selection/shared filter mask. Current adapter previews are full-resolution; throttle and show busy/cancel rather than claiming mip-speed previews. Cancel may discard a result without interrupting every engine subroutine: document measured cancellation behavior honestly.

Tests: new document_liquify_ui.rs and document_content_aware_ui.rs (>=8 named each), DocumentLiquifyTests.swift and DocumentContentAwareTests.swift (>=8 each). Compare small checker/alpha output to existing engines, inverse direction, frozen mesh invariance, reconstruct, selection-to-mask mapping at zoom, Move versus Extend source behavior, destination OUTSIDE original selection, fractional feather not double-clipped, stable seeds, cancellation/stale document/lock enforcement, one-node atomicity including output destination, native smart re-edit, full-res dimensions at fit previews, and B5-09 Remove/CAF/neural regressions.

Allowed: document/{liquify.rs,content_aware.rs}; tests/{document_liquify_ui.rs,document_content_aware_ui.rs}; document.rs registration/reexports and transient per-session state only. document/filters.rs narrowly for existing preview/checked-write adapter helpers or visibility, never duplicate algorithms; do not edit retouch.rs. New apps/mac/Sources/{Tessera,TesseraCore}/Document/{Liquify,ContentAware}/** and the two named Swift tests. Hooks only in Tessera/Document/{DocumentView.swift,DocumentViewport.swift,Tools/DocumentTools.swift,Tools/ToolsPalette.swift,Tools/ToolOverlayView.swift,Filters/FilterMenus.swift,Filters/SmartFilterRows.swift}, Tessera/App/{AppCommands.swift,KeyRouter.swift}, TesseraCore/Document/{DocumentKeyMap.swift,Tools/EditorTools.swift}, EngineDocumentBackend.swift/StubDocumentBackend.swift adoption. No new Cargo dependencies anticipated. Common paths below.

Conflicts: strong functional overlap with B5-09: MUST consume it after merge, preserve its generation-counter outline fix, never fork its Remove mask accumulator or write retouch.rs. Shares palette/menu/SmartFilterRows hooks with B5-12; coordinator serializes integration. B5-07 only registration/menu overlap; render.rs/styles.rs forbidden.

Sol acceptance 400–419:
400. Real-engine Filter > Liquify workspace opens, correct canvas and selected target.
401. Forward Warp follows brush direction, not inverse-sign motion.
402. Twirl clockwise/counterclockwise, Pucker/Bloat/Push Left are visibly distinct.
403. Smooth/Reconstruct and Reset behave against original mesh/source.
404. Freeze paint prevents deformation there; thaw restores edits; overlay follows zoom/pan.
405. Size/density/pressure/rate and mesh display controls function.
406. Before/after does not mutate source or history.
407. Apply is one undoable Liquify edit, undo/redo restores exact state.
408. Smart-filter output preserves source; reopen/re-edit same stage without duplicating.
409. Cancel a busy preview/apply, no late result/history mutation.
410. Create selection then enter Content-Aware Move.
411. Drag outside original selection; moved subject and healed source both appear.
412. Extend retains original subject and adds moved subject.
413. Edge/feather/seam adaptation respects frozen selection once, not double-clipped.
414. Fill seed/settings changes preview, same seed is repeatable.
415. Zoom/pan and integer document-pixel offsets agree with on-canvas placement.
416. Cancel leaves layer/selection/history unchanged; Apply is one step.
417. Locks, deleted/stale targets and no-selection errors leave no partial result.
418. Native reopen and smart-filter re-edit match saved appearance; unsupported PSD loss is explicit.
419. Re-run B5-09 Remove stroke, CAF, neural missing-weight behavior; 1440-pt inspector and screenshot evidence.

## Common allow-list and integration rules

Every package may add its own tools/orchestrate/wp/<id>/{brief.md,acceptance.md,NEEDS.md,REPORT.md,evidence/**}, append only its assigned section to apps/mac/ACCEPTANCE.md and design notes to apps/mac/DESIGN.md, and regenerate apps/mac/Sources/{TesseraFFI,CTesseraFFI}/** (never hand-edit generated Swift/C). READY.md and board.json updates are coordinator-only on the package branch, only own entries. No engine-api/engine source edits, no root Cargo.toml changes, no runner-script edits. Exact new Swift test files named in each package are allowed. Backend-protocol adoption is narrow; implementation belongs in each feature's new TesseraCore directory.

Hotspots: document.rs, document/render.rs (B5-07 + B5-10 only), AppCommands.swift, KeyRouter.swift, DocumentView.swift, PropertiesPanel.swift, Tools/{DocumentTools,ToolsPalette}.swift, generated bindings, ACCEPTANCE.md and Cargo.lock. Avoid whole-file rewrites. Merge additive hooks on the coordinator's branch, regenerate bindings from the combined Rust source, rerun all gates. Do not cherry-pick stale generated ABI wholesale or let a later branch remove new enum arms.

Deferred after these four: Camera Raw filter sheet (next 420–439), Auto-Align/Blend/Photomerge in document/stack.rs (440–459), Adaptive Wide Angle + Vanishing Point (460–479). Reserve numbers now, but do not launch/implement them as scope creep. The inspector defect is B5-10, not another background task.

## Exact gates

Run in a bash shell for EACH package after tests are authored. The B5-10 commands use typography; B5-11 vector; B5-12 transform; B5-13 filters. These are future gates, NOT executed results.

    set -euo pipefail
    ROOT=/Users/rutmehta/Developer/lightroom
    WP=B5-10  # repeat separately with B5-11, B5-12, B5-13
    WT="$ROOT/.worktrees/$WP"
    cd "$WT"
    export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/$WP"
    export MACOSX_DEPLOYMENT_TARGET=15.0
    export CARGO_BUILD_JOBS=2
    export CARGO_INCREMENTAL=0
    df -h "$WT" "$CARGO_TARGET_DIR"
    xcodebuild -version
    xcrun swift --version
    git status --short
    git diff --check
    case "$WP" in
      B5-10) cargo test --locked --release -p typography -p compositor -p psd -p tessera-ffi ;;
      B5-11) cargo test --locked --release -p vector -p compositor -p psd -p tessera-ffi ;;
      B5-12) cargo test --locked --release -p transform -p compositor -p psd -p tessera-ffi ;;
      B5-13) cargo test --locked --release -p filters -p compositor -p tessera-ffi ;;
      *) exit 2 ;;
    esac
    cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings
    cargo fmt --all -- --check
    cd "$WT/apps/mac"
    "$WT/apps/mac/build-ffi.sh"
    swift build --jobs 2
    swift test --jobs 2
    xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' \
      -derivedDataPath "$HOME/.cache/tessera-derived-data-$WP" -jobs 2 build
    "$WT/apps/mac/Support/make-app.sh" debug
    codesign --verify --deep --strict "$WT/apps/mac/build/Tessera.app"

Cargo.lock must already include any permitted new local FFI dependency edges before the --locked gate. Do not bypass --locked or refresh unrelated crate versions. Build cache adoption is a coordinator prerequisite; if target directory or disk space is unavailable, resolve that before the gate rather than consuming another full cold cache.

Run new-suite completeness checks explicitly as well (prevents a green result that never ran the feature tests):

    # B5-10, from its worktree:
    cargo test --locked --release -p tessera-ffi --test document_text_ui -- --nocapture
    (cd apps/mac && swift test --jobs 2 --filter DocumentTextTests && swift test --jobs 2 --filter DocumentInspectorLayoutTests)
    # B5-11:
    cargo test --locked --release -p tessera-ffi --test document_vector_ui -- --nocapture
    (cd apps/mac && swift test --jobs 2 --filter DocumentVectorTests)
    # B5-12:
    cargo test --locked --release -p tessera-ffi --test document_transform_ui -- --nocapture
    cargo test --locked --release -p compositor --test transform_gpu -- --nocapture
    (cd apps/mac && swift test --jobs 2 --filter DocumentTransformTests)
    # B5-13:
    cargo test --locked --release -p tessera-ffi --test document_liquify_ui --test document_content_aware_ui -- --nocapture
    (cd apps/mac && swift test --jobs 2 --filter DocumentLiquifyTests && swift test --jobs 2 --filter DocumentContentAwareTests)

Record actual feature-test counts, total tests, skipped/ignored tests, compile/clippy/fmt exit codes, app build/signature and commit in each REPORT.md. Any Metal test skip is unverified GPU coverage, not a GPU pass. Expected defects discovered outside the allow-list become NEEDS.md to Machine A, never a silent stub.

Sol is serialized, preferably on Machine A as the existing coordination plan specifies. After the coordinator installs each branch and its acceptance.md, run from that checkout root, with ROOT set to its absolute repository path:

    cd "$ROOT"
    "$ROOT/tools/orchestrate/verify-sol.sh" B5-10
    "$ROOT/tools/orchestrate/verify-sol.sh" B5-11
    "$ROOT/tools/orchestrate/verify-sol.sh" B5-12
    "$ROOT/tools/orchestrate/verify-sol.sh" B5-13

Run only after each package's build gate, not while another Sol session uses the display. Each acceptance.md must state its exact app bundle path, real-engine fixture and every numbered expected state above. Require screenshots under the package evidence directory plus verdict.json with every assigned number; an overall boolean with missing steps is NOT acceptance. IME verification needs a configured input source; if absent, record that step unverified instead of substituting pasted text. Machine A merge decision requires green relevant gates plus honest complete Sol evidence.
