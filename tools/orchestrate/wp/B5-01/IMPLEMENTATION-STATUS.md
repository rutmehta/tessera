# B5-01 — FFI `DocumentSession`: implementation status

Branch `wp/B5-01`. Layered documents (the `compositor` crate) over UniFFI for the mac app's
document mode, following the `DevelopSession` conventions: `Arc` sessions created by `Engine`,
a `with_foreign` listener, IOSurface presentation, `EngineError`-backed `BridgeError` results,
and records (never pixels) across the bridge.

## What was built

- `crates/tessera-ffi/src/document.rs`: records, listener, session registry, `Engine` open
  calls, `DocumentSession` (reads, edits, history, presentation, output).
- `crates/tessera-ffi/src/document/render.rs`: the per-session render thread (coalesced
  frames), `ResidentRenderer` presentation with a straight-alpha unpremultiply pass, the CPU
  fallback, and thumbnails.
- `crates/tessera-ffi/src/document/io.rs`: open (`.tessera-doc`, PSD/PSB, flat JPEG/PNG/TIFF,
  library images through the export path), save (native, PSD/PSB), flat export (PNG/JPEG/TIFF
  with colour conversion and embedded ICC), merge down, flatten, marquee selections.
- `crates/tessera-ffi/src/lib.rs`, `backend.rs`: the engine now owns **one** `gpu_core::GpuDevice`
  (`Engine::shared_gpu`), created on first use; the develop backend
  (`pipeline_gpu::GpuContext::from_shared`) and document sessions (`GpuCompositor::from_shared`)
  both use it. No second Metal device is created.
- `crates/tessera-ffi/src/export.rs`: `Source` made `pub(crate)` so documents reuse its decoding.
- `crates/tessera-ffi/tests/document.rs`: tests and the ignored bench.
- `apps/mac/README.md`: the document bridge calls. Bindings regenerated with `build-ffi.sh`.

### Design notes

- **One `DocOp` per edit.** Every edit goes through `Document::apply` exactly once and is one
  history node; merge down, flatten, group/ungroup and blend changes that also switch a group's
  mode are one `DocOp::Batch`.
- **Interactive edits.** `interactive: true` applies the op to a scratch clone of the
  `Document` (history capped at 2) that the viewport and `layers()` show; the net ops (one per
  `(layer, props|adjustment|fill)`) are applied to the real document by `commit(label)` as one
  node. A non-interactive value of the control being dragged folds into the drag and commits it;
  any other edit, undo/redo/checkout, snapshot or save commits the drag first as its own node.
  After a commit the resident renderer finds identical programs and pages, so the committed
  state costs no recomposite.
- **Frames.** Mutations bump the document epoch and signal the render thread; everything that
  arrives while it works coalesces into one frame (41 edits produced 2 frames in the test). The
  listener gets `on_frame`, `on_layers_changed` (union of changed ids) and `on_history_changed`
  once per pass, on the render thread, with no session lock held.
- **Straight alpha.** `ResidentRenderer::present` writes premultiplied RGBA8, so the session
  presents into an intermediate texture and runs a small WGSL unpremultiply pass into the
  IOSurface texture (imported once per attached surface). The CPU fallback writes the same
  contract.
- **Layer rows.** `layers()` is flat pre-order with siblings top-first; `index` is the
  compositor child index (0 = bottom). Change detection for `layers_changed` compares layer
  `Arc` identities between states, so it is exact for every op, undo and checkout (edited
  layers plus their ancestor groups).

## Public API (UniFFI)

`Engine`: `new_document(width, height, depth: DocDepth, profile: Option<String>)`,
`open_document(path)`, `open_document_from_image(image_id, developed)`,
`document_session(id)`, `document_ids()`. Free function `blend_mode_names()`.

`DocumentSession`:
- reads: `id`, `info -> DocumentInfo`, `layers -> Vec<LayerNode>`, `layer(id)`,
  `set_selected_layers(ids)`, `set_listener(Option<DocumentListener>)`;
- edits (→ `DocumentUpdate`): `add_layer(NewLayer, name, parent, index)`, `duplicate_layer`,
  `remove_layer`, `move_layer(id, parent, index)`, `set_props(id, LayerPropsRecord)`,
  `rename_layer`, `set_visible`, `set_opacity(id, value, interactive)`,
  `set_fill_opacity(id, value, interactive)`, `set_blend_mode(id, name)`, `set_group_mode`,
  `set_clipped`, `set_locks`, `set_adjustment_json(id, json, interactive)`,
  `set_fill_json(id, json, interactive)`, `add_mask(id, MaskInit)`, `remove_mask`,
  `set_mask_enabled`, `set_mask_density`, `merge_down`, `flatten`, `group_layers(ids, name)`,
  `ungroup_layer`, `set_selection_rect(x, y, w, h, feather)`, `clear_selection`,
  `commit(label)`; plus `set_mask_linked(id, linked)` (no history);
- history: `undo`, `redo`, `history_items -> Vec<DocHistoryItem>`, `checkout_history(id)`,
  `snapshot(name)`, `snapshots`, `restore_snapshot(name)`, `set_max_states(n)`,
  `history_memory_bytes`;
- presentation: `plan_surface(w, h) -> DocSurfacePlan`, `attach_surface(id, w, h)`,
  `set_viewport(level, x, y, w, h, zoom)`, `set_display_headroom`, `refresh`,
  `detach_surfaces`, `layer_thumbnail(id, max_px)`, `mask_thumbnail(id, max_px)`,
  `composite_thumbnail(max_px)`;
- output: `save`, `save_as(path)`, `export_flat(path, ExportFormat, quality, ExportColor)`,
  `close`.

Records/enums: `DocDepth`, `DocLayerKind`, `DocGroupMode`, `DocRect`, `LayerLocks`, `LayerNode`
(incl. `revision`, `mask_linked`, `mask_density`, `knockout`, `background`), `NewLayer`,
`MaskInit`, `LayerPropsRecord`, `DocumentUpdate`, `DocumentInfo` (incl. `source_image_id`,
`selection_bounds`, `can_undo`/`can_redo`, `backend`), `DocHistoryItem`, `DocSurfacePlan`,
`DocFrameInfo` (level region, level-0 `canvas_rect`, level extent, zoom, epoch, `render_ms`,
`full_recomposite`, `blocks`), `ExportFormat`, `ExportColor`; trait `DocumentListener`
(`on_frame`, `on_layers_changed`, `on_history_changed`, `on_render_failed`).

Not exported (tests/benches): `DocumentSession::{read_level, document_state,
thumbnail_renders, wait_idle}`, `Engine::adopt_document`.

## Tests

`cargo test -p tessera-ffi -p compositor --release`: all green.
- `tests/document.rs`: **10 passed, 1 ignored (bench)** — layer order/depths/indices, blend
  names and pass-through, group/ungroup; undo/redo/checkout/snapshots/branching/pruning;
  `.tessera-doc` round trip (bit-identical readback) and same-path-same-session; PSD written
  in-test by `psd` opens with names/modes/opacity and saves back with an unknown tagged block
  intact, plus PSB; `open_document_from_image` on `fixtures/raw/sample.dng` (one 16-bit pixel
  layer of the developed size, 0.49 s); `export_flat` PNG vs the CPU composite (≤ 1 code
  value) and resident readback vs CPU (≤ 2e-3), JPEG/TIFF in other spaces; thumbnails cached
  per revision (render counter); interactive opacity/adjustment adds no node until `commit`,
  folding rules, undo over a pending drag; merge down (≤ 1 code of the composite) and flatten;
  coalesced frames, ring rotation, straight-alpha pixels, zoomed and clipped viewports.
- Whole run: tessera-ffi 95 passed / 6 ignored across its targets, compositor 65 passed /
  4 ignored (unchanged crate).
- `cargo clippy -p tessera-ffi --release -- -D warnings` (and `--tests`): clean.
  `cargo fmt --all -- --check`: clean.
- `(cd apps/mac && ./build-ffi.sh && swift build)`: bindings regenerate; the `TesseraFFI` and
  `TesseraCore` targets build. The `Tessera` app target fails on a **pre-existing** Swift 6.3
  strict-concurrency error in `Sources/Tessera/Cull/AssistController.swift:74/81` ("sending
  'self' risks causing data races"), reproduced with the bindings from `HEAD` (before this WP);
  that file is outside this WP's allowed paths.

## Measured interactive recomposite

`cargo test -p tessera-ffi --release --test document -- --ignored --nocapture`, the
COMPOSITOR.md §12.4 document (100 layers, 20 MP, 8-bit, all 27 modes, a pass-through and an
isolated group) adopted into a session, three 1368×912 surfaces at level 2, Apple M4 Max,
load average ≈ 13:

| Interaction | render_ms (change → pixels in the IOSurface, GPU complete) |
|---|---|
| Opacity drag, level-2 viewport (full recomposite, 4902 blocks) | median **3.77 ms**, p90 3.86, max 3.98 |
| Exposure adjustment-layer drag (101 layers) | median **3.66 ms**, p90 3.82, max 3.95 |
| Cold first frame (all pages uploaded, mips on the GPU) | 947 ms |

Target < 16 ms: met with a wide margin.

## Deviations and notes

- **`source_image_id` is session state.** `.tessera-doc` has no document-metadata field and
  `crates/compositor/src/format.rs` is outside this WP's paths, so the source image id is
  reported by `info()` but not saved. Likewise `mask_linked` (the compositor has no link flag
  or translation yet) is session state and has no effect on pixels.
- **History authors** are all `"user"`; `DocHistoryItem` ids are compositor history node ids.
  The engine-api `DocumentHistory`/`Action` record (invariant 16) is not materialized here —
  the compositor's history tree is the source of truth, labels are overridden per node for
  commits and composite ops. That arrives with the MCP document tools.
- **Thumbnails** use the CPU compositor on a one-layer document (layer content with opacity,
  mode and mask ignored; masks as grey through a white fill), at the finest pyramid level that
  fits `max_px`, so the surface is that level's extent (≤ `max_px`), not an exact resize. The
  cache key is `LayerNode.revision`, which includes property changes (a re-render on opacity
  changes is harmless).
- **`LayerNode.bounds`** is tile-granular (stored 256-px tiles) for pixel layers; pixel-exact
  bounds need a scan and are left to B5-04.
- **EDR:** frames are SDR RGBA8; `set_display_headroom` is stored only (the compositor's
  RGBA16F presentation is M5-08).
- **Merge down** composites the pair in isolation (lower layer at Normal/100 % with its mask
  baked in, the upper layer as it is) into a pixel layer that keeps the lower layer's id and
  properties; the lower layer must be pixel, fill, text or smart object. **Flatten** composites
  over white into an opaque Background layer, as Photoshop does. Both use the CPU compositor at
  level 0 (the reference).
- **Flat export** does not reuse the `export` crate's encoders: its codec is private and
  expects an RGB develop render, and that crate is outside this WP. `io.rs` encodes with the
  same libraries (`png`, `jpeg-encoder`, `tiff`) and converts colour with LCMS on the
  `color-mgmt` built-in profiles, embedding the target ICC. PNG/TIFF keep alpha (8-bit for 8-bit
  documents, 16-bit otherwise); JPEG is flattened over white.
- **PSD save** of documents with fill layers or edited text fails with the compositor PSD
  adapter's explicit error (it never discards content silently).
- **Extras** requested by M5-10 and added because they are small: `set_fill_opacity`,
  `set_mask_linked`, `mask_thumbnail`, `history_memory_bytes`, `LayerNode.revision`,
  `DocFrameInfo.canvas_rect`, `group_layers`/`ungroup_layer`, `rename_layer`, `set_locks`,
  `set_mask_density`, `document_session`/`document_ids`, `blend_mode_names`.
- `add_mask`'s parameter is named `mask` (`addMask(id:mask:)` in Swift), since `init` is a
  Swift keyword.
