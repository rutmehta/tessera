# UX-05 source map: Photo Edit → Document/Layers continuity

Read-only review of current `main` at `ffd1c84cbdfeb7a25b0e6ee6c43edb03c1354d8d` (2026-09-28). No repository edits, builds, tests, app launch, or GUI actions were performed. Existing dirty/untracked workspace artifacts were left untouched. The scope is source mapping and a contract recommendation; it is not acceptance for a live RAW layer.

## Current behavior

The UX-05 board row says the transition is a rendered copy and asks for a graph/persistence/version contract before a live-RAW claim (`docs/coordination/TASK-BOARD.md:21,438`). The user-facing “Open in Layers” sheet explicitly promises a copy of current adjustments, no refresh from later Develop edits, and separate source/recipe and document saves (`apps/mac/Sources/Tessera/Shell/WorkspaceHeader.swift:62-84`). The existing test `testFirstLayeredCopyIncludesPendingPhotoAdjustment` checks both sides: pending exposure appears in the first copy; a later exposure change leaves the already-open document ID and pixels unchanged (`apps/mac/Tests/TesseraCoreTests/AgentReviewNavigationTests.swift:263-300`).

For an engine-backed image, AppModel captures the selected `PhotoItem` and `EngineLibrary`, reserves the recipe-read gate, and waits for the save result. It then rechecks library/load generation, selection/focus, view/source, agent mutation, and recovery state before dispatching `editInLayers`; the gate is held until the asynchronous backend settles (`apps/mac/Sources/Tessera/App/AppModel.swift:1177-1213,1227-1251,1251-1282`). `prepareForRecipeRead` reserves the owner/image and closes the matching Develop controller (`AppModel.swift:2284-2291`). This is an ownership/save barrier around reading the recipe, not a persisted recipe snapshot or document-to-RAW link.

`DocumentWorkspace.editInLayers` calls `openDocumentFromImage(imageId, developed: true)` for an engine RAW (`apps/mac/Sources/Tessera/Document/DocumentWorkspace.swift:271-309`). The FFI resolves the indexed path/orientation, reads the current sidecar recipe when `developed`, decodes the source, and calls `export::render_pixels` with sRGB, no resize/sharpen, scale 1. It converts the resulting RGB float pixels to opaque RGBA and builds one U16 pixel layer with an sRGB profile (`crates/tessera-ffi/src/document/io.rs:243-315`). `open_document_from_image` is idempotent for the in-process key `image:{image_id}:{developed}` (`crates/tessera-ffi/src/document.rs:632-646`). The opened object has `path: None`, `unsaved: true`, and a runtime `source_image_id` (`io.rs:303-315`); that ID is exposed in active-session info (`document.rs:1335`) but is not serialized as document provenance. Opening a saved `.tessera-doc` sets `source_image_id: None` (`io.rs:105-124`); saving writes the compositor document/tiles or PSD, not the RAW recipe/source (`io.rs:351-367`).

The native document format is v1. Its manifest carries the document graph and chunks (`crates/compositor/src/format.rs:37-80`), and its serialized layer variants are pixel, adjustment, fill, group, smart object, text, and shape (`format.rs:101-145`; runtime `LayerKind` at `crates/compositor/src/document.rs:397-427`). A smart object embeds a child `DocState`, not an external RAW/source/recipe reference (`document.rs:358-383`). There is no RAW source node. The current v1 parser now checks the small manifest header and rejects a future format version before decoding typed layer variants (`format.rs:688-727`); this preflight is accepted on main `372dbbcc` with validated final source `26845eed` (`fb81f279` is earlier source history), and the format version remains 1. Therefore the draft’s older statement that the preflight is still missing is stale on this main.

RAW identity and recipes have separate meanings. `ImageId` is a stable sidecar identity and explicitly covers a master or virtual copy (`crates/engine-api/src/id.rs:88-94`); it is not physical-file identity or a content digest. A recipe contains `image_id`, `source_kind`, `process_version`, settings, selection, history, counters, provenance, and unknown top-level values (`crates/engine-api/src/recipe/mod.rs:226-253`). Its render hash includes source kind, process version, and settings, but excludes history, selection, counters, and unknown fields (`recipe/mod.rs:272-303`), so the hash alone cannot reconstruct an old pinned recipe. Older recipe schemas migrate on read while future-schema writes fail (`recipe/mod.rs:420-444`). The index stores paths, size/mtime, and recipe hashes; scan freshness compares size, mtime, and sidecar stamp (`crates/index/src/lib.rs:109-117,215-235`), not an immutable content digest.

Rendering semantics need an explicit boundary. Current handoff is full-resolution, display-oriented, opaque U16 sRGB. `render_pixels` delegates to a GPU path when available, then its CPU fallback calls `pipeline_cpu::render_managed_scaled` with cloned settings (`crates/export/src/lib.rs:190-228,303-325`). That fallback does not receive `Recipe.process_version` as an explicit renderer parameter. Thus the future contract cannot assume this handoff is already a version-pinned RAW render. The existing flattened copy is display-referred sRGB; it does not establish linear/HDR sample semantics.

## Coordinator decision incorporated

The “Coordinator review of B feedback” section of `docs/coordination/LIVE-RAW-LAYER-CONTRACT-DRAFT.md` supersedes its earlier live-follow outline for the initial slice: use an immutable pinned recipe snapshot; distinguish physical asset identity/content digest from recipe `ImageId` (which can identify a virtual copy); defer automatic committed-follow; keep the rendered-copy API/disclosure unchanged; fail closed on missing/replaced source and future schemas; specify color, extent/orientation, and sample semantics before any source node. A recipe hash is insufficient to recover settings. A owns engine resolver/render/admission and native-format responsibilities; B owns Document/FFI integration/UI. No graph work is authorized by the draft itself.

The draft’s “minimum ready preparatory work is version preflight” is now satisfied by accepted main `372dbbcc`, validated final source `26845eed`, with current/future format discriminator coverage. Do not repeat that as the next unmet prerequisite.

## Smallest feasible next contract slice

Before introducing a source-backed node, settle and document one **pinned source snapshot envelope** for a single source-backed layer. It should be immutable and sufficient to reproduce or fail closed after save/reopen:

- physical asset identity plus content digest (not just path or recipe `ImageId`);
- recipe `ImageId` and the complete pinned recipe snapshot/declared schema, including `source_kind`, `process_version`, and render-affecting settings (not merely `recipe_hash`);
- a version-aware renderer contract that either explicitly honors the pinned process version or returns an unsupported-version error;
- fixed extent, orientation, color space/transfer function, alpha, and sample/depth semantics;
- resolver behavior for moved, missing, replaced, and virtual-copy sources, with failure preserving the document graph;
- v1 compatibility behavior: retain existing flattened documents unchanged; future source-node versions fail closed without destroying/re-writing the original file.

Keep the first slice to resolver/snapshot serialization and round-trip/fail-closed tests; do not include automatic recipe-follow, shared undo, masks/transform behavior, or broad UI. The source-backed node itself should only follow after A/B agree the envelope and rendering boundary. Existing “Open in Layers” stays a separate baked-copy path.

## Unanswered decisions

1. What is the authoritative physical identity/digest algorithm and how does it distinguish a virtual-copy recipe from the original physical asset?
2. Which exact version-aware renderer API is authoritative for the pinned recipe, particularly Adobe process revisions and native revision behavior?
3. Are source-layer pixels/display the existing opaque U16 sRGB output, or a different linear/HDR/depth boundary? Define orientation and canvas extent under camera rotation/crop.
4. What should reopen do if the exact content digest is unavailable but the same `ImageId` resolves, or if a path is reused for different bytes?
5. What is the initial node’s refresh policy? Coordinator direction defers automatic committed-follow; it still needs a precise immutable/stale indication and explicit user action before future refresh behavior is specified.
6. Where does source reference data live in the format, and what is the v1-reader behavior for the future format version? Current v1 readers now reject a future version before typed decode; migration/retention policy for a future source node remains unimplemented.

## Recommendation and limits

The next decision artifact should be the pinned source snapshot envelope and renderer/color semantics, jointly reviewed by A and B. Then A can own a small resolver/snapshot API and tests; B’s source-backed layer integration remains a separate owned slice. No current behavior should be described as live RAW continuity, recipe-follow, or persistence of source provenance. This review did not inspect GUI behavior, run tests/builds, or establish render performance/cancellation guarantees.

Coordinator publication corrections: clarified initial versus final preflight provenance and replaced historical draft line references with its section title. Original source-map artifact remains in /tmp.
