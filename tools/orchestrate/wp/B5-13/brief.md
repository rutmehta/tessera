# WP B5-13 — Liquify workspace and Content-Aware Move/Extend (Opus, Machine B; planned by GPT-6 Astra)

GPT-6 Astra's plan section for B5-13 follows verbatim, then the coordinator's notes. The 'Contracts fixed for both source editors' section in astra-plan.md next to this file applies (one DocOp per completed edit, cancel semantics).

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


## Coordinator notes (Opus, Machine B)
- Base: wp/B5-10c (Type tool + fixes, shapes, font snapshot) merged with wp/B5-09b (retouch downloads, non-blocking RetouchJobs cancel). Reuse B5-09b's RetouchJobs pattern for long jobs (Cancel returns immediately, late results discarded) and ModelAcquisition if any model is needed. Route any compositor through `fonts::compositor` / `fonts::install`.
- Critical regression to avoid (from Astra): the current apply_filter crops to the original selection and write_pixels clips by it again, so a naive Content-Aware Move wrapper loses the destination. Compute from a frozen snapshot and install the full affected result atomically; never temporarily clear the live selection.
- Concurrency: B5-14 (perf; document/render.rs, snapshot rendering, filter-worker pressure in filters.rs) and B5-12 (transforms; document/transform.rs, small filters.rs/tools.rs hooks for reserved transform stages) run on sibling branches. Keep your filters.rs edits to a narrow helper inside `// B5-13 begin` / `// B5-13 end`, and do not touch render.rs or transform.rs.
- Background-only on this Mac: launch with `open -g -n`, drive through a new `--liquify-selftest=<dir>` argument, no activation or System Events input, screenshots only as `screencapture -x -o -l <windowID>` of your own window, quit only your PID. Brush-feel checks go under "needs on-screen verification".
- Gate (from the worktree root): `export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-13" MACOSX_DEPLOYMENT_TARGET=15.0 CARGO_BUILD_JOBS=2`; `cargo test --locked --release -p filters -p compositor -p tessera-ffi`; `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`; `cargo fmt --all -- --check`; `(cd apps/mac && ./build-ffi.sh && swift build --jobs 2 && swift test --jobs 2)`; `(cd apps/mac && xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-13" -jobs 2 build)`.
- ACCEPTANCE.md: a new section `## B5-13. Liquify and Content-Aware Move`, steps 400–419. Regenerated bindings only; engine changes go in NEEDS.md. Keep CGFloat vs Double explicit (Swift 6.2.4).
