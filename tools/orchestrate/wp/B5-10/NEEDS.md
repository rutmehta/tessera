# B5-10 needs for Machine A (engine / out-of-scope files)

1. **Live text tile rasterization is O(tiles × glyphs) and re-lays text out per tile**
   (`crates/compositor/src/render/live.rs`, `Compositor::live_tile`). Every output tile calls
   `TextRenderer::layout` (or `layout_on_path`) and `outlines`, then rasterizes EVERY glyph outline into the tile
   viewport, even glyphs far outside it. A content edit damages the full canvas, so each typing preview re-renders
   every tile of the view level. Measured in the app on a 5472 × 3648 document at Fit (level 1, 2736 × 1824) with
   274-px Helvetica: frame render grows from ~81 ms (1 glyph) to ~400 ms (16 glyphs), ~10 ms per glyph, and
   keystroke → presented frame reaches a median of ~0.75 s over a 43-character line (first key ~115 ms). In
   the Rust bench (`typing_preview_latency_20mp`, ignored, level 2, no photo layer) the preview call itself is
   < 0.1 ms and preview + frame has a 74 ms median. Wanted: cache layout + outlines per (model, transform, font
   snapshot) once per render, cull glyph paths by the tile's bounds (glyph bbox from the layout), and optionally
   report the text's bounds as the edit's damage instead of the full canvas.
2. **Font snapshot at other compositor constructions.** `document/filters.rs` (3 sites), `document/io.rs`
   (export composite) and `document/tools.rs` (sampling) construct `Compositor::new` without the shared font
   snapshot (`document/text.rs::shared_text_renderer`); they discover system fonts themselves on first text tile
   (same installed fonts, so output matches, but each pays a full system-font discovery). These files are outside
   B5-10's allow-list; a one-line `set_text_renderer(super::text::shared_text_renderer())` at each site fixes it.
3. **`rasterize_layer` / `ConvertToPixels` create a fresh system-font renderer** (compositor contract). Conversion of
   text in a font only present in a host-supplied database would fail; B5-10 only uses installed fonts, so this is
   consistent today, but a way to pass the renderer (or a font database) into `DocOp::ConvertToPixels` would let
   hosts keep one snapshot everywhere.
4. **Resident renderer rejects layer styles** (`styles in the resident document program require CPU composition`).
   B5-07 owns the CPU fallback in `document/render.rs`; the B5-10 conversion test compares CPU composites for that
   reason.
5. **No glyph direction / bidi level in `typography::Glyph`.** B5-10 derives RTL per glyph from cluster order plus
   a strong-RTL script range check (`text.rs::glyph_rtl`). Exposing the bidi level (or a direction flag) per glyph
   would make carets at isolated neutral / weak characters exact.
6. **No font ascent/descent per line in `typography::Line`.** Caret heights use 0.9 / 0.25 of the largest run size
   on the line. Exposing line ascent / descent would align carets and selection to the font metrics.
