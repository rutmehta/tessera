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
- this HANDOFF.

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
- (b) not done: the re-trace per focal tick stays synchronous on main. It is pure maths (≤ 64 damped-Newton
  inversions per line), cheap next to the preview, which is already off main and latest-wins. Left as a follow-up
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
