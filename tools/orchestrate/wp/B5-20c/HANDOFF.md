# B5-20c handoff — plain message for Adaptive Wide Angle on large smart objects

Branch `wp/B5-20c`, rebased onto `origin/main` `6a35e233` (B5-20 + B5-20b merged); first built on `wp/B5-20b`
`769c7b2a`. No compositor change, no format change, no new FFI export (bindings unchanged),
Cargo.lock and board.json untouched.

## Decision (Machine A)
Keep the compositor's CPU smart-filter pass limit (`compositor::render::smart_filters::FilterPassLimits::retained_bytes`,
1 GiB). Replace the cryptic refusal ("resource exhausted: CPU smart-filter pass retained results exceed configured
limit", see `../B5-20b/HANDOFF.md`, "Smart-object apply above ≈ 33.5 MP is refused") with:

> Adaptive Wide Angle on a Smart Object is limited to about 33 MP. Rasterize the layer, or apply to a pixel layer.

## Approach (`crates/tessera-ffi/src/document/adaptive.rs`)
- **Up front, at commit, before any render.** The compositor charges each smart object `canvas.area() * 32` bytes
  (unmasked source + result, RGBA F32; `smart_filters::entry_bytes`) against the pass limit. `smart_object_max_pixels()`
  = `FilterPassLimits::default().retained_bytes / 32` = 33,554,432 px, read from the real constant.
  `commit_adaptive_wide_angle` on a smart object whose child canvas exceeds it returns the plain message without
  rendering; history is unchanged and the workspace stays open.
- **Fallback mapping.** The compositor's refusal reaches the bridge as text only (`EngineError::ResourceExhausted
  { resource: String }`, no dedicated type), so a smart-object commit error containing the compositor's resource
  string maps to the same message. That covers nested smart objects whose entries add up past the limit while the
  outer one alone fits.
- **Begin is not refused.** The workspace opens and previews on the proxy as before, so the sheet (not the status bar)
  shows the message when OK is pressed. The sheet already shows commit errors verbatim; no Swift source change.
- Pixel layers: unaffected (the only limit stays > 100 MP).

## Tests
- RED `d708b001` (was `812b29cd` before the rebase): `document_adaptive_ui::smart_objects_over_the_smart_filter_pass_limit_are_refused_plainly`
  (6000 × 6000 smart object → exact message, history unchanged; 6000 × 5500 smart object applies one node;
  6000 × 6000 pixel layer applies one node; sequential in one test so the documents never coexist, ≈ 3.5 min debug).
  Failed with `left: Err("resource exhausted: CPU smart-filter pass retained results exceed configured limit")`.
  Swift `DocumentAdaptiveWideAngleTests.testSheetShowsTheSmartObjectSizeLimit`: OK on a 6000 × 6000 smart object
  leaves `error` equal to the message, no history.
- Unit: `the_smart_object_limit_comes_from_the_compositor_pass_limit` (33,554,432; 6000 × 5500 and 8192 × 4096
  accepted, 6000 × 6000 refused).

## Accepted limitation: tilt residual at ~100 MP (unchanged, see `../B5-20b/HANDOFF.md`, "Mesh-resolution limit at 100 MP")
The 17 × 17 control mesh cannot absorb a ≈ 1° residual tilt within the 0.25 px line tolerance at 12240 × 8160: the
`real_size_recipe` vertical fails "constraint residual exceeds tolerance" there, while it solves at 6000 × 4000,
8000 × 6000 and 10000 × 7000. Machine A accepted this: the default objective (mesh size, tolerance) is **not**
changed, because that would change stored B5-20 renders. Users can raise the mesh size or line tolerance in the
recipe. Revisit only with a versioned objective.

## Gates
After the rebase onto `6a35e233`:
- `cargo test -p tessera-ffi --no-fail-fast`: 534 passed, 0 failed, 28 ignored.
- `cargo clippy -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --check`: clean.
- `apps/mac/build-ffi.sh`: OK, bindings unchanged.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (855 XCTest, 3 skipped, 0 failures).
