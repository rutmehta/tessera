# B5-20b handoff — Adaptive Wide Angle on real photo sizes (coarse solve lattice)

Branch `wp/B5-20b`, on top of `wp/B5-20` `23c8faa2` (unchanged). A merges B5-20 and B5-20b together.
No document-format change (FORMAT_VERSION 1), no recipe field added, no new dependency, Cargo.lock unchanged.

## Commits
- `807f7d63` RED (tests only): real-size filters tests, memory bound, coarse-vs-dense equivalence (does not compile
  without `adaptive_lattice`), FFI real-size/absolute-limit tests, updated B5-20 cap tests.
- `986f085c` RED (Swift): re-edit keeps the stored focal lengths.
- `732a96e7` filters: `adaptive_lattice` module + adapter wiring; transform cap constants.
- `69d640f7` tessera-ffi: begin/commit/preview for real-size layers, mid-render cancel, `adaptive_wide_angle_max_pixels`.
- `432a1b2a` mac: real-size self-test step, stored focal kept, `maxPixels`, runner case, bindings.
- `6b7ad152` HANDOFF.
- Review round (A: APPROVE-WITH-CHANGES at 6b7ad152), commits on top:
  - `84d5c545` RED: full-width horizon at 6000 × 4000 / 5212 × 3468 fails "constraint residual exceeds
    tolerance"; 3999 × 2999 coarse-vs-dense equivalence; smart-object branch of the > 100 MP refusal restored.
  - `d9d333d8` traced-curve segment count from the sagitta bound (below).
  - `029fd2d2` ignored counting-allocator measurement at 100 MP (`tests/document_adaptive_memory.rs`).
  - merge of `origin/main` `00f8152f` (B5-18b and later). `SmartFilterRow.init` takes main's side (the engine
    names `camera_raw` "Camera Raw Filter") plus `AdaptiveWideAngleFilter.displayName(r.name)`; test
    `testSmartFilterRowsNameCameraRawAndAdaptiveWideAngle`.
- Gates after the merge: `cargo test -p tessera-ffi -p filters -p transform --no-fail-fast` 739 passed, 0 failed,
  36 ignored; clippy `--all-targets -D warnings` clean on the three crates; `cargo fmt --check` clean;
  `build-ffi.sh` OK (bindings unchanged by the merge); `swift-gate.sh` **SWIFT GATE OK** (854 XCTest, 3 skipped,
  0 failures, +5 swift-testing).

## Approach (`crates/filters/src/adaptive_lattice.rs`, one module)
The 16,777,216-vertex cap stays, as a cap on the **solve lattice**. Layers whose `(w+1)(h+1)` fits keep the dense
B5-20 path byte-for-byte. Above it, `Lattice::solve` solves the same stored recipe scaled by `f` onto a lattice of
at most `COARSE_VERTICES` = 2,097,152 vertices, and `render` samples the full-resolution layer through
`transform::sample` (bicubic, premultiplied — the displacement renderer's kernel). The render is tile-parallel on
`std::thread::scope`, deterministic, and checks a `CancellationToken` for every output tile.
- Scaling: focal lengths, centre, traced samples, crop and `line_tolerance` scale by `f`. The objective is
  quadratic in output pixels, so the scaled minimiser is `f` times the full one. `f = m/gcd(axes)` makes every axis
  an integer when that costs at most 25 % of the resolution (6000 × 4000 → 1773 × 1182). Otherwise each axis is
  rounded up and the crop absorbs the half-size shift (5212 × 3468), which is exact for Manual cameras.
- The solver densifies source segments by `ceil(len/4)` up to 64. Short segments get the full-resolution count by
  inserting those samples before scaling, so both solves weigh the same points. The FFI preview proxy uses the same
  `scale_recipe`: the B5-20 proxy scaling failed the ÷8 tolerance on a 24 MP proxy.
- Limits: the only refusal is > 100 MP (`transform::MAX_IMAGE_PIXELS`). The message is "supports layers up to 100
  megapixels; this layer is W × H (N megapixels)".
- One source per constant: `transform::displacement::MAX_VERTICES` (used by displacement/adaptive validation and
  `adaptive_lattice::DENSE_VERTICES`) and `transform::MAX_IMAGE_PIXELS` (Image/canvas and `MAX_PIXELS`). FFI and
  Swift read the limit through `adaptive_wide_angle_max_pixels()` / `AdaptiveWideAngleFilter.maxPixels`.
  transform validation is otherwise untouched: the coarse recipe is a normal, valid recipe.
- Test hook: `adaptive_lattice::with_coarse_budget(n, || …)` (thread-local) forces the coarse path.

## Tolerances (stated in `tests/adaptive_lattice_equivalence.rs`, 4000 × 3000, factor 0.417, 2,089,588 vertices)
| quantity | bound | measured |
|---|---|---|
| inverse map, max \|Δ\| (source px) | ≤ 0.02 | 0.0015 |
| pixels, mean \|Δ\| (all channels, whole layer) | ≤ 1e-3 | 2.4e-7 |
| pixels, max \|Δ\| away from the coverage edge | ≤ 0.01 | 1.3e-4 |
| coverage boundary differs | ≤ 0.1 % of pixels | 0 / 245,388 samples |

Straightness (stripe centroid spread along the traced range, uncorrected tilt ≈ 16 px): 6000 × 4000
0.004 / 0.008 px, 5212 × 3468 0.003 / 0.007 px (bound 0.5 px). Memory (own test binary, counting allocator):
24 MP peak 819 MB. The output raster is 384 MB of that. The dense estimate is 1,728 MB (bound: ≤ 60 %).

## Traced-curve segments (review item 1)
`adaptive_wide_angle_curve` joins its samples with straight source segments. A chord of length `L` on a curve of
curvature `k` misses it by the sagitta `k L² / 8`, and curvature in pixels falls as 1 / image size for a given field
of view, so B5-20's fixed cap of 64 segments grew the error linearly with the photo (≈ 0.46 px full-width at 24 MP,
≈ 1.9 px at 100 MP, against 0.25 px). Now: start at `clamp(ceil(chord / 24), 4, 256)`, then double (up to 1024)
while any segment's projected midpoint is more than `CURVE_SAGITTA_PX` = 0.05 px (a fifth of the tolerance) from the
straight projected edge. Full-width horizon (3 %–97 % of the width at 0.3 h, equidistant f = 0.4 w): 470 segments at
6000 × 4000, 410 at 5212 × 3468, 512 at 12240 × 8160; all three solve. The solver still densifies every 4 source px,
so the densified-sample count per line is unchanged; the input-sample limit (16,384 per recipe) now allows ≈ 34
full-width 24 MP lines (the 65,536 densified limit already allowed ≈ 46).

## Coprime axes (review item 2)
`coarse_matches_dense_on_coprime_axes_within_the_same_tolerance`, 3999 × 2999 (gcd 1, rounding path), the same
table as 4000 × 3000: factor 0.4175, 2,093,763 vertices; map max Δ 0.0048 px (≤ 0.02); pixel mean |Δ| 9.7e-7
(≤ 1e-3); interior max |Δ| 3.0e-4 (≤ 0.01); coverage differs on 0 / 245,388 samples; straightness 0.004 / 0.006 px.

## Memory at 100 MP (review item 4)
Measured with a counting allocator (`document_adaptive_memory`, release, `--ignored --test-threads=1`), 12240 × 8160
layer with materialized tiles, begin + apply through the session:
| case | document before | AWA working set above it | absolute peak | reviewer's estimate |
|---|---|---|---|---|
| U8 pixel layer | 0.80 GB | 3.26 GB | 4.06 GB | ≈ 4.1 GB |
| F32 pixel layer | 2.00 GB | 3.26 GB | 5.25 GB | up to ≈ 6.5 GB |
| U8 smart object, begin | 0.80 GB | 3.24 GB | 4.04 GB | ≈ 3.2 GB (begin) |
The working set is the premultiplied F32 planes (1.6 GB) plus the F32 output raster (1.6 GB) either way.

**Smart-object apply above ≈ 33.5 MP is refused** ("resource exhausted: CPU smart-filter pass retained results exceed
configured limit", history unchanged). `compositor::render::smart_filters::FilterPassLimits::retained_bytes` is 1 GiB
(same on main); passes at 6000 × 5500, fails at 6000 × 6000, 8000 × 8000 and 12240 × 8160. This is a compositor
pass limit shared by the smart-filter stack, not AWA code, and is not changed here; the "> 100 MP is the only
refusal" statement above holds for pixel layers only.

**Mesh-resolution limit at 100 MP.** The fit error of the 17 × 17 control mesh scales with the correction it must
absorb, which grows with the image while the tolerance stays 0.25 px. The `real_size_recipe` vertical (x 0.25 w →
0.26 w over 0.2 h → 0.8 h, ≈ 1° of residual tilt) solves at 6000 × 4000, 8000 × 6000 and 10000 × 7000 but fails
"constraint residual exceeds tolerance" at 12240 × 8160, with the traced curve already inside the sagitta bound and
at a 4× larger coarse budget too. Users can raise the mesh size or line tolerance; changing the default objective
would change stored B5-20 renders, so it is left for a follow-up.

## Timings (release, `document_adaptive_ui` ignored timing tests; blank U8 layer)
- 6000 × 4000: begin 0.05 s, proxy preview 750 × 500 40–47 ms, full apply 0.36 s.
- 4000 × 3000 (dense path, unchanged): preview 29–31 ms, apply 0.77–0.81 s. The dense solve is serial and
  dominates; the coarse solve is 8× smaller.
- Debug (filters tests, textured layer): 6000 × 4000 16.5 s, 5212 × 3468 14.7 s.

## Review items (A's guidance)
1–6 done: solve-lattice cap; FFI begin changed with the adapter; no recipe fields; fine path kept under the cap;
honest limit message; one constant source.
- (a) done: `CompositorFilters::evaluate_with_cancel` passes the token into the AWA render (pixel-layer commits use
  it through the job's `CancellationToken`). Tests: `a_cancel_stops_the_coarse_render` and
  `cancel_during_a_real_size_commit_stops_the_render_without_history`. The coarse *solve* itself (transform,
  serial) is not interruptible: a cancel lands after it. The smart-object validation render keeps its AtomicBool
  boundary checks.
- (c) done: the draft keeps the stored `focal_px` / `output_focal_px` verbatim until the focal length is edited.
- (d) done for Rust (807f7d63) and Swift (986f085c).
- (b) not done: the re-trace per focal tick stays synchronous on main. It is pure maths (≈ chord / 24 damped-Newton
  inversions per line, doubled when the sagitta check refines, at most 1024), cheap next to the preview, which is already off main and latest-wins. Left as a follow-up
  so this package keeps the workspace model unchanged.

## Gates (at 432a1b2a)
- `cargo test -p tessera-ffi -p filters -p transform --no-fail-fast`: 725 passed, 0 failed, 32 ignored. Neither
  known flake recurred on this run.
- `cargo clippy -p tessera-ffi -p filters -p transform --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- `apps/mac/build-ffi.sh`: OK (bindings regenerated and committed).
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**: 847 XCTest, 3 skipped, 0 failures (+5 swift-testing).

Known flakes carried from B5-20 (not touched by B5-20/B5-20b):
- `document_liquify_ui::brush_latency_on_a_20_megapixel_layer` — load-sensitive p95. Fails under full-suite load,
  passes alone.
- `tessera-ffi --lib smart_preview_thumbnail::tests::hdr_saved_offline_recipe…` — shared admission state between
  parallel tests ("close the active Smart Preview editor"). Passes alone and with `--test-threads=1`.

## Self-test
`tools/orchestrate/wp/B5-selftest-window/run-background-selftest.sh adaptive-wide-angle` (new case; copies
sample.dng into the library). The AWA self-test now opens sample.dng (5212 × 3468), constrains, previews, applies
one history row through the coarse lattice, undoes, then runs the B5-20 synthetic-grid steps.
Run at 432a1b2a (`make-app.sh` release, background, front app unchanged): **`done, 0 failure(s)`**. sample.dng:
workspace open 1.02 s, proxy preview 745 × 496 in 41 ms, full-resolution apply 0.71 s.

## Remaining
- Visual/on-screen pass of the sheet on a real fisheye photo (none in fixtures; sample.dng is rectilinear).
- The coarse solve is serial (transform); a 100 MP solve at the same budget costs about the same as 24 MP.
