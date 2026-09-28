# UX-05 draft: live RAW source and layer continuity

## Descriptor/capture proposal — 2026-09-28 14:20 UTC

The concrete next proposal is LIVE-RAW-PINNED-SOURCE-PROPOSAL.md, independently reviewed against LIVE-RAW-CAPTURE-POLICY.md. It is descriptor/capture design only. Native process2 plus pinned bytes does not alone qualify rendering: renderer/dependency admission remains unresolved. No compositor node, decoder wrapper, FFI or UI implementation follows from publishing this proposal.

## Current precedence and completed prerequisite — 2026-09-28

The later coordinator review below supersedes the initial automatic-follow proposal: the first source-backed contract pins an immutable recipe payload. Later edits to the original photo do not automatically change that snapshot. Automatic follow and shared undo require a separate explicit policy. `ImageId` can denote a virtual copy; physical asset identity/content digest remains distinct. A recipe hash cannot recover the pinned settings.

The format-version preflight is already DONE on main372dbbcc with validated final26845eed. Current code parses ManifestHeader and rejects future versions before decoding typed layer variants. The older statement below that complete Manifest decoding happens first is historical and superseded. Format remains v1, with no RAW source node or persisted document undo.

See LIVE-RAW-GRAPH-PERSISTENCE-AUDIT.md for current source findings and the smallest engine contract proposal. Source bytes need a coherent digest/read policy; supported recipe/process versions and output color/extent/sample semantics must be explicit. Existing flattened export is not proof of arbitrary pinned-process fidelity. No new graph, B-owned adapter or user-facing live-RAW capability is implemented by these notes.


Source-only contract draft inspected at `review-ownership/tessera` `fa871bb9`; root checked the current handoff and recipe definitions again on main54a8e85f. No product edits, build, or app launch. This defines the next dependency behind UX-05; it does not change the completed rendered-copy path.

## Current user-visible contract and implementation

The existing Library action is intentionally **Open in Layers…**. Its disclosure says the first open makes a rendered copy with current photo adjustments; a later open reuses the open copy's current pixels/layer edits; later Develop edits do not refresh it; the original photo/recipe remain separate; save the layered document separately (`apps/mac/Sources/Tessera/Shell/WorkspaceHeader.swift:62–84`, `apps/mac/DESIGN.md:543–547`). Keep these statements until an actual linked-source path ships.

For indexed images, `DocumentWorkspace.editInLayers` calls `openDocumentFromImage(imageId:developed: true)` (`apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift:175–200`). The FFI path loads the RAW and current recipe, renders through the export pipeline to display-oriented full-resolution sRGB, converts to one U16 pixel layer, and stores `source_image_id` only in the active session (`crates/tessera-ffi/src/document/io.rs:241–310`, `document.rs:632–644, 573–609`). This is a source-aware **creation action**, not a live source graph. Opening a saved `.tessera-doc` sets `source_image_id` to `None` (`document/io.rs:96–121`); that ID is runtime state, not persisted provenance. The compositor's current `LayerKind` / format manifest represent pixels, adjustments, fills, groups, smart objects, text and shapes, with no RAW source layer (`crates/compositor/src/document.rs:397–426`, `format.rs:108–145`). `.tessera-doc` format v1 serializes document graph/tiles but explicitly does not persist history; reopening starts a fresh history (`format.rs:1–16, 37–40`).

RAW edits already have a separate durable contract: per-image `.edits/<stem>.json` plus XMP alongside source files; recipes contain schema version, source kind, process version, settings, selection, append-only history, ID counters and provenance (`docs/05-catalog-storage-and-import.md:14–40`, `crates/engine-api/src/recipe/mod.rs:43–44, 229–267`). Recipe hash includes source kind, process version and render settings, not history, selection, or import provenance (`recipe/mod.rs:271–300`). Current recipe schema is 3; current native process revision is 2, with Adobe PV1–6 represented separately. Reads migrate older recipe fields, preserve unknown top-level data, and reject writes from a future schema (`recipe/mod.rs:43–104, 420–443`). Stable `ImageId` survives moves/renames through its sidecar, but is not itself a path or content-revision fingerprint (`engine-api/src/id.rs:91–99`).

## Draft contract for a future live source layer

1. **One named source of truth.** A live source layer identifies the indexed RAW master and resolves to its authoritative per-image Develop recipe. It is a graph node that can be recomputed from source + recipe; it is not a baked pixel copy. Layer-local placement, mask, visibility, blend and opacity remain document graph state and do not write into the photo recipe.
2. **Explicit edit relationship.** Develop edits on a linked photo update the source layer's upstream render while the document is open. Document edits remain in document history. UI must state where Undo/Redo applies, whether the document is dirty, and when source changes become visible. Until source and document histories have a deliberate shared/transactional policy, never present them as one continuous undo timeline.
3. **Persist enough to resolve or fail safely.** The native document must store a typed source reference (stable image ID plus resolver/path hint), source content revision/fingerprint, recipe hash/revision, recipe schema and process version, and layer-local graph state. The library index is rebuildable, so it cannot be the only saved locator. On missing/replaced source, show an unavailable/relink state and preserve graph edits; never silently bind a different file or substitute an unlabelled stale flattened layer.
4. **Version as graph data.** Extending `.tessera-doc` with a source-backed layer requires an explicit format-version migration and defined behavior for older readers. Recipe schema and render `ProcessVersion` stay authoritative; the document records which recipe/process it links to and detects a changed revision. Cache identity must include source revision + recipe hash/process version + layer render inputs. A source change invalidates derived layer render without changing document-only operations.
5. **Keep flat handoff a separate path.** Existing Open in Layers and Save Rasterized PSD Copy remain clear snapshot/copy actions. A live RAW layer is offered only through a path whose UI promises are backed by the persisted source node and update/conflict policy.

This is a contract proposal, not a request to implement all five points in the UX layer. The minimum next dependency is a jointly reviewed engine/document persistence API: source identity and content revision resolution, live rendering from the Develop recipe into the compositor, recipe-change notification/invalidation, and a versioned serialized source-layer node. The current compositor/`.tessera-doc` model has no such node, while A owns the engine recipe/render pipeline and B owns the active Document/FFI integration; a UI-only lane cannot supply continuity.

## Release acceptance scenarios

- **Open and persistence:** On a small supported RAW, open the live-layer path. Inspect that the document contains a source-backed node with the expected stable image ID, source fingerprint and recipe/process version, not only a pixel base. Save, close the process, reopen the `.tessera-doc`, resolve the same source, and compare rendered pixels/metadata within the declared deterministic tolerance. Verify native document edits and source recipe remain distinct and intact.
- **Live Develop update:** Keep the document open, change RAW exposure/white balance in Develop, and verify the source layer refreshes from the new recipe while retaining layer transform/mask/opacity and selected document context. Verify change notification, cache invalidation, dirty state and both undo histories follow the specified policy.
- **Layer-only edit:** Change a document-only operation (for example layer opacity or transform); verify the source `.edits` recipe/history is byte-stable and RAW pixels/source file are not rewritten.
- **Conflict and stale state:** Change the source recipe while the document has unsaved layer edits; assert the declared refresh/conflict behavior preserves both sides. Delay an older RAW render, then change recipe or switch source; the stale render must not replace the current source layer.
- **Relink/mismatch:** Move/rename the RAW while preserving its stable ID; resolve it from the sidecar/index without binding another image. Then separately test missing source and changed-content-at-same-path: surface an explicit state, preserve the document graph, and require the contract's relink/rebase action.
- **Version compatibility:** Reopen a saved graph with its recorded native process revision (and an Adobe-PV recipe where supported); keep recipe semantics stable. Test migration from the prior `.tessera-doc` format and reject/retain future source-node data safely according to the version policy.
- **Copy-path regression:** Existing Library Open in Layers still says and behaves as a rendered snapshot; later RAW recipe changes do not silently turn that already-open flattened document into a live layer. Existing Save Rasterized PSD Copy leaves the source document and RAW recipe unchanged.

No performance, cancellation, arbitrary-camera or full-suite acceptance is implied by these functional scenarios. Those need their own source-size, render-resource and format policies once the persistence API is reviewed.

## Coordinator review of B feedback — 2026-09-28 UTC

B source review18cfb1c8 at tools/orchestrate/wp/B5-16/LIVE-RAW-LAYER-CONTRACT-REVIEW.md
is accepted as design evidence, not implementation acceptance. Root confirmed
native from_bytes parses the complete Manifest/MKind before checking version.

The next internal slice should use an immutable pinned recipe snapshot, distinct
physical asset identity/content digest and recipe ImageId (which may identify a
virtual copy). A recipe hash alone cannot recover past settings. Existing
rendered-copy APIs and disclosures stay unchanged. Automatic committed-follow
remains a later capability requiring accepted dirty/undo/geometry policy; the
initial source-backed slice must not be presented as automatic live editing.
Save/reopen preserves document state, not document undo history. Missing/replaced
sources and future schemas fail closed, retaining the original graph/file. Define
fixed color, extent/orientation and sample semantics before introducing a node.

A owns engine identity/resolver/render/admission, compositor and native-format
coverage; B owns Document/FFI adapter and eventual UI. No new graph implementation
is authorized by this note alone. Minimum ready preparatory work is a version
preflight contract/regression, since current old readers cannot retroactively
produce a clean unsupported-version response for unknown layer variants.
