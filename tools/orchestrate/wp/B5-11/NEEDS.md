# B5-11 needs from Machine A (engine)

Found while building live shapes, Pen / Direct Selection and vector masks. None were changed here (engine sources are
outside B5-11's allow-list); the host states each limitation instead of hiding it.

1. **PSD reopen fails for a shape with a vector mask and a large raster mask.** `compositor/src/psd/vector.rs`
   `export_bridge` stores the uncombined raster mask channels in the private `tvMk` tag as serde JSON (`Vec<u8>` as a
   JSON number array, ~4 bytes per sample). On a 5472 × 3648 document with a full-canvas "Reveal All" layer mask the
   tag exceeds `restore_bridge`'s 64 MB limit, so saving succeeds but reopening the PSD fails with
   `invalid argument 'PSD adapter': vector mask bridge exceeds limit` (the whole document, not just the mask).
   Reproduced by `--vector-selftest` step 378 (`evidence/vector-selftest.log`, "known engine limitation"). Suggested
   fix: store the channels as a binary payload (or compressed / base64) and bound them by channel size, not a JSON
   byte count; or omit a reveal-all raster mask. Until then the host cannot promise PSD round trips for that case.
2. **PSD export of pattern shape fills.** `to_psd` rejects a shape layer whose fill or stroke paint is a pattern
   (`PSD pattern shape fill is unsupported`), so the whole PSD save fails. Native files keep the pattern. The host
   flags it (inspector warning, engine note) and never claims lossless export. Needed: a `PtFl` / pattern resource
   writer, or a documented raster fallback for that layer.
3. **Preview cost of stroked / complex shapes.** A dashed, Inside-aligned 18 px stroke on a 2100 × 1400 px custom shape
   in the 20 MP document takes about 5.4 s per preview frame (`render_ms`), and a fill-only ellipse about 200–280 ms
   (conservative full-canvas damage plus CPU rasterization at the viewport level). The session lock is held while a
   frame renders, so the next interactive call waits for it. The host coalesces (one call in flight) and draws its own
   overlay at pointer rate, but pixels lag. Needed: cache the stroke outline per (model, transform) instead of per
   tile, damage only the old ∪ new bounds, and do not hold the state lock across rasterization.
4. **Summary bounds for live layers.** `Layer::affected_bounds` returns `None` for text and shape layers, so the
   Layers summary reports "Whole canvas". The shape bridge computes geometric bounds itself (`ShapeLayerRecord.bounds`)
   but `LayerNode.bounds` stays `None`.
5. **ShapeModel::validate** accepts Inside / Outside strokes on open paths; rendering then fails
   (`aligned strokes require closed paths`). The bridge validates this before mutation; the model validator could.
