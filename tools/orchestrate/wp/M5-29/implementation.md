# M5-29 implementation

## Surfaces

The engine contract is 1.4.0 (see `crates/engine-api/CONTRACTS.md`). Nine new document calls carry `document`, `layer`, `params`, `smart` (default false), and the existing request envelope. Destructive calls require pixel layers. Smart calls require an existing smart object and evaluate before committing. They do not implicitly convert a pixel layer into a smart object.

`remove_object` was already a recipe-domain tool/Action. Its existing behavior is preserved. MCP accepts a document-shaped branch of that name; the canonical document call and history Action name is `document_remove_object`. All other new names are unambiguous.

FFI exposes `RasterFilterRequest` / `RasterFilterOperation` through `DocumentSession.apply_raster_filter`, alongside the existing JSON filter API. `DocumentSession.remove_distractions` returns `DistractionRemovalResult` with an update and JSON report. Pixel layers are destructive. Call the existing `convert_for_smart_filters` first for non-destructive edits. Adapter smart nodes use the native compositor format, native child coordinates and shared filter masks, including transformed smart objects.

## Parameter forms

- Remove: `{ "mask": [coverage...], "remove": { "backend": "cpu" | "auto" | "onnx", "dilation": 0, "fill": {...} } }`.
- Distractions: `{ "faces": [[x,y,width,height]], "wires": true, "people": true, "remove": {...} }`. Detection uses the existing geometric wire and dilated face-box proxies, not semantic segmentation. The report identifies this limitation, detected masks, requested backend and dilation. Masks are reported before removal dilation and selection/shared-mask clipping. Smart nodes freeze the union rather than rerunning detection during rendering.
- Fill: `{ "mask": [...], "fill": {...} }`.
- Move: `{ "mask": [...], "offset": [dx,dy], "fill": {...}, "seam": "default" }`.
- Liquify: `{ "mesh": <serialized Mesh>, "interpolation": "bilinear" | "bicubic" }`.
- Camera Raw: existing `{ "settings": <DevelopSettings>, "amount": 1 }` schema.
- Skin: `{ "faces": [[x,y,width,height]], "blur": 4, "smoothness": 0.5 }`. Explicit nonempty face boxes are required. Skin smoothing itself needs no model weights.
- Colorize: `{ "artifact_reduction": 0, "saturation": 1 }`.
- JPEG artifact removal: `{ "strength": 0.5 }`.

Masks and meshes use full-resolution source coordinates (child coordinates for a smart object). Unknown nested parameters are rejected by the adapters. `output_new_layer: true` is explicitly unsupported by single-raster filters, rather than silently discarded. Duplicate a layer first when separate output is wanted. Smart calls with active parent-space selections are explicitly rejected for these operations. FFI distraction removal requires no active selection; explicit Remove masks remain available.

## Models and routing

Neural catalog metadata is exposed by `filters::neural_catalog`. All neural resident-GPU capability probes return false, using the compositor's per-layer CPU fallback. Colorize/JPEG with no installed model return a clear Unsupported weights error, even for a zero-strength request. Validation happens before publication of document state/history.

Rust hosts can explicitly call `CompositorFilters::load_model(name, Some(&registry))` to install Colorize, JPEG or LaMa sessions, using the existing model adapters and CPU execution. This explicit installation may download registry-approved artifacts. Document parsing, evaluation, previews and capability probes never call it or download weights. Sessions are pinned for the process lifetime. Without a LaMa session, Auto uses the existing PatchMatch fallback; explicit ONNX retains its missing-weights error.

No weights were downloaded for this package's tests. Real weighted inference is not claimed as verified by these wiring tests.

## Verification

The required combined gate is run in this worktree with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-29`. See `gate.log` for the final exit status and real test/build output. Additional focused tests cover PatchMatch pixel mutation and undo, parameter rejection, neural weights errors without history mutation, tool alias/schema routing, smart rendering, child/parent extent mismatches, translated smart objects and shared masks.
