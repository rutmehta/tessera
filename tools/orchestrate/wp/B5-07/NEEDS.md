# B5-07 needs: GPU-resident layer styles in the compositor (for Machine A)

engine-api was not changed. B5-07 needs no contract field. It does need compositor work, outside the FFI:

1. **The resident renderer refuses styled documents.** `ResidentRenderer::render_region`
   (crates/compositor/src/resident/mod.rs, around line 1132) returns `Unsupported("styles in the resident document
   program require CPU composition")` for any layer with effects. B5-07 added a CPU fallback in
   crates/tessera-ffi/src/document/render.rs (`// B5-07 begin/end`; approved by the coordinator). Frames and
   `read_level` of a styled document now go through `Compositor` on the CPU, and the GPU path is unchanged for
   documents without styles. The fallback is a stopgap: effects should render on the GPU.
2. **The CPU style path recomputes every effect plane for every tile.** `TileJob::emit_styles`
   (render/effects.rs) calls `styles::render` on the full-canvas source raster for each tile job. A frame costs
   roughly (tiles in view) × (full-canvas style render). Measured on an M4 Max, viewport 1368 × 912,
   drop shadow (size 20) + stroke (8 px), `bench_styled_large_viewport_frame`:
   | Document | Level | Styled frame |
   | --- | --- | --- |
   | 1024 × 768 | L0 | 0.77–0.84 s |
   | 2048 × 1536 | L0 | 13–26 s |
   | 4896 × 3264 (16 MP) | L1 | about 108 s |
   | 5472 × 3648 (20 MP) | L2 | fails |
   Caching the style planes per (layer content, styles, light) revision, or computing them per tile with a
   halo, would fix most of it.
3. **Pixel limit.** `render::styles` refuses a padded alpha canvas above `MAX_PIXELS` (16.7 M), so any styled
   layer on a document of about 16 MP or larger cannot be drawn at all ("style alpha canvas exceeds CPU pixel
   limit"). This applies at every viewport level, because the source raster is always the full canvas.
4. **Layer Knocks Out Drop Shadow** (Photoshop's default) is not modelled. With Fill 0 %, a drop shadow shows
   through the empty interior. That is a correct render of the current engine semantics, but it differs from
   Photoshop.
