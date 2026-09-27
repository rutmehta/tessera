# B5-12 needs for Machine A (engine / out-of-scope files)

1. **Compositor filter-cache lock held during stack evaluation can deadlock rayon**
   (`crates/compositor/src/render/smart_filters.rs`, `Compositor::filtered_source`). The cache mutex is held
   while every enabled stage evaluates, and `evaluate_transform` → `TransformOp::apply` runs on rayon. When the
   stack is first evaluated from inside the parallel tile render (`render_level` → rayon tiles → `filtered_source`),
   other tile workers block on the std mutex, and the holder's rayon join can steal a tile job that blocks on the
   same non-reentrant mutex: the process stops at 0 % CPU. Reproduced in the app (Save Rasterized PSD Copy of a
   4-stage stack). B5-12 works around it in `document/filters.rs::native_stack` by rendering one tile from the
   calling thread first (warming the cache). Other callers are still exposed: the CPU backend and the B5-07 CPU
   fallback in `document/render.rs`, `native_filtered` (B5-09) and any `Compositor::render_level*` of a smart
   object with enabled stages. Wanted: evaluate outside the lock (per-key in-flight marker / `OnceLock` per key),
   or use `rayon::in_place_scope` safe waiting.
2. **Geometry preparation of nonlinear stages is per-pixel CPU work**, also on the resident GPU route
   (`TransformOp::displacement` → `WarpInverseField::inverse`, `PreparedPuppetWarp::inverse_map`). Measured
   (`bench_warp_drag_20mp`, Apple M4 Max, release): a 5472 × 3648 Arc warp at bend 0.3 takes 4.4–5.1 s per
   geometry change (preview → level-2 read-back), rising with bend because Newton misses fall back to a linear
   scan of the 24 × 24 sampled field; Puppet `inverse_map` is linear in the triangle count per pixel (the
   rasterized PSD copy of a 1600 × 1000 stack with a puppet stage took 33.6 s). The render thread holds the
   session lock while it renders (render.rs), so the app waits on the first exact frame after Apply
   (1.8 s on 20 MP in the self-test). Wanted: a coarse inverse lattice (e.g. every 4th pixel + bilinear, as
   `Displacement` already supports) or GPU preparation, a triangle grid / BVH for puppet lookup.
3. **Content-aware protect masks are stored inline** (`transform::seam::ContentAwareScale::protect: Vec<f32>`
   inside the stage's serde params): a 20 MP child would be ~20 M JSON numbers (hundreds of MB as
   `serde_json::Value`) in the document, its history and every native save. B5-12 caps full-resolution protection
   at 4 MP children (honest error above) and quantizes values to 1 %. Wanted: a channel reference (ChannelId) plus
   sampling at evaluation, or a compact binary mask attachment.
4. **Warp source domain is fixed at the child origin** (`WarpMesh::width/height`, `uv × size`). The net therefore
   spans the whole child canvas, not the layer's content bounds (Photoshop warps the content box). Wanted: a source
   rectangle (origin + size) on `WarpMesh`.
5. **Stage outputs are clipped to the fixed child canvas** (compositor contract): warping or growing content past
   the smart object's canvas cuts it off. Wanted: an expandable child canvas / output bounds per stage.
6. **Content-aware scale has no incremental energy update**; on a 20 MP child a large change takes minutes on one
   bake. Drafts use the reduced proxy; the exact bake after Apply is slow.
7. **Puppet meshes come from the child source**, not from the output of the stages below the puppet stage (no
   engine API evaluates a stack prefix for the host); `from_alpha` is capped at 16,384 vertices, so large opaque
   layers are meshed from a coarser level (the app says so).
8. **PSD**: enabled transform stages (and all smart filters) are native-only in `compositor::psd::to_psd`; there is
   no Photoshop smart-filter / warp descriptor export. B5-12 offers File ▸ Save Rasterized PSD Copy… instead.
9. **Thumbnails** (layer and composite, document/render.rs) strip smart filters, so a layer thumbnail shows the
   un-warped source (render.rs is outside B5-12's allow-list).
