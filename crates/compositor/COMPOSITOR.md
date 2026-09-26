# compositor: maths and invariants (M5-01)

The layered document model and tiled compositor of spec 02 §1–2 and spec 04
§4. This file is the reference for the blend and compositing maths, the
cache and revision invariants, and what the GPU port matches. Built against
engine-api 1.1.0, which is unchanged.

| Module | Contents |
|---|---|
| `document` | `DocState`, `Layer`, `LayerKind`, `LayerProps`, `Mask`, `Fill`, `SmartObject`, `TextLayer`, selections |
| `raster` | `Raster`: tiled COW storage with per-tile revisions; `Depth` |
| `edit` | `DocOp`, `Document` (history tree, damage log), `paint_op` |
| `blend` | blend modes, Blend If, dissolve hash: the scalar CPU reference |
| `adjust` | adjustment layers |
| `render` | `Compositor`: tile programs, CPU executor, caches, mips, smart objects, dirty rects |
| `gpu`, `composite.wgsl` | WGSL port of the tile program |
| `format` | the `.tessera-doc` container |

## 1. Conventions

- Pixel rasters hold **straight** (non-premultiplied) RGBA in the document
  depth (`U8`, `U16`, or `F32`), planar, in 256² engine-api `Tile`s with no
  halo. Masks and selections have one channel. Selections are always `F32`.
- All maths runs in f32. Accumulators are **premultiplied** f32 RGBA. Cached
  composites (`Part::Root`, `Part::Group`) are premultiplied f32. The public
  outputs `render_tile` and `render_level` are straight f32 RGBA.
  `render_tile_premultiplied` returns the premultiplied form.
- `b` / `Cb` / `αb` is the backdrop (the composite below). `s` / `Cs` / `αs`
  is the source (the layer). A colour written without a subscript is
  straight.
- Colours are blended in the document's own encoding, which for 8/16-bit is
  gamma-encoded, as Photoshop does by default. "Blend RGB using gamma 1.0" is
  not implemented (§9).
- For 8/16-bit documents, adjustment outputs are clamped to [0, 1].
  Nothing is clamped in F32 documents. With inputs in [0, 1], every blend
  formula below stays in [0, 1].

## 2. Blend modes

### 2.1 Alpha compositing (separable and non-separable alike)

The layer's **shape** is σ = content α × effective mask × Blend-If weight
(§3). Its effective source alpha is `a = σ · opacity · fill`. With `B(Cb, Cs)`
the blend function, the result is (W3C Compositing 1 §5.8 and PDF 1.7
§11.3.6, written premultiplied):

```
co = a·(1 − αb)·Cs + a·αb·B(Cb, Cs) + (1 − a)·(αb·Cb)
αo = a + (1 − a)·αb
```

`Cb` comes from unpremultiplying the accumulator (0 where αb = 0). A pixel
with σ = 0 leaves the backdrop bit-for-bit unchanged. The formula is affine
in `a`, which is what §2.4 relies on.

### 2.2 Separable modes (per channel)

| Mode | B(b, s) |
|---|---|
| Normal, Dissolve | s |
| Darken / Lighten | min(b, s) / max(b, s) |
| Multiply | b·s |
| Screen | b + s − b·s |
| Colour Burn | 1 if b ≥ 1; else 0 if s ≤ 0; else 1 − min(1, (1 − b)/s) |
| Linear Burn | max(0, b + s − 1) |
| Colour Dodge | 0 if b ≤ 0; else 1 if s ≥ 1; else min(1, b/(1 − s)) |
| Linear Dodge (Add) | min(1, b + s) |
| Overlay | HardLight(s, b) |
| Hard Light | s ≤ ½: 2bs; else Screen(b, 2s − 1) |
| Soft Light (Photoshop) | s ≤ ½: 2bs + b²(1 − 2s); else 2b(1 − s) + √b·(2s − 1) |
| Vivid Light | s ≤ ½: ColourBurn(b, 2s); else ColourDodge(b, 2s − 1) |
| Linear Light | clamp(b + 2s − 1, 0, 1) |
| Pin Light | s ≤ ½: min(b, 2s); else max(b, 2s − 1) |
| Hard Mix | 1 if b + s ≥ 1 else 0 (Adobe: "sum ≥ 255 → 255") |
| Difference | \|b − s\| |
| Exclusion | b + s − 2bs |
| Subtract | max(0, b − s) |
| Divide | s ≤ 0: (0 if b ≤ 0 else 1); else min(1, b/s) |

Soft Light uses Photoshop's formula, not the W3C/Illustrator one (the W3C
D(b) variant differs by up to about 0.02). The 0/0 convention for Divide and
the ≥ at the Hard Mix threshold are our choices; no Photoshop reference was
available to check them against.

### 2.3 Non-separable modes (PDF 1.7 §11.3.5.3)

```
Lum(C)      = 0.3·R + 0.59·G + 0.11·B
ClipColor(C): L = Lum(C), n = min, x = max
              if n < 0: C = L + (C − L)·L/(L − n)
              if x > 1: C = L + (C − L)·(1 − L)/(x − L)
SetLum(C,l) = ClipColor(C + (l − Lum(C)))
Sat(C)      = max(C) − min(C)
SetSat(C,s) = (C − min(C))·s/(max(C) − min(C))   (0 if max = min)
Hue         = SetLum(SetSat(Cs, Sat(Cb)), Lum(Cb))
Saturation  = SetLum(SetSat(Cb, Sat(Cs)), Lum(Cb))
Color       = SetLum(Cs, Lum(Cb))
Luminosity  = SetLum(Cb, Lum(Cs))
Darker Colour  = Cs if ΣCs < ΣCb else Cb     (Adobe: "total of all channel values")
Lighter Colour = Cs if ΣCs > ΣCb else Cb
```

`SetSat` is written in its closed form. It sends the maximum channel to s,
the minimum to 0 and scales the middle one, which is exactly the PDF
max/mid/min definition, ties included. The WGSL port uses the same form.

**Dissolve:** a pixel is drawn at full alpha (with Normal) when
`h(x, y, seed) < σ·opacity·fill`, and is skipped otherwise. `h` is a
deterministic integer hash, identical in Rust and WGSL, of the pixel
coordinates *at the rendered level* and a per-layer seed.

### 2.4 Knockout

A knockout layer composites against a knockout backdrop `K` instead of the
running backdrop. The backdrop at the start of the enclosing frame is
`K_shallow`: transparent for isolated groups and clip groups, the entry
backdrop for pass-through groups, and the Background layer at the root.
`K_deep` is the Background layer's contribution, or transparent when there is
no Background layer. The layer's shape and opacity then choose between the
running result and that composite, as in the PDF knockout-group rule
(§11.4.6.2):

```
R   = composite(Cs with alpha = fill, over K)          (§2.1 with a = fill)
out = acc + σ·opacity·(R − acc)                          (premultiplied, all 4 channels)
```

Fill 0 reveals `K`, and fill 1 with opacity 1 shows the layer over `K`.
Because §2.1 is affine in `a`, setting `K = acc` recovers ordinary
compositing exactly, so knockout is a strict generalization. Clipped layers
ignore their own knockout. Photoshop's special handling of fill opacity for
the "special eight" modes (Colour/Linear Burn/Dodge, Vivid/Linear Light,
Hard Mix, Difference) is **not** modelled: fill multiplies alpha exactly as
opacity does.

## 3. Layer pipeline

**Effective mask:** `m' = 1 − density·(1 − m)`. Pixels outside a mask's
stored tiles read as the mask's default (reveal-all = 1, hide-all = 0).
Feather is stored but not rendered (§9).

**Blend If:** each of Gray, R, G and B has two slider pairs,
`[black_lo, black_hi, white_lo, white_hi]`, normalized to [0, 1]. One pair
applies to the layer's own colour and one to the underlying composite
(straight). Gray is `Lum` (§2.3).

```
lo(v) = 1                         if black_hi ≤ 0 or v ≥ black_hi
      = 0                         if v < black_lo
      = (v − black_lo)/(black_hi − black_lo)
hi(v) = 1                         if white_lo ≥ 1 or v ≤ white_lo
      = 0                         if v > white_hi
      = (white_hi − v)/(white_hi − white_lo)
w     = Π over 8 slider pairs of min(lo(v), hi(v))
```

Unsplit sliders are hard steps, inclusive at both ends. A black slider at 0
or a white slider at 1 never excludes a pixel. This matters: a backdrop
channel of exactly 1.0 unpremultiplies to 1.0000001 on one backend and 1.0
on the other, and the GPU gate caught that as an excluded pixel before the
rule existed. It also keeps float documents' >1 values. The weight
multiplies σ. For adjustment layers, "this layer" is the adjusted colour.

**Clipping:** a run of `clipped` layers above a base becomes a clip frame,
which reproduces Photoshop's default "Blend Clipped Layers as Group":

1. Push a transparent buffer.
2. Composite the base with Normal, opacity 1, its fill and its mask.
3. Composite each visible clipped layer **source-atop**:
   `co = αb·(Cb + a·(B(Cb,Cs) − Cb))`, `αo = αb`.
4. Pop and composite the buffer with the base's mode, opacity, Blend If and
   knockout.

A hidden base hides its clipped layers. A clipped group composites as an
isolated group. Adjustment layers cannot be clip bases here: layers clipped
to one render unclipped.

**Groups:** *Isolated* groups (any real blend mode) composite their children
into a transparent buffer. The buffer is then a source with the group's
mode, opacity, fill, mask, Blend If and knockout. *Pass-through* groups
composite children straight onto a copy of the backdrop. On exit,
`acc = entry + t·(child − entry)` with `t = opacity·fill·mask`, so group
opacity fades the group's *effect*, as in Photoshop.

**Adjustment layers** apply to the composite below, keeping its alpha:
`co = αb·(Cb + w·(B(Cb, adj(Cb)) − Cb))`, `αo = αb`, where
`w = mask·opacity·fill·BlendIf`. With Dissolve, `w` goes through the dissolve
threshold. An adjustment at the bottom of an isolated group sees a
transparent backdrop and does nothing. In a pass-through group it reaches
through to the backdrop.

**Fill layers** are sampled at pixel centres in level-0 coordinates
(`(x + ½)·2^level`). **Text layers** render their rasterized proxy.

## 4. Adjustments

| Adjustment | Definition |
|---|---|
| Levels | per channel, then master: `ob + (ow − ob)·clamp((v − ib)/(iw − ib), 0, 1)^(1/γ)` |
| Curves | Fritsch–Carlson monotone cubic through the points (spec 02 §7), per channel then master, as 4096-entry LUTs with linear interpolation |
| Hue/Saturation | HSL: hue += h/360; sat k = s/100: `k < 0 ? S(1 + k) : S + (1 − S)k`; lightness in RGB: `k < 0 ? v(1 + k) : v + (1 − v)k`; Colorize sets H and S and uses L = Rec.601 luma |
| Exposure | `max(0, v·2^e + offset)^(1/γ)` in document encoding |
| Invert | `1 − v` |
| Posterize n | `min(n − 1, ⌊v·n⌋)/(n − 1)` |
| Threshold t | `Y601 ≥ t ? 1 : 0` |
| Channel Mixer | `out_i = Σ_j m_ij·v_j + c_i`; monochrome uses row 0 |

These are display-referred operators on the document encoding.
pipeline-cpu's operators are scene-referred linear Rec.2020, and reusing them
would pull `raw-decode`/LibRaw into the compositor, so they are not reused.
Photoshop's per-range Hue/Saturation bands, Brightness/Contrast, Colour
Balance, Black & White, Selective Colour, Gradient Map, Photo Filter and
3D LUTs are not implemented yet.

## 5. Revisions, stamps and caches

**Revisions:** one process-wide clock. Every `DocOp` gets one fresh
revision, strictly greater than every earlier one. Loading a file advances
the clock past every revision it contains. Revisions are stored on each
raster slot (tile or tombstone), on `props_rev`, `content_rev` and
`DocState::root_rev`, and on `DocState::rev` (the op that produced the
state).

**Stamp:** `Layer::stamp(level, tx, ty)` is the maximum over the layer's
props and content revisions, the slot revisions of its raster and mask in
the tile's level-0 footprint, its children's stamps (groups) and the child
document's revision (smart objects). The root stamp also includes `root_rev`.

**Invariant (stamp → content):** within one document lineage, equal
`(node, stamp, tile)` implies identical rendered content. Proof sketch:

- Revision r is created by exactly one op, which is exactly one history
  node N.
- A state containing r in a footprint descends from N, because checkouts
  restore whole states and never mix them.
- Every later write in that footprint carries a revision greater than r.
- So any state whose footprint maximum is r has the same footprint contents
  as N.

This requires:

1. Erasing leaves a **tombstone** slot, so footprint maxima never fall.
2. `AddLayer` and `SetMask` **restamp** all incoming slots with the op's
   revision, so a caller-built raster cannot reuse old revisions.
3. `Document::clone` gets a **new cache key**, because diverging clones
   would break lineage.

A consequence is that undo and history jumps hit caches from earlier
renders.

**Caches** (`RenderCache`: byte-budgeted LRU, key
`(doc key, node, part, stamp, coord)`):

| Part | Content | Stamp |
|---|---|---|
| Content / Mask | a raster's mip tile, document depth, straight | raster footprint revision only, so opacity and mode edits never invalidate mips |
| Group | isolated group composite, premultiplied | group stamp |
| Smart | smart object resampled into the parent tile, straight | max(child `rev`, layer `content_rev`) |
| Root | document composite, premultiplied | root stamp |

Invalidation propagates up the tree because stamps are maxima: an edit
raises the stamp of the edited node, of every ancestor group and of the
root, and only in the edited tiles' footprints. Nothing is ever explicitly
evicted for correctness. Stale entries age out of the LRU.
`tests/caching.rs::cache_invalidation_is_local_to_the_edited_tiles` checks
exact counts:

- Cold L1: 3 layers × 4 mips plus 4 composites.
- Warm: nothing is recomputed.
- A dab in one level-0 tile: 1 mip and 1 partial composite, 3 blends.
- An opacity change: 4 composites and 0 mips.

**Mips:** level n+1 is the recursive 2×2 box of level n, clipped at odd
edges. Colour is alpha-weighted: `C = Σαᵢcᵢ / Σαᵢ`, `α = Σαᵢ / count`.
One-channel rasters use a plain mean. Levels are computed lazily through the
cache and stored in the raster's depth. Levels beyond the one-tile level are
allowed, up to `MAX_LEVEL = 24`.

**Smart objects:** a parent pixel centre at level L maps through the inverse
transform into the child's level-0 space. The child level is
`Lc = ⌊log₂(2^L·√|det T⁻¹|)⌋`, and sampling is bilinear on the child's
*premultiplied* composite at `Lc` (transparent outside), then unpremultiplied.
Child composites are cached under the child's own key. With the identity
transform the result is pixel-exact. With a ½ scale it is exactly the
child's level-1 composite (tested).

## 6. Tile programs, CPU executor and GPU port

For one tile, the tree is flattened into a program:

- `Blend{src, params, mask}`
- `Adjust`
- `Push(Isolated | Clip | PassThrough)`
- `Pop{params, mask, pass, cache}`
- `SnapshotBackground`

Isolated groups with a cached composite become a single `Blend` of that
tile. The CPU executor runs one op at a time over a (sub-)rectangle of the
tile on planar f32 buffers. The per-mode inner loop is monomorphized. The
common case (no knockout, Blend If, atop or Dissolve) is a branch-free,
bounds-check-free row loop that LLVM vectorizes. It is the same maths as
`pixel::blend_px`, and the reference tests hold at 2e-6.

The GPU port (`composite.wgsl`) interprets the same program per pixel, with
an 8-deep private stack of premultiplied accumulators. The CPU resolves the
sources (mips, masks, fills, smart objects, cached groups) through the same
caches and uploads them. Every formula above is mirrored exactly. Adjustment
layers return `Unsupported` and the caller uses the CPU path.

Gate, as in docs/11 §1.3:

- Each of the 27 modes over a 4-layer stack: max |GPU − CPU| ≤ 2.4e-7,
  except Saturation at 2.3e-6.
- A 39-node chain with every mode, pass-through and isolated groups,
  shallow and deep knockout, masks, Blend If, a clip group with Dissolve and
  a radial gradient fill, at levels 0 and 1: 3.6e-4, which is under the 2e-3
  chain bound.

The chain error comes from threshold modes (Hard Mix, Darker/Lighter Colour)
amplifying 1-ulp upstream differences.

## 7. Dirty-rect compositing

Each `Document::apply` logs `(rev, damage)`, where damage is the level-0
canvas region whose composite may change:

- Paint ops: the tile rectangles ∩ the op's `dirty` rectangle.
- Property, structure and mask changes: the layer's content bounds.
- Adjustment, fill, smart-object and procedural changes: the full canvas.

This is sound because every op except smart-object resampling is
**pixel-local**: no neighbourhood operators, and mips are handled by rounding
outward. The compositor remembers, per `(doc, tile)`, the `(epoch, stamp)`
it last rendered. When the new stamp is higher in the same epoch, it unions
the logged damage in `(old, new]`, clipping each entry to the tile first,
then:

- recompositing only that sub-rectangle into a copy of the previous tile, if
  the damage covers at most ½ of the tile;
- re-keying the old tile, if the damage misses it.

Undo, redo and checkout bump the epoch and clear the log. The log keeps 4096
entries, and older stamps fall back to a full recomposite. Partial results
are bit-identical to a cold full render
(`dirty_rect_compositing_is_bit_exact`).

## 8. History and the file format

Each history node stores an `Arc<DocState>` snapshot. Unchanged layers and
tiles are `Arc`-shared, so a paint op costs one tile per touched tile
(`history_bytes` counts distinct buffers; tested). History is a tree:

- `undo` moves to the parent.
- `redo` moves to the newest child.
- `checkout(node)` jumps to any node.
- Named snapshots pin nodes.
- `max_states` prunes the oldest nodes other than the current one and the
  snapshot targets.

`.tessera-doc` layout:

- An 8-byte magic.
- zstd-compressed chunks of raw little-endian tile samples, deduplicated by
  BLAKE3, so COW-shared tiles are stored once.
- A zstd-compressed JSON manifest describing the whole model: canvas, depth,
  ppi, profile (ICC bytes as a chunk), layer tree with every property,
  masks, vector-mask payloads, adjustments, fills, nested smart-object
  documents with transforms and filters, text models and proxies,
  selection, and all revisions including tombstones.
- A 24-byte trailer.

Re-serializing a loaded document reproduces the input bytes exactly
(tested). History is not persisted, as with PSD.

## 9. Not done / deviations

- The fill-opacity behaviour of Photoshop's "special eight" modes (§2.4).
- Layer styles (spec 02 §1.4), smart filters (stored only), mask feather
  (stored only), vector-mask rasterization (payload stored only), and the
  translation op and position lock semantics.
- "Blend RGB colours using gamma 1.0" and colour conversion between
  profiles. The profile is stored and resolved by `color-mgmt`, and the
  compositor does not need a CMM.
- GPU adjustment layers, and GPU-resident caches: sources are uploaded for
  every tile, and the GPU path is a correctness port, not the interactive
  path yet.
- Band-parallel rendering for levels with few tiles. At level 2 of 20 MP
  there are only 24 tiles across 10 threads, and single-thread time is
  360 ms against 75 ms on 10 threads.
- Pixel-exact agreement with Photoshop is not claimed. The formulas are the
  published ones, but there was no Photoshop to diff against. The Divide 0/0
  and Hard Mix tie conventions are assumptions.
- Brush engine and selections tools are out of scope. `paint_op` is the
  primitive a brush engine emits.

## 10. Bench

Run it with:

```
cargo test -p compositor --release --test bench -- --ignored --nocapture
```

The document is 100 semi-transparent 8-bit pixel layers at 5472×3648
(20 MP): 10 distinct layers plus 90 COW duplicates with one repainted tile
each. It cycles through all 27 modes, adds Blend If on some layers, and has
one pass-through and one isolated group of 10. Measured on an M4 with
10 rayon threads while other builds were running:

| Measurement | Result |
|---|---|
| Composite at level 2 (1368×912, 24 tiles, 2400 layer blends), layer mips resident, composites cleared | **median 73 ms**, min 70 ms |
| Same, 1 thread | ≈ 360 ms |
| Cold: 11 200 mip tiles from 20 MP × 100 layers, plus composite | 1.8 s |
| Warm (root cache hits) | 1.2 ms |
| 64² brush dab → level-2 update (2 partial tiles, 6 mips) | 4.0 ms |
| 32² dab → level-0 dirty-rect update (2 partial tiles × 100 layers) | 0.70 ms |
| History for 100 layers | 821 MB (the 10 distinct 80 MB layers plus 90 tiles) |

## 11. engine-api (unchanged; fields that would help)

- A generic tile memo key for non-pipeline nodes, for example
  `NodeMemoKey { namespace, node, stamp, tile }`. `MemoKey` is tied to
  `ImageId` and `StageId`, so the compositor keeps its own LRU instead of
  image-core's `TileCache`.
- `DocumentId` and `LayerId` in `id`, plus layer tool calls in `tools`
  (spec 10), so MCP can address layers.
- A premultiplied/straight flag on `TileLayout` or `Tile`. Premultiplication
  is currently a convention per cache part.
- The `Pyramid::level_count` default stops at the one-tile level. The
  compositor renders deeper levels, and a `max_level` hook would expose that.
- Not engine-api, but related: `pipeline-gpu::GpuContext` depends on
  image-core, raw-decode and LibRaw, so the compositor creates its own Metal
  device. A small shared GPU-context crate would let them share one.
