# M5-26 GPU implementation — verified

All pointwise M5-26 variants now execute in resident interpreter and specialized shaders: Vibrance, ColorBalance, BlackWhite, PhotoFilter, GradientMap (all three methods, reverse/dither), SelectiveColor, Desaturate, Equalize, Auto, MatchColor, ReplaceColor, ColorLookup, BrightnessContrast. No CPU pixel evaluation/readback substitute was introduced.

## Verification

`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-26 cargo test -q -p compositor --test m5_26_gpu`: **4 passed**, 0 failed. Exact `to_bits()` assertions at L0/L2, F32/U8/U16, interpreter and actual waited-for specialized pipeline. Includes transparent/partial alpha, positive/negative controls, identity, degenerate Equalize/Auto, realistic Lab MatchColor statistics, gradient empty/single/unsorted/duplicate stops and dither, 2³/3³ LUTs.

`cargo test -q -p compositor --lib` with the same target dir: **47 passed, 1 ignored**.

Attempted full `cargo test -q -p compositor`: blocked by peer's newly added `tests/m5_26_icc.rs` calling not-yet-defined `Adjustment::color_lookup_from_icc` (6 compile errors). GPU target itself compiles/runs. PSD exhaustiveness blocker is resolved by peer changes.

## Explicit native formula refinements for bit-exactness

- Modern BrightnessContrast is now defined as the linearly interpolated 4096-knot curve sampled from the existing endpoint-preserving formula over [0,1]. Same CPU-generated knots uploaded to GPU; legacy affine mode unchanged. Initial real GPU test caught a 1-ULP native pow discrepancy.
- Auto retains exact black/white normalization (including zero-span step) and identity gamma. Nonidentity gamma is the interpolated 4096-knot normalized gamma curve; knots shared with GPU. This preserves clipping thresholds rather than sampling the entire black/white mapping.
- `adjust/color.rs` sRGB powers and signed cube root use shared CPU-generated 4097-knot mantissa tables on [1,2] plus exponent tables (-149..127). Interpolate `(a + (b-a)*f)*exponent_factor`, avoiding backend pow/cbrt without clamping HDR to a fixed LUT domain. Root uses exponent 1/3 as f32. CPU and GPU use identical operation ordering. These are native approximations, not claims of bit-exact equivalence to libm or Photoshop.
- GPU non-power-of-two division uses existing correctly-rounded pdiv; polynomial/matrix/trilinear accumulation order matches CPU. All prior CPU unit tests remain green.

ShadowsHighlights remains explicit `EngineError::Unsupported` in resident program; neighborhood execution/fallback remains parent's responsibility. No silent identity port.

Owned files changed: resident/program.rs, resident/adjustments.wgsl; previous agent's minimal resident/mod.rs, doc.wgsl, specialize.rs inclusion/dispatch wiring retained; tests/m5_26_gpu.rs expanded. Authorized minimal CPU changes: adjust.rs compiled curves, adjust/color.rs shared nonlinear tables. No commit/global formatting.
