# M5-12 integration needs (no engine-api/compositor edits)

Inspected `docs/02-photoshop-spec.md` §9, `compositor::document::TextLayer`,
`compositor::raster::Raster`, `engine_api::document::{LayerKindTag, NewLayer}`,
and `psd::metadata::{Text, TextStyleRun, EngineStyles}`.

## Existing contracts

- `LayerKindTag::Text` exists, but `NewLayer` cannot create text and there are
  no editable character/paragraph text fields or text-edit tool calls.
- Compositor `TextLayer` has only text, font, size, RGB colour and a straight
  RGBA tiled proxy. It cannot retain multiple runs, features, axes, paragraph
  parameters, warp or a baseline path. M5-12 does not modify this structure.
- PSD already parses TySh descriptors and basic EngineData FontSet/StyleRun.
  Typography uses those readers rather than introducing another PSD parser.

## Requested future fields / operations

1. Store `text_engine_data` (versioned JSON from `TextModel::to_engine_data`)
   on text layers, separately from the disposable proxy. Store source TySh
   bytes for lossless preservation of properties not mapped by the basics hook.
2. Add create/update-text tool contracts accepting the versioned source. Text
   changes must invalidate the proxy and participate in document history.
3. Store layer transform, text origin, colour encoding/profile, and raster
   proxy bounds/zoom. A font-resource revision must also invalidate layout.
   Do not persist fontdb IDs: they are local to a renderer instance.
4. Preserve missing-font status and let the UI choose substitutions explicitly.
   Expose paragraph overflow and the vertical/hyphenation placeholder status.
5. Pass `GlyphOutline.path` (lyon quadratic/cubic contours; warped contours are
   flattened polygons) to M5-13's shape conversion. Converting must be explicit,
   not an incidental consequence of making a raster cache.

## Raster bridge and alpha contract

`TextRenderer::render(&model, zoom)` returns `RenderedText { raster, bounds,
zoom, layout }`. `typography::Raster` is a local, tightly packed premultiplied
sRGBA8 buffer, NOT `compositor::Raster`. `bounds` are signed, half-open pixel
bounds in zoomed document coordinates; bitmap (0,0) maps to bounds.x0/y0.
Divide bounds by zoom to position it in document coordinates. Empty text has
zero extent. Rerender from the editable model when zoom/transform changes.

The M5-08b compositor bridge must unpremultiply RGB (zero for alpha=0), convert
sRGB to the document encoding/profile, then populate straight-RGBA tiles of
the document depth and position at the returned bounds. Copying bytes directly
would darken edges. This separation avoids a dependency cycle when compositor
later calls typography and avoids any edits to M5-08b's active files.

## PSD hook

`import_tysh(&psd::metadata::Text)` returns an editable model plus the original
PSD affine order `[xx,xy,yx,yy,tx,ty]`, bounds, original engine bytes and warnings.
Descriptor ranges take priority; otherwise FontSet/StyleRun lengths are mapped
using UTF-16 units, including surrogate-boundary checks. Sizes are pixels at
72 dpi; apply the source document DPI for point sizes before layout. PSD's
paragraph box, warp, other styles and transformation are not guessed from
bounds. Full TySh export remains the PSD layer writer's responsibility.
