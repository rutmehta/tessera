# Engine-api additions requested (not changed in M5-10)

The engine-api 1.2 contract remains unchanged. MCP-local extensions are listed by tools/list with schemas.

- SelectionShape variants for wand (seed, tolerance, contiguous, antialias, sample_size), quick selection (stroke, radius, threshold, texture_weight, close), colour range (sRGB samples, Lab fuzziness), object (box/click/lasso prompt, refine_radius), subject and sky (refine_radius). Current transport: select_advanced {document, operation: {kind, ...}, mode, feather}.
- DocumentToolCall variants for refine_edge, selection_boolean, undo, redo, ABR import/list, and preset painting. Local mutation envelopes accept rationale and expect_head. Boolean operands reference saved selection IDs.
- Brush preset ID and deterministic seed/full preset snapshot fields in PaintStroke. Current transport: paint_preset {document, layer, preset_id, points, target?, seed?}; complete immutable presets resolve from <app-dir>/brush-presets.json. Store API saves all brush settings, including sampled tips, dynamics and clone/heal sources. ABR descriptor dynamics unsupported by the brush parser are explicitly reported as warnings.
- Portable Action registration/replay for these new names. Local mutations store honest command/parameter descriptors in session history; preset strokes include full brush snapshots. They reject execution while portable Action recording is active rather than silently dropping steps. Undo/redo uses retained states, not command replay.
- DocumentOpened warning field. MCP currently adds warnings beside the typed ok output, and describe_document retains warnings.
- Persistent saved alpha-channel selections in the document container. This implementation tracks saved selections in session history, including branching undo/redo, but does not extend the frozen persistence contract.
- Output ICC profile-handle registry and transformation policy. Document exports embed the document's RGB profile, or color-mgmt sRGB for untagged documents. Explicit output profile handles still return Unsupported, never mislabeled samples.

## Runtime dependencies

Object/subject/sky require a real segmentation provider. Embedders install it through Documents::set_segment_model. The CLI does not bundle model weights or guess a model path; without a provider these operations fail explicitly without history changes. Wand, quick selection, colour range, refinement and boolean operations do not require models.
