# B5-10b needs for Machine A (engine crates, not edited here)

Every compositor the document bridge constructs now takes the shared font snapshot
(`crates/tessera-ffi/src/document/fonts.rs`). These engine-owned constructions still build a
system-font renderer of their own, so text in a family that is only in the host snapshot (the
bundled Noto Sans test fixture; any future app-bundled or user-activated font) fails there with
`font unavailable: <family>` although the viewport, export_flat, filters and merge down render it.
With installed fonts the output matches (same fonts, discovered again at a cost).

1. **PSD writer** (`crates/compositor/src/psd.rs`, `export_imported`): the merged composite is
   `crate::Compositor::new(64 << 20).render_level_rgba(..)` (~line 2381) and text / shape layer
   pixels come from `crate::rasterize_layer` (~line 2629). Wanted: `to_psd` taking a text renderer
   (or a `&Compositor`), e.g. `to_psd_with(doc, &Compositor)`, used for both.
   Minimal reproduction (tessera-ffi test crate, fixture font loaded with
   `load_text_fonts_for_tests(".../typography/tests/fonts")`): new U8 document, add a text layer in
   "Noto Sans", `read_level(0)` and `export_flat(.png)` succeed, `save_as("x.psd")` fails with
   `invalid argument \`live layer\`: font unavailable: Noto Sans`.
2. **Resident smart-object children and CPU stack** (`crates/compositor/src/resident/mod.rs`
   ~line 828 `ResidentRenderer::new` for `children`, `resident/filters.rs` ~line 302
   `Self::with_budget` for children and `StackRuntime::new` ~line 31 `Compositor::new(budget)`):
   `ResidentRenderer::set_text_renderer` only reaches `self.live`; nested child renderers and the
   CPU stack compositor rediscover system fonts. Wanted: propagate the renderer to children when they
   are created and to `stack.cpu` (and re-propagate on `set_text_renderer`).
   Minimal reproduction: as above, then `convert_for_smart_filters(text_id)`; `read_level(0)` on the
   Metal session fails with `font unavailable: Noto Sans` (the CPU compositor path renders it, and
   with a smart filter enabled the bridge presents baked pixels, so filtered smart objects work).
3. **`DocOp::ConvertToPixels` / `rasterize_layer`** (`render/live.rs` ~line 347 `Compositor::new(0)`)
   — B5-10 NEEDS 3, unchanged.
4. **Engine merge / rasterize helpers**: `edit.rs` `render_merge_source` (~line 1157, used by the
   engine's merge and layer-rasterize ops) and `psd/placed.rs` `render` (~line 160, placed smart
   object rendering) construct `Compositor::new(64 << 20)`. The bridge's own Merge Down / Flatten
   go through `composite_raster` (shared snapshot), so these only matter to engine-side callers.
5. **Documented missing-font contract**: a missing family fails the whole composite (every path
   above, including thumbnails). If the product later wants the rest of the document to render
   with the missing layer skipped or its last raster shown, that is a compositor decision; the
   bridge only reports what the compositor returns.
