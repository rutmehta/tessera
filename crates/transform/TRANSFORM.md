# Transform geometry (M5-21)

Apache-2.0. `transform` has no compositor or engine-api dependency. The CPU API
consumes `Image { width, height, planes: [Vec<f32>; 4] }`: tightly packed planar,
finite f32 **premultiplied** RGBA. HDR RGB and signed reconstruction lobes are
not clamped. Dimensions are nonzero and limited to 100 MP. Publicly mutated
images are revalidated on entry. Operations never mutate input pixels.

## Coordinates, serialization and rendering

`TransformOp { version: 1, operation: Operation, kernel: Kernel }` is serde
serializable. Validate after deserialization; evaluation also validates and
rejects unknown versions. `Operation` supports Free, Warp, Perspective, Puppet,
ContentAwareScale, and Displacement. Geometry uses level-zero pixel-edge coordinates; the
first pixel center is (0.5, 0.5). At level L, evaluate the inverse at
`2^L * (x+0.5,y+0.5)`, then divide the source coordinate by `2^L`.

`apply(input, width, height, level)` renders into an explicit origin-zero
canvas. Geometric operations do not infer/expand the canvas. Outside source or
mesh coverage is transparent. `displacement(width,height,level)` emits absolute
source centers as f32 pairs (not offsets), with `[-1e20;2]` for missing geometry.
Content-aware scale has no geometry-only displacement field.

## Free transform and reconstruction

Forward homography: `q = (H00*x+H01*y+H02, H10*x+H11*y+H12) /
(H20*x+H21*y+H22)`. Inversion and mapping reuse `lens::Homography`; affine
reference-point helpers implement `p + A*(x-p)` for scale, rotation, shear and
flip. Positive rotation is clockwise in y-down images. A full 3x3 matrix also
expresses distortion/perspective. `bounds(w,h)` maps all four rectangle corners
and returns the exact real-valued min/max bounds. It rejects singular matrices
and any homogeneous denominator sign crossing/zero over the source rectangle.
For integer bounds, floor minima and ceil maxima; these bounds describe the
source rectangle, not the reconstruction kernel's extended support.

Reconstruction is separable with nearest, triangle/bilinear, Catmull–Rom cubic
(a=-0.5), and `sinc(x)*sinc(x/3)` Lanczos-3 kernels. Lanczos weights normalize
across the **full** six-tap support, not only the in-image taps. All four
premultiplied planes use identical weights; outside taps are transparent black,
so alpha edges do not acquire hidden straight-RGB colors. Signed cubic/Lanczos
lobes are preserved. F32 accumulation order is y-major then x-major without FMA.

Automatic chooses nearest for exact affine pixel-lattice permutations with
integer translation at the requested level, Lanczos-3 for affine area reduction,
and bicubic otherwise. This is not Photoshop's proprietary chooser and is not
scale-widened antialiasing; render from an appropriate input mip for large
reductions. Standalone `sample(..., Automatic)` defaults to bicubic because it
has no geometry context.

The repository's calibration crate (`lens`) contains the reusable homography,
but not public RGBA reconstruction kernels or the composed develop map. Those
currently live in `pipeline-cpu` and are tied to RAW/develop RGB, not this alpha
contract. This implementation reuses lens homography and implements a shared
transform CPU/Metal sampling contract rather than introducing the RAW pipeline
as a dependency. Extracting one public cross-pipeline kernel library remains
unimplemented; this is a deviation from the requested kernel-reuse requirement.

## Bézier mesh

A default mesh is one tensor-product cubic patch with 4x4 control points:
`P(u,v) = sum_i sum_j B_i^3(u) B_j^3(v) C_ij`. Split knot vectors partition
normalized UV; adjacent patches share control rows/columns. `split_u`, `split_v`
and `subdivide` use de Casteljau, preserving the represented surface exactly.
`inverse` returns normalized UV, not source pixels. The renderer multiplies UV
by the mesh's original width/height.

Inversion uses analytic Jacobians, up to 40 Newton iterations and damped steps
clipped to [0,1]^2. Failure falls back to a sampled triangulated displacement
field (24x24 cells by default), barycentric seeds and residual-verified Newton
refinement. Curved-boundary fallback also tries the nearest 16 sampled vertices.
Field snapshots can be reused. Resolution is 2..512. Invalid/collapsed nets are
rejected; folds are allowed and deterministically choose the first convergent
preimage, not a globally unique inverse. Extremely small features may be missed
by finite fallback sampling. Out-of-mesh results are None, not edge clamping.

Presets: arc, arc lower, arc upper, arch, bulge, shell, flag, wave, fish, rise,
fisheye, inflate, squeeze and twist. Bend is signed [-1,1]; zero is identity.
These are cubic approximations, not proprietary Adobe preset replicas.

## Perspective warp

Each user quad has perimeter-ordered source/destination corners. Compile its
homography with lens's inverse. Shared vertices must match in both spaces;
nonconvex/degenerate quads, interior overlaps, cracks and unsplit T-junctions are
rejected. Isolated planes remain projective. Shared-edge residuals are blended
linearly in each cross-edge parameter (Coons-style bilinear boundary blending)
so both sides agree on linear edge parameterization. The corrected map is
inverted with damped Newton iterations. This guarantees shared positional
continuity, not derivative continuity. Outside all quads is transparent.

## Adaptive Wide Angle (M5-27)

`adaptive::Adaptive` is a serde recipe with a manual `CameraModel` (rectilinear
or equidistant fisheye), or an embedded interpolated `lens::Profile` sample.
`CameraModel::from_profile` converts focal mm to pixels as
`f_mm * image_width / active_sensor_width_mm`. Native Brown, odd radial terms,
axis normalization and distortion scale are preserved. Current profile importers
only support rectilinear lenses; a fisheye is explicitly manual, not silently
interpreted as rectilinear. No new lens database or profile format is introduced.

For centered radius r, sphere angle is `atan(r/f)` (rectilinear) or `r/f`
(equidistant). Lift to `(sin(theta)*direction.x, sin(theta)*direction.y,
cos(theta))`, then project to `f_out * scale * crop_factor * ray.xy/ray.z +
source_size/2 - crop`. `crop` is a pixel offset into a fixed output canvas;
`crop_factor` is an additional field-of-view multiplier, default 1. Do not
double-count crop already included in the active sensor width or output focal.
No automatic largest-inscribed crop is performed. Rays at/behind the horizon
cannot be represented by the rectilinear output.

`LineConstraint` stores observed source-space polylines, weight and Straight,
Horizontal or Vertical orientation. Two endpoints mean a source-image chord;
use additional points to trace curved observed edges. Segments are densified
every 4 pixels (at most 64 pieces per segment). The camera-reprojected traces
feed a bilinear control mesh `F(p)=p+B(p)d`. Straight lines use a TLS normal;
horizontal/vertical use fixed axis normals, all through each trace's centroid.
The coupled least-squares objective is

`1000 sum_l w_l sum_p (n_l dot (F(p)-centroid_l))^2
 + smoothness sum_edges |d_i-d_j|^2
 + 4*smoothness sum_triples |d_i-2*d_j+d_k|^2
 + regularization sum_i |d_i|^2`.

Deterministic Jacobi-preconditioned CG verifies its true residual. The positive
identity anchor removes nullspaces. Damped Newton inverts the solved mesh before
inverse camera projection. Rejects nonconvergence, degenerate/conflicting lines,
excessive residuals and folds. A conservative positive symmetric Jacobian bound
of 0.05 at every cell corner guarantees injectivity but rejects large rotations.
Default fitted-line tolerance is 0.25 pixels; this is not a bound on arbitrary
unsampled photographic edges. Grid tests measure the final interpolated field
at independent points, not just the fitted vertices.

`solve()` returns `displacement::Displacement`. This repository's TransformOp
is a struct, so wrap the field as `TransformOp { version: 1, operation:
Operation::Displacement(field), kernel: Kernel::Bicubic }`, rather than an enum
variant named TransformOp::Displacement. The serde field contains absolute
source coordinates at integer destination vertices, `(width+1)*(height+1)`;
None means uncovered. Bilinear field evaluation produces the existing CPU/GPU
pixel-center lookup convention at any mip. Source-exterior cells are transparent;
an entirely uncovered output is an error. The existing resident GPU renderer
consumes this operation unchanged, with geometry prepared on the CPU.

Limits: 16,777,216 field vertices, source axes <=1,000,000, 3..65 controls per
axis (default 17), 256 lines, 16,384 input / 65,536 densified samples. The mesh
domain is padded to [-width/2, 3*width/2] on each output axis. Large-image field
preparation is not claimed interactive, and JSON fields can be large.

## Vanishing Point (M5-27)

`vanishing::VanishingPoint { planes, camera }` is the serde document-tool payload.
Each `PlaneSpace` stores corresponding perimeter-ordered canvas and unfolded
atlas quads. `from_quad(canvas_quad, size)` starts with a rectangular atlas.
Prepare once for pixel loops. Forward is `H=H_canvas*inverse(H_atlas)`, inverse
is `H^-1`, with homogeneous division and convex-quad coverage tests. Unlike
PerspectiveWarp, these remain pure homographies without bilinear seam blending.
Adjacent atlas and canvas edges must match in endpoints and projective midpoint,
which fixes the whole edge parameterization. Overlaps, T-junctions, degeneracy,
nonfinite coordinates and horizon crossings are errors.

`tear_off(parent, edge, width, angle_degrees)` reconstructs a camera-space plane
using `K^-1 H`, rotates its outward derivative around the shared 3D edge using
Rodrigues' formula and projects through K. The rank-one update vanishes on the
hinge, preserving every shared-edge point. Zero is coplanar and 90 perpendicular.
Supplied pinhole intrinsics, not inferred camera calibration, determine the 3D
interpretation. Occluding/edge-on/behind-camera folds are rejected atomically.
Arbitrary initial quads define a projective, not necessarily metric, atlas.

Clone source mapping is `H_source(H_destination^-1(canvas)+offset)` with offset
in the common unfolded atlas. Source and destination can be on different planes;
outside all planes is None. `paste` inverse-maps a premultiplied Image placed at
atlas `origin` with positive atlas-units-per-texel `pixel_size`, using the shared
nearest/bilinear/bicubic/Lanczos samplers. It returns a transparent overlay, not a
background composite. `plane_stroke` samples an atlas polyline at uniform spacing
with continuous phase across edges, up to one million dabs, explicitly marking
gaps rather than joining disconnected planes. Seam continuity is positional,
not derivative continuity. Severe minification still requires input mip choice.

`brush::api::vanishing_point_stroke` connects clone/heal to the existing Stroke
engine without changing its serialized CloneSource. It snapshots the plane-mapped
source with alpha-correct premultiplied bilinear resampling and intersects valid
plane coverage with selection. The ordinary canvas clone offset must be zero;
the separate atlas offset supplies alignment. Returns a normal Stroke supporting
pressure/dynamics, clone/heal, tile output and history integration. Preparation
materializes a source and mask in O(canvas pixels), capped at 16 MP. Input stroke
points and brush footprint sizes are still canvas pixels; the atlas stroke helper
provides plane-space dab centers, not perspective-deformed tip footprints. The
payload is exposed here, not registered as a new engine-api DocOp or UI tool.

Verification: required transform/lens/brush release tests, strict Clippy and
workspace fmt gate recorded in `../../tools/orchestrate/wp/M5-27/gate.log`.
The inaccurate-focal synthetic fisheye grid improves from 1.984666 to 0.064760 px
maximum axis deviation. Real Metal displacement parity covers all four kernels
at levels 0 and 1, with error below 1e-4 (no device-skip or CPU fallback).
Tests also cover checker paste pixels/corners, projective seam clone continuity,
tear-off angles, atlas stroke spacing, serde, malformed inputs and determinism.

## Puppet warp

`from_alpha` builds a deterministic conservative occupancy mesh from nonzero u8
alpha. Sparse/Normal/Dense use 8/4/2-pixel grid cells, each split into two
triangles. Expansion is square dilation clipped to the input bounds (0..64 px).
This approximates an alpha silhouette; it is not constrained-Delaunay contour
triangulation, and holes narrower than a cell may disappear.

Pins constrain vertex positions and optionally rest-relative rotation in
radians. Uniform positive-weight ARAP minimizes edge distortion with alternating
local polar rotations and a global constrained graph Laplacian solve:
`sum_(i,j) ||(x_i-x_j) - R_i(p_i-p_j)||^2`. Global right-hand sides use the average
of the two endpoint rotations; matrix-free conjugate gradients run up to 1024
steps and return an error on nonconvergence. Default 20 local/global iterations,
valid range 1..100. Rigid mode couples rotations per connected component; hard
incompatible pin positions can still cause stretching. Unpinned islands stay
at rest. Sorted topology and pin handling make execution deterministic.

Limits: 16,777,216 alpha pixels, 16,384 vertices, 32,768 triangles. Coordinates
are finite and bounded by 1e9. Inverse lookup is linear in triangle count,
using barycentric coordinates. Overlapping deformed triangles use the first;
collapsed triangles are skipped. No self-collision or depth-order solver.

## Content-aware scale

Forward-energy dynamic programming uses RGBA gradient magnitude plus a strong
soft protect penalty. For vertical seams the transition adds `|I(right)-I(left)|`
and, on diagonal transitions, the corresponding new upper/side adjacency cost.
Distances include premultiplied RGB and alpha, with f64 costs for HDR safety.
Tie breaks are deterministic. Width is processed first; transposition reuses
that implementation for height. Insertion discovers distinct seams on a
shrinking copy before inserting interpolated pixels, in batches for >2x growth.
Masks travel with removed/inserted pixels.

Amount [0,1] controls the rounded fraction of the dimension change performed by
seam carving; bilinear resize completes the requested dimensions. Zero is an
ordinary bilinear resize, one is all seam carving. Protect values are [0,1] on
the supplied input grid (including a supplied mip grid), not absolute guarantees.
`apply_with_skin_protection` accepts a caller hook once per source pixel and
combines its score with the mask by max. There is no bundled skin detector.
Standalone `apply` targets pixel dimensions at that level; TransformOp targets
level-zero dimensions and ceil-divides them at lower levels. No fast incremental
energy update; large seam counts can be expensive.

## Non-destructive compositor and PSD

`SmartFilter::transform` stores the versioned operation in reserved `transform`
params. `DocOp::AddTransform` inserts at a chosen stack index; `SetTransform`
replaces geometry while retaining filter blend/enabled options. Position/all
locks are checked. Wrap ordinary pixel content in a SmartObject before adding
these stages. Native load validates reserved transform stages, even disabled.
History, cache invalidation, shared filter masks and filter blending reuse the
existing smart-filter system. A custom evaluator cannot override transform.
Whole-stack replacement also checks position locks when transform parameters,
enabled/blend options or stack indices change; affine SmartObject placement
checks position/all locks as well. Color-only filter edits remain permitted
under a position lock if they leave existing transform stages at their indices.

The compositor converts straight tiles to premultiplied planes and back. It
applies transforms at the child native resolution before existing smart-object
mip/resampling. Child canvas dimensions remain fixed; geometric output clips to
that canvas. Content-aware resize is padded/clipped at the origin into the same
canvas so blending/masks remain compatible. Original source pixels are retained.

PSD exports SmartObject's existing affine placement through standard descriptor
SoLd/PlLd records and an embedded liFD PSD source. Imports reconstruct an affine
only when a supported embedded PSD source and safe descriptor are available,
using its merged pixels rather than recursively importing its editable tree.
Unknown descriptor fields are retained and duplicate linked-source IDs avoided.
Unsupported external/warped sources remain opaque rendered proxies. Enabled
TransformOp/filter stages, including warp meshes, are native-only: PSD export
returns an explicit error requesting rasterization, rather than silently losing
an edit. There is no Photoshop application interoperability certification.

## GPU and measurements

See `../compositor/src/resident/TRANSFORM.md` for the resident displacement-texture
API and precise Metal kernels. The explicit stage consumes resident premultiplied
buffers or a rendered level, uploads only the geometry map, and never reads pixels
back during rendering. GPU geometry preparation currently uses the CPU inverse
mapper. Plans are reusable while geometry is unchanged. Automatic document
SmartFilter routing still selects CPU: the resident stage must be invoked
explicitly by its caller. No engine-api or develop GPU files are changed.

Tests on Apple M4 Metal observed exact nearest/bilinear/bicubic agreement and
maximum Lanczos error 4.7683716e-7, below 1e-4. Bit-identical transcendental
Lanczos is not claimed. GPU computation is compiled via gpu-core IEEE precise
MSL passthrough; unsupported devices fail explicitly rather than use fast math.

Reproduce (keep CARGO_TARGET_DIR outside this repository):

```
cargo test -p transform -p compositor --release
cargo clippy -p transform -p compositor --all-targets -- -D warnings
cargo fmt --check
cargo run -p transform --release --example affine_bench
cargo test -p compositor --release --test transform_gpu benchmark_36mp -- --ignored --nocapture
```

Performance is hardware/load dependent; the example prints measured times and
is not a CI timing assertion. Initial implementation runs measured CPU bicubic
280–312 ms at 36 MP. Parent reruns under concurrent Rust builds and a sibling
GPU test measured 568–923 ms. A final parent rerun measured 646.437, 600.760 and
553.902 ms, so the <400 ms requirement was not consistently verified.
Parent GPU rerun measured bicubic median 18.568 ms (<30 ms), with
227.733 ms including geometry-map preparation/upload/dispatch/wait. Lanczos
median was 32.901 ms. The <30 ms result is cached bicubic GPU work only, not
interactive geometry-change latency. No 36 MP warp-preparation timing claim.

Retry verification reproduced the CPU target: 265.370, 259.059 and 262.638 ms
including input validation, allocation and rendering. An earlier retry batch
was 367.351, 331.614 and 590.980 ms, so load sensitivity remains. Final bicubic
GPU median was 15.969 ms cached compute and 104.551 ms including geometry
preparation/upload/dispatch/wait. Lanczos-3 median was 32.812 ms. Raw command
output is in `../../tools/orchestrate/wp/M5-21/benchmarks.log`; the full required
gate passed with 186 tests passed, 9 ignored, clippy and fmt clean (gate.log).
