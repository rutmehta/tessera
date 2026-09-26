# Typography

Editable type engine for M5-12, independent of the compositor implementation.

```rust
use typography::{TextModel, TextRenderer};
let mut renderer = TextRenderer::new();
renderer.discover_system_fonts(); // fontdb's platform-specific discovery
renderer.load_font_dir("/path/to/user/fonts");
let model = TextModel::point("Live text", "Helvetica", 36.0);
let rendered = renderer.render(&model, 2.0)?;
# Ok::<(), typography::Error>(())
```

This example requires the named font. For deterministic output, load explicit
font bytes into `renderer.fonts_mut()` and do not discover system fonts.
Font collections, family/weight/italic queries and PostScript names are
supported. Unknown families return an error, not an arbitrary substitution.

## Contracts

- Distances are document pixels, positive Y down. Point text's first baseline
  is the largest run ascender below Y=0. Left alignment starts at X=0 plus
  indents; centered/right point text extends left of that anchor. Tracking adds
  pixels per shaped cluster, including the final cluster. Positive baseline
  shift lifts glyphs without moving their baseline. Explicit leading is the
  line step and may intentionally overlap ink. Automatic leading is 1.2em.
- Paragraph text wraps at Unicode break opportunities. Greedy composition
  reshapes candidate lines, keeping ligatures and contextual shaping inside
  each line. Mandatory separators, bidi visual ordering, mixed character runs,
  alignment, inter-word justification (not the final/forced line), indents,
  paragraph spacing and box-height overflow are handled. An unbreakable word
  remains intact and sets overflow rather than breaking a shaping cluster.
- OpenType tags and variable axes are passed to Rustybuzz. The same axes are
  applied to ttf-parser outlines. Typography outlines are rasterized by
  tiny-skia with AA, fractional coordinates and no hinting. Rerendering at each
  zoom avoids bitmap scaling; allocation is capped at 64 megapixels and zoom
  at 4096. RGBA8 output is premultiplied sRGB; see the bridge contract below.
- `layout` returns flat metrics. `layout_on_path` places a single point-text
  line on any single-contour lyon path using an arc-length table and tangent
  rotation. Glyphs outside the contour are omitted and report overflow.
  `TextModel.path` stores stable Move/Line/Quadratic/Cubic/Close commands and an
  offset; `render` applies this path automatically. `render_layout` accepts an
  already positioned layout, useful for interactive fractional placement.
- Arc/Flag/Wave presets use a bilinearly sampled 64×8 deformation mesh over
  glyph contours before rasterization. Amount is displacement relative to
  text width. Zero bypasses deformation exactly. Curves are flattened with a
  zoom-dependent tolerance for rendering; shape export uses 0.01px tolerance.
- `outlines(model, layout)` exports lyon paths and per-glyph colour/source
  clusters, retaining exact quadratic/cubic curves when warp is zero. Layouts
  must belong to the same renderer/font database and unchanged model.
- Engine JSON is `{ "version": 1, "model": ... }`. BTreeMaps make tag order
  stable, defaults allow additive model fields, unknown fields/versions and
  non-finite metrics are rejected. Source remains editable after rasterizing.
- `import_tysh` maps string/font/size ranges and retains original engine bytes,
  affine and bounds, using the existing PSD crate. It is not an Adobe EngineData
  writer. Details in [NEEDS.md](../../tools/orchestrate/wp/M5-12/NEEDS.md).

## Deliberate limits

Vertical composition is an explicit unsupported error. Hyphenation is a
serialized placeholder, not a dictionary-based algorithm. Paragraph style is
shared across the model's paragraphs. This is a greedy line composer, not
Knuth–Plass. There is no automatic font fallback or Adobe Fonts activation;
unsupported characters use the selected face's .notdef glyph. Script guessing
is per directional/style span. Shape-bound wrapping, colour emoji/SVG glyphs,
faux styles, font matching and the Character/Paragraph UI are outside this
package. No engine-api or compositor source files were changed.

## Verification

`cargo test -p typography --release && cargo clippy -p typography --all-targets -- -D warnings && cargo fmt --check`

Keep `CARGO_TARGET_DIR` outside the repository on macOS. Tests bundle OFL
Noto fonts, so shaping/raster goldens do not depend on installed fonts. The
coverage golden is FNV-1a over row-major alpha for "office", 24px, zoom 1,
NotoSans-Regular: 63×21, bounds [0,6,63,27], checksum 9554203516772678598.
It was recorded from the real unhinted rasterizer, not a synthetic mask.

See `LICENSES.md` and `tests/fonts/README.md` for dependency/font licensing.
