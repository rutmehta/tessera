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
and ContentAwareScale. Geometry uses level-zero pixel-edge coordinates; the
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
