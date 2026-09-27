# WP B5-12 — Non-destructive Warp, Perspective Warp, Puppet Warp and Content-Aware Scale (Opus, Machine B; planned by GPT-6 Astra)

GPT-6 Astra's plan section for B5-12 follows verbatim, then the coordinator's notes. The 'Contracts fixed for both source editors' section in astra-plan.md next to this file applies (affine conventions, one DocOp per completed edit, cancel semantics).

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


## Coordinator notes (Opus, Machine B)
- Base: wp/B5-10b (B5-10 Type tool + document/fonts.rs; route any new compositor/renderer through `fonts::compositor` / `fonts::install`). B5-11 (shapes/Pen, document/vector.rs) and B5-14 (perf: document/render.rs, snapshot rendering, filter-worker pressure) run concurrently on sibling branches. Keep your edits to document.rs, filters.rs and tools.rs to small blocks delimited `// B5-12 begin` / `// B5-12 end`, and do not touch render.rs.
- Background-only on this Mac (the user is working on it): launch with `open -g -n`, drive through a new `--transform-selftest=<dir>` argument (same pattern as the other self-tests), no activation or System Events input, screenshots only as `screencapture -x -o -l <windowID>` of your own window, quit only your PID. List interactions that need real mouse feel (mesh dragging, pin rotation) under "needs on-screen verification" in IMPLEMENTATION-STATUS.md.
- Gate (from the worktree root): `export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-12" MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2`; `cargo test --locked --release -p transform -p compositor -p psd -p tessera-ffi`; `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `(cd apps/mac && ./build-ffi.sh && swift build --jobs 2 && swift test --jobs 2)`; `(cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-12" -jobs 2 build)`.
- ACCEPTANCE.md: a new section `## B5-12. Warp, perspective, puppet and content-aware scale`, steps 380–399. Regenerated bindings are never hand-edited. Engine changes go in NEEDS.md, not engine crates.
- Keep CGFloat vs Double explicit (Swift 6.2.4 on this Mac).
