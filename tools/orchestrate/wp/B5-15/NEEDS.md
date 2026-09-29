# B5-15 needs (for Machine A: crates/compositor, crates/filters; M5-37)

B5-15 did not edit engine crates. These are the engine-side findings and asks from its measurements.
Figures: Apple M4 Max, macOS 26, release builds; method in IMPLEMENTATION-STATUS.md.

## 1. The "memory regression from B5-14" is freed malloc memory, not retained objects (M5-37 item 4)

M5-37 NOTES item 4 ("app footprint 3.3 GB vs 1.6 GB after B5-14's run — likely GPU mip chains retained for
thumbnails") should be closed or re-scoped. Attribution of B5-14's document self-test (`TESSERA_DOC_PERF=1`,
60 × 18 MP layers), sampled with `footprint`, `vmmap --summary` and `malloc_history -allBySize`
(MallocStackLogging=lite) at the P17→P14 boundary and while idle after the run:

| When | pre-B5-14 (ee11f55) | B5-14 / B5-15 |
| --- | --- | --- |
| "done" line (what B5-14 reported) | 1577 MiB (after a 27-minute run) | 2610 / 1778 / 3403 MiB (25-second run) |
| 5 s idle after "done" | 1538 MiB | 1446 / 1370 MiB |
| 15–240 s idle | 1529 MiB | 1437 / 1361–1369 MiB |

* At the P17→P14 boundary the footprint is 3528 MiB, of which **1.5 GB is `MALLOC_LARGE (empty)`**: freed
  large allocations (the photo exports of the P17 loop and the Edit in Layers develop buffers) that the
  allocator keeps dirty until the process goes idle. No live allocation accounts for it
  (`malloc_history`: 1.3 GB live in total, including VM regions). It is returned within 5 s of idle.
  Graphics memory is ≈ 400 MB above idle at the same point (in-flight frames and export device buffers).
* Before B5-14 the same run lasted 27 minutes (3.5 s per edit, 1304 photo exports in the P17 loop), so the
  allocator had long since reclaimed it; B5-14's run is 25 seconds long and its "done" sample lands on the
  transient. Idle, B5-14/B5-15 are **90–170 MiB below** pre-B5-14.
* The document session's own memory at idle: resident renderer 358 MiB (406 live pages + level buffers,
  `TESSERA_DOC_PERF_LOG`), the surface ring (3 × 33 MB IOSurfaces at 4K), the document raster (149 MB,
  `open_document_from_image`). Composite thumbnails do not retain mip chains beyond these pages.

Measurement ask: sample footprints after ≥ 5 s idle (or call `malloc_zone_pressure_relief` first) when
comparing runs of different length.

## 2. Resident smart-filter stage cache: its own, smaller budget (compositor `resident/filters.rs`)

`StackRuntime` caches every stage result and every per-level final result with the renderer's page budget
(2 GiB), separately from the page pool. A filter drag creates one full-resolution result per tick (18 MP ×
16 B = 289 MB), so after 6–7 ticks the stage cache holds ≈ 1.9 GiB of results nobody reuses (measured
1930 MiB). B5-15 releases it when the interaction ends (`Renderer::trim_smart_filter_cache` re-installs the
evaluator, the only public way to clear it; that also drops child renderers and layer tables), but the peak
during the drag stays. Wanted:
* a stage-cache budget of its own (e.g. 3 × the child's level-0 size), and
* not caching the result of the *last* stage while a preview edits it (only prefixes are reused by a drag), or
  a `ResidentRenderer::clear_filter_cache()` that keeps children and pages.

## 3. Smart objects are re-keyed when a layer is replaced (compositor `DocOp::AddLayer`)

The FFI edits a smart filter list as `RemoveLayer` + `AddLayer` of the same layer with new filters. The added
smart object gets a new cache key (`SmartObject::key`), so the resident stage cache and the child renderer
start cold: committing a drag re-renders the child at level 0 and re-runs the whole stack once, although
the preview just computed it (test `drag_of_the_top_filter_reruns_only_that_stage`: 2 stages on commit).
Wanted: a `DocOp::SetSmartFilters { id, filters }` that keeps the key (or `AddLayer` keeping the key of a
smart object whose child state is the same `Arc`).

## 4. Premultiplied Gaussian and larger radii on the GPU (filters)

The app's CPU route blurs premultiplied colour (transparent neighbours do not darken edges); the engine's
`CompositorFilters` (CPU and `GpuFilters::apply_buffer`) blurs straight colour. The GPU route is therefore
limited to smart objects whose child is opaque. Radii above 32 px use area reduction whose grid follows the
evaluation region, so the CPU route (evaluated in 256–1024 px blocks) and the whole-image GPU route differ
there; the GPU route is limited to sigma ≤ 32. Wanted: a premultiplied mode on the resident Gaussian (or on
the stack plumbing around it) and a block-independent reduction grid, which would let the route cover
transparent layers and every radius.

## 5. Layer styles on canvases above 16.7 MP (compositor `render/styles.rs`, perf P15)

`style_planes` allocates its alpha canvas for the whole document canvas plus the style margin and refuses
above `MAX_PIXELS` (16 777 216): "style alpha canvas exceeds CPU pixel limit". Any document of about 16 MP
or more with a layer style cannot be exported, flattened or rendered at level 0 (both before and after
B5-15). B5-15's styled export measurements therefore use 4608 × 3072 (14 MP). Wanted: style planes over the
layer's bounds plus margin, not the canvas. A styled tile can also run for tens of seconds (hotspot 4),
which bounds how quickly a cancelled export returns (cancellation is checked per tile).

## 6. A lighter mip for the smart-object stage result

`filtered_buffer` evaluates every stack at the child's level 0 and reduces, so a fit-view drag (level 1–2)
pays full-resolution filtering per tick where the CPU preview filtered the view level with a scaled radius.
The result is more exact (it is the level-0 result reduced), but costs 18 MP of GPU work per tick. A
level-aware evaluation for previews (filter at `level` with the radius scaled, as the CPU route did) would
cut fit-view drags roughly by the level's area ratio.
