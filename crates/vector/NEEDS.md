# Integration needs (engine-api unchanged)

M5-13 exposes `VectorRenderer`, `CoverageRaster`, `RgbaRaster`, `ShapeLayer`,
`Path`, paint/stroke types, affine/projective/mesh transforms and PSD/SVG adapters.
It deliberately does not change engine-api, compositor or psd.

1. Document contracts: add a versioned serializable path/shape description,
   stable path/subpath/anchor IDs, path role (work/saved/clipping/shape/mask),
   live properties, paints, stroke settings, and transform stack. The current
   engine-api document contracts describe tools but have no equivalent complete
   vector model. Avoid making engine-api depend on this concrete renderer.
2. Compositor adapter: replace/interpret `document::VectorMask.path`'s opaque
   JSON placeholder, evaluate `coverage` at the requested tile origin and
   pyramid level, and multiply coverage with existing raster-mask density and
   feather handling. Wire `ShapeLayer` output as a layer source. Raster output
   is interleaved, premultiplied f32; adapt to the compositor's representation
   deliberately, including unpremultiplication if the target is straight.
3. PSD adapter: route the raw vmsk/vsms bytes into `PsdVectorMask`, retaining raw
   records for lossless writing. Existing `psd::metadata::parse_vector_mask`
   exposes the same knot ordering, 26-byte records and normalized 8.24 values.
   `SoCo`/`GdFl`/`PtFl` are currently classified with retained descriptors by
   `parse_fill`; translate these into vector paints in the owning integration.
   Stroke/live-shape descriptor keys (`vstk`, `vscg`, `vogk`) need descriptor
   interpretation there. This crate does not claim full Photoshop descriptor
   fidelity or alter the PSD writer.
4. UI/actions: bridge hit/edit helpers to history transactions and tool calls;
   store live geometry, not only raster snapshots. Custom shape libraries,
   selection ownership and work/saved path names remain document-level concerns.
5. Non-affine stacks: apply `Path::warp` before coverage and decide whether
   strokes/paints transform in local or document space. The supplied layer
   convenience renderer currently retains an affine transform; perspective and
   mesh APIs work on Path directly. Mesh shared-edge editing must preserve
   continuity. Add invalidation by geometry revision and level when caching.
6. Color/render policy: convert document colors before supplying fills; choose
   a future pattern minification and gradient prefilter strategy. Coverage is
   area-linear, not gamma adjusted. Add cancellation/optimized scanline or GPU
   rendering if production workloads outgrow this bounded CPU reference.
7. Content-aware scale remains an explicit unsupported placeholder pending a
   raster seam-carving implementation, rather than pretending affine scaling
   is content-aware.

Coordinates: level-zero document pixels except PSD-normalized records. Viewport
origin is level-zero, dimensions are output pixels, and level n has pixel size
2^n. No engine-api public types or existing signatures changed in this package.
