# Vector paths and shape layers (M5-13)

Standalone CPU vector geometry and rendering. No changes to compositor or
engine-api are required to use this crate. See NEEDS.md for integration work.

## Model and use

`Path` contains open/closed `Subpath`s of `Anchor`s, each with a point and
incoming/outgoing cubic control points. Straight segments have coincident
handles. `FillRule` selects nonzero or even-odd winding. `Shape` retains live
rectangle (TL/TR/BR/BL corner radii), ellipse, polygon/star, line or custom path
properties. Call `Shape::path()` again after editing live properties. Oversized
corner radii are scaled together to fit adjacent rectangle edges.

`ShapeLayer` retains a shape, optional fill, optional stroke+fill, and affine
transform. `VectorRenderer::layer` evaluates these without caching a stale
raster. Stroke dimensions are local shape units; the affine transform applies
to both fill geometry and stroke outline. Fills are document-space paints.

`Viewport` width/height are output pixel counts, origin is level-zero document
coordinates and pixel size is 2^level. Render a tile by selecting its origin.
`coverage` returns row-major f32 [0,1] area coverage; `rgba`/`layer` return
row-major premultiplied f32 RGBA. Input colors are straight normalized RGBA in
the caller's color space. There is no implicit sRGB conversion or ICC handling.
Raster allocations are limited to 16,777,216 pixels per call.

## Geometry, booleans and accuracy

Curves are flattened with kurbo. Boolean operations use i_overlay's integer
scanline/overlay graph, not min/max combinations of antialiased masks. Each
operand is first normalized under its own fill rule, then union, difference,
intersection or xor is applied to canonical contours. Holes, touching edges
and self-intersections are resolved by the winding rule. Open paths are
implicitly closed for fill/boolean operations, but stay open for stroking.
Boolean output consists of line segments represented as degenerate cubics,
not reconstructed editable curve arcs.

Boolean topology uses a bounding-box-relative integer grid (roughly 29 bits
per half-extent). Details smaller than that grid may merge/disappear. Curve
flattening adds the supplied tolerance in geometry units. Rectangle tests use
exactly representable coordinates and produce exact expected areas. This is
not an arbitrary-precision CAD boolean kernel. Extreme coordinate magnitudes,
large dynamic ranges and tolerances far below machine precision are outside
the supported numerical envelope.

Coverage uses signed polygon/pixel intersection areas: normalize winding,
clip each contour against each pixel square, then sum signed shoelace areas.
This is analytic for the flattened polygons, including fractional rectangles
and holes, and does not require 16x supersampling. Curves are approximated at
`VectorRenderer::tolerance` (default 1e-5 output pixels). The unit-radius circle
rendered into a 2x2 mask is tested against pi with less than 0.1% relative error.
Ellipse construction itself is a cubic approximation via kurbo. This CPU
reference favors correctness over throughput; it clips contours per pixel and
is not a GPU or optimized active-edge rasterizer. Use bounded tiles.

Fills: solid, sorted multi-stop linear/radial/angle/reflected/diamond gradients,
and transformed repeating nearest-neighbor pattern tiles. Gradient interpolation
is straight RGBA, with optional deterministic document-space RGB dither of at
most half an 8-bit step; endpoints and alpha are not dithered. Angle zero lies
on the start-to-end ray and wraps at one turn. Reflected uses absolute axial
distance, diamond uses axial Manhattan distance. Paint is sampled at the pixel
center, while edge coverage is integrated. Very small/high-frequency patterns
and nonlinear gradients are not analytically prefiltered and can alias when
minified; this is a paint-filtering limit, not a coverage limitation.

## Strokes and transforms

Lyon tessellates stroke geometry with width, caps, joins and miter limit.
Consistently oriented triangles are unioned before rasterization, so overlap
at joins does not darken the stroke. Center uses width w; inside/outside uses
a 2w center stroke intersected with/subtracted from the original fill region.
Inside/outside alignment is rejected for open paths. Zero width is empty.
Lyon uses f32 internally; stroking is not f64-exact.

Dash entries must be finite and strictly positive. Odd patterns repeat twice;
offset wraps over the full pattern and resets at each subpath. Arc distances
use the flattened curve. Dashes crossing a closed seam are joined, not capped
twice. Caps apply at dash ends. Negative/zero entries are rejected rather than
silently reinterpreted.

`Affine` supports free transform, translation, rotate, scale, skew and flip.
`Perspective::from_quads` maps corresponding TL/TR/BR/BL corners via a full
homography; singular maps/horizon samples are errors. `MeshWarp` holds a grid
of tensor-product bicubic patches with editable 4x4 control nets (handles).
`MeshWarp::identity(domain, 1, 1)` supplies a default 4x4 net; larger grids are
supported. Adjacent patch edges must be edited together to keep continuity.
`inverse_map` uses Newton iteration for locally invertible meshes. Folded maps
have no guaranteed unique inverse and may fail to converge.

`Path::warp` subdivides cubics in output space (quarter/mid/three-quarter
samples, bounded recursion) and returns polylines. This is an adaptive
approximation, not exact rational Bézier preservation or a certified error
bound for arbitrary Warp implementations. Keep mesh paths inside the domain
and avoid homographies crossing the horizon. `content_aware_scale` explicitly
returns an unsupported error: seam carving belongs to the raster pipeline.

## UI and interop

Anchor hit testing, nearest cubic hit testing, anchor translation, mirrored or
independent handles, exact de Casteljau insertion, deletion and Catmull-Rom
curvature-pen construction are exposed. A standard pen can append/edit the
public anchors/subpaths. Path roles (work/saved/clipping/vector-mask) and custom
shape library persistence belong to the owning document/UI.

`PsdPathRecord` reads/writes exactly 26 bytes. `PsdVectorMask` reads/writes
version-3 vmsk/vsms payloads, retaining all raw selectors, flags, linked bits and
reserved data byte-for-byte. Geometry conversion validates knot counts and
open/closed selectors and converts signed big-endian 8.24 coordinates in
vertical/horizontal order. Coordinates are normalized to document dimensions;
`coverage` applies document scaling, disabled/inverted flags and initial fill.
Unknown records round-trip but cannot be interpreted as geometry. Regenerating
records from a Path writes unlinked knots, defaults initial fill to false and
requires even-odd geometry. To export nonzero geometry, boolean-normalize it,
then set the resulting canonical contours to even-odd. Keep original raw records
when lossless metadata preservation matters. Saving a full PSD and adapting
fill/stroke descriptors is not performed by this crate.

SVG supports path data through kurbo and a strict single-path XML wrapper
with `d` and `fill-rule`. Relative commands, quadratic/cubic segments and SVG
arcs are converted to the cubic model (arcs approximate). Full SVG layout/CSS,
viewBox transforms, multiple independently painted paths and filters are not
supported: unsupported attributes/elements are rejected, not silently dropped.

## Dependency licenses

Checked the downloaded Cargo.toml manifests for the locked versions:

- lyon 1.0.19, lyon_path 1.0.19, lyon_geom 1.0.19,
  lyon_tessellation 1.0.22, lyon_algorithms 1.0.21: MIT OR Apache-2.0.
- kurbo 0.11.3: Apache-2.0 OR MIT.
- i_overlay 4.5.2: MIT OR Apache-2.0; i_float 1.16.0, i_shape 1.18.0,
  i_tree 0.18.0, i_key_sort 0.10.3: MIT.
- quick-xml 0.38.4: MIT; thiserror 2.0.21: MIT OR Apache-2.0.

All geometry/rendering dependencies above are permissively licensed. The local
psd crate is a dev dependency for an independent parser compatibility test.

## Verification

Run from the workspace with CARGO_TARGET_DIR pointing outside the repository:

    cargo test -p vector --release
    cargo clippy -p vector --all-targets -- -D warnings
    cargo fmt --check

Tests cover circle area and pyramid levels, exact overlapping-rectangle boolean
areas, fractional pixel coverage, winding/hole behavior, stroke bounds/caps/
joins/dashes, gradients and pattern wrapping, live shape updates and layer
painting, projective corner mapping, mesh handles/inversion, pen/editing helpers,
PSD raw and geometric round trips (including the existing psd parser), SVG round
trips and malformed inputs. No compositor/engine-api files are modified.
