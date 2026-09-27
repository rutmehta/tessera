# Editable text and vector host contract

`LayerKind::Text { model: typography::TextModel, transform: Affine }` and
`LayerKind::Shape { model: vector::ShapeModel, transform: Affine }` retain editable
sources. `Affine.m` is row-major `[a,b,c,d,e,f]`: `(ax+by+c, dx+ey+f)`.
Typography distances and shape geometry are local level-zero pixels; the affine
maps them to document pixels. Vector masks are already in document coordinates.

`ShapeModel.path` is the stored render geometry. `live_shape` retains construction parameters
for rectangle/radii, ellipse, polygon/star, line or custom-path controls.
`AddShape` and `EditShape` regenerate the path from `live_shape` whenever present,
so live parameter edits change pixels in one history entry. Clear `live_shape`
when submitting an arbitrary path edit. For direct layer construction, use
`ShapeModel::from_shape(shape, fill, stroke)` to build matching geometry. Fill and
stroke are optional and both can be present. Stroke retains alignment, width,
dash pattern/offset, cap, join and miter limit. Fill supports solid, gradient and
pattern values. Gradient/pattern sampling uses document-space coordinates,
matching the vector renderer; changing the layer affine moves geometry but does
not move its paint coordinates. Vector crate serde preserves these sources in native files.

## Editing and history

`Document::apply` accepts `AddText`, `EditText`, `EditTextRuns`, `AddShape`,
`EditShape`, `SetVectorMask`, and `ConvertToPixels`. Each successful application
is one history entry; `Batch` is an atomic entry, including rollback if any nested
operation fails. Add operations return the new layer ID in `Applied.created`.
Add `parent: None` means root; indexes are bottom-first and clamp to child count.

`EditText`/`EditShape` replace the complete model and transform. `EditTextRuns`
replaces a half-open range of **run indexes**, not UTF-8 byte offsets, Unicode
scalars or visual glyphs. An empty range inserts; replacement runs may be empty
or carry independently styled text. Paragraph, area/point geometry, warp and
path properties remain intact. Out-of-range/reversed ranges fail without history
changes. Both models validate before mutation; singular/non-finite transforms
fail. Pixel/all locks block content edits and conversion. Position locks block
changes to the transform while allowing edits to local source content.

`ConvertToPixels` rasterizes only source content at level zero to document depth.
It retains ID, name, layer properties, raster mask and vector mask, so opacity,
styles and masks continue to apply once. Undo restores the exact editable source.
Only text and shape layers accept this operation. `Layer::raster()` returns
`None` for live layers; callers wanting pixels use `rasterize_layer`.

`VectorMask { path, enabled, feather, density }` uses closed-path coverage and
`1 - density * (1 - coverage)`. Density is finite in `[0,1]`, feather is a finite
nonnegative level-zero pixel radius; disabled masks are ignored. `None` removes
one. Raster and vector masks can coexist. Masks do not move automatically when a
text/shape affine changes; hosts must explicitly transform their mask path if
that is the intended gesture.

## Selection, caret and hit testing

No compositor caret, selection, or hit-test object is provided. Hosts own transient
caret/selection state. For text, use the same font database as the compositor via
`Compositor::set_text_renderer`, invert the layer affine, and obtain local layout
with `typography::TextRenderer::layout` (or `layout_on_path` for path text). Layout glyphs expose run index, local
origin/advance and cluster index; cluster indexes are UTF-8 byte offsets in the
concatenated source. Lines expose source ranges and baselines. Preserve valid
UTF-8 and grapheme/cluster boundaries when deriving a caret from pointer input;
ligatures and bidirectional text are not one glyph per character. Split affected
runs at valid text boundaries and submit their replacement via `EditTextRuns`.
IME composition should remain host state until committed as one operation.
Warped/path text requires the host to account for glyph rotation and geometry;
there is no automatic inverse-warp caret mapping.

For shape hit testing, invert the affine and call `Path::contains` on
`model.path`. Also test the outline produced by `Stroke::outline` when stroke
hits are desired. Honor its
fill rule. For visible-pixel hit testing including masks, use rendered coverage;
geometric path hits and visible-alpha hits have different semantics. Host handles
and selections should derive from local geometry then apply the affine.

## Rendering, persistence and cost

CPU and resident rendering share output-level rasterization of live source and
vector-mask coverage through `Compositor::live_tile`. Positioned text outlines
and transformed shape fill/stroke paths are prepared once per model/transform
while resident in the geometry memo. Typography caches paragraph layouts,
shaped lines (with paragraph-local source offsets and bidi ordering), and
positioned glyph outlines. Font-database mutation clears typography caches;
`set_text_renderer` clears compositor caches as well.

Layer rasters are sparse collections of potentially occupied output tiles. Their keys contain
only the ordered geometry and paints intersecting the tile, canvas, depth and
level. Glyph/path coverage is cached separately from paint, keyed by exact
positioned outline geometry (therefore font, axes, size and transform), output
level and tile origin. Appending text retains unchanged glyph coverage and
source tiles. Changing glyph positions regenerates the affected coverage.
Rasterization retains the original tile coordinate system and vector area
arithmetic for exact parity; it does not translate previously rounded bitmaps.
Tiles outside geometry bounds are not stored. Stroke bounds come from the actual stroke
outline, including alignment, joins, caps and dashes.

For CPU pixel-local scenes containing live layers, root tile identities include
intersecting live geometry plus ordinary layer revisions, hierarchy, properties
and masks. The common ordered prefix/suffix of old/new geometry determines the
union of changed run/glyph or fill/stroke bounds. Small damage recomposites only
that rectangle; other tiles reuse their completed composites. This also handles
movement, deletion, reflow, undo and skipped intermediate revisions. Document
history still records conservative damage; no document/edit ABI was changed.
Styled live layers expand changed bounds by the finite effect support (including
offset sampling and bevel derivatives); nested styled ancestors add their support.
Only intersecting root tiles/regions recompose. Global light and style properties
remain in cache identity, and masks/pixel sources feeding effects retain conservative
revision dependencies. Neighborhood adjustments still use full revision-based
recomposition. Effect-source evaluation itself remains the existing whole-source
CPU barrier, so styled editing has no interactive latency claim.

The resident GPU path benefits from the shared prepared source and coverage
caches, but its upload/damage scheduling still uses its existing revision keys.
M5-31 can wire the same spatial identity/damage approach into that scheduler;
this package does not modify resident code or claim GPU frame latency.

Small warm CPU live viewports (up to 32 tiles, roughly 2MP) execute without
thread-pool fanout; cold frames and larger outputs retain parallel rendering.
This avoids scheduling and shared-cache contention when most tiles are hits.

The live source/mask tile LRU has the compositor's supplied byte budget; coverage
has half that budget, geometry one quarter (minimum 1 MiB), and CPU damage
snapshots one eighth. These are separate from the existing composite/filter
budgets. Typography's three memos each have an 8 MiB limit. Geometry/typography
memos use FIFO eviction; tile coverage uses LRU. Eviction loses reuse, never
correctness. `live_tiles`, `live_preparations` and `live_coverages` expose work
counts. Cache byte/entry counters retain their existing composite-cache meaning.

Limitations: paragraph placement is recalculated when incoming baseline/box
geometry changes, with shaped-line reuse where inputs agree. Warped/path text
uses the full positioned-outline preparation, and complex or very large paths,
styled layers, cold font discovery, cold photo mip generation, and insufficient
cache budgets can exceed the measured warm editing latency. Glyph coverage is
position-specific, not a font atlas with approximate subpixel phases. A new
coverage miss still runs exact polygon clipping. The vector renderer shares the
first two (horizontal) clipping planes down each pixel column, preserving the
original clipping order and area sums while avoiding repeated contour scans. Sources are rasterized on CPU
before GPU upload, not by native GPU curves.

The compositor discovers system fonts by default. A host-supplied typography
renderer provides controlled fonts; missing fonts and unsupported typography
features can return rendering errors. `ConvertToPixels` and the free
`rasterize_layer` helper create a fresh renderer using system fonts; they do not
inherit a custom font database installed on another compositor. Hosts must ensure
those fonts are available for matching conversion/export. Font databases are not
automatically propagated to nested smart-object renderers. No font files or font database IDs are
embedded in native text models. Native files preserve models, transforms, masks
and properties; raster caches and history are not persisted. Legacy text records
upgrade their text/font/size/color to a live model (their old raster proxy is not
retained); simple legacy polygon-array masks upgrade to typed paths. Arbitrary
opaque legacy vector-mask JSON is unsupported and fails loading rather than
silently changing it.

Text colors currently normalize the model's sRGBA bytes directly into document
samples. Vector paints are caller-supplied document-space colors. There is no
automatic per-layer ICC/transfer conversion. F32 does not identify an encoding:
linear/non-sRGB hosts must account for this limitation before combining live text
with their document colors; selecting linear display output does not convert
text source colors. Feather uses a Gaussian with a three-sigma halo, limited to
1024 output pixels; exceeding that limit returns a resource error.

## PSD interchange

Editable text uses a real `TySh` descriptor and Adobe's dictionary-based
`EngineData`: UTF-16 run lengths, font references, size, tracking, leading,
baseline shift, faux bold/italic, kerning, ligatures, color, paragraph alignment
and spacing, point/area bounds, orientation and warp. Original unknown
EngineData dictionary fields and TySh descriptor fields are retained while
modeled fields are updated. `typography::TextModel::to_engine_data` is the older
**native JSON envelope** API; it is not written to PSD EngineData. The Adobe
serializer is `typography::export_engine_data`.

Shapes use standard `vmsk`/`vsms` paths, `SoCo`/`GdFl` fill descriptors, `vstk`
strokes and `vogk` origination descriptors. Rectangle/rounded-rectangle/ellipse
construction parameters have standard origination fields. Paths retain curves;
fill and stroke pixels are also refreshed as each layer's cached raster.
Unknown source tags and descriptor fields remain retained. Unsupported source
syntax remains an opaque raster-backed layer instead of being silently discarded.

Private `tvTx` and `tvSh` tags supplement those real Adobe records with the exact
native model (including variable axes, arbitrary feature settings, polygon/star/
line controls and the original shape affine). A digest of the standard records
prevents stale native controls from overriding subsequent external edits.
Applications ignoring these private tags still receive standard text/shape
records and refreshed pixels. Shape paths bake the affine; uniform scaling also
scales standard stroke widths/dashes. A nonuniform affine's stroke can differ
when another application rerenders its standard descriptor; Tessera restores
the exact native source and renders it correctly. Pattern shape fills are not
yet exported. PSD gradient geometry uses document-relative angle/scale/offset;
more advanced Adobe gradient interpolation is not modeled.

A shape's primary path already occupies its PSD vector mask. An additional
independent vector mask is therefore written as a standard raster user mask,
combined with any enabled raster mask using the same coverage/density/feather
calculation as rendering. The private `tvMk` tag preserves the editable vector
mask and uncombined original raster mask. Its digest also defers to external
raster-mask edits. Native round-trip restores both masks; applications ignoring
`tvMk` see the combined standard raster mask. Pixel and text layers use ordinary
PSD vector masks, including disabled state, density and feather parameters.

The existing FFI summary ABI temporarily reports shape layers as `Fill`; text
continues to report `Text`. This work changes only the exhaustive compatibility
match arms. Machine B owns the shape-specific FFI enum and editing/UI surface.

On importing a standard un-stroked Adobe shape with a disabled, feathered, or
reduced-density primary vector mask, the adapter represents the paint as a
canvas-sized editable Shape and moves the original path/settings into its live
VectorMask. This preserves fill revealed outside the original path and feather
coverage; edit that mask path for the original outline. A stroked shape with
those nondefault primary-mask settings remains an explicitly reported raster
proxy because the combined Adobe stroke/mask semantics are not yet modeled.
`ConvertToPixels` removes previously decoded type descriptors during PSD export,
so reopening cannot resurrect stale text/shape source.
