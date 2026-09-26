# M5-26 spatial GradientMap dither

## Result

Replaced RGB-content hashing with spatial hashing. `Compiled::apply_at(c, x, y)` takes absolute integer coordinates at the requested mip level; `apply(c)` remains an origin wrapper for tests/legacy coordinate-free callers. Only GradientMap uses the new coordinates. CPU `render/exec.rs` has exactly one call-site replacement, passing its existing tile origin plus pixel offsets.

Hash seed is `x ^ y.wrapping_mul(0x9e3779b9)`, followed by the existing wrapping two-multiply avalanche. WGSL uses matching u32 math. Noise amplitude, luminance calculation, gradient interpolation, and other operators' arithmetic are unchanged. `doc.wgsl` forwards its existing absolute mip-level `x,y` through `adjustment` and `extended_adjustment`. Inspected `specialize.rs`: it reuses these source fragments, so no change there was required.

## Verification

Commands used `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-26`.

- Baseline real-GPU `cargo test -p compositor --test m5_26_gpu brightness_contrast_exact -- --nocapture`: 1 passed.
- RED: new `gradient_spatial_dither_multitile_exact` failed on old code with `flat field must spatially dither`.
- GREEN: targeted new real-GPU test passed after spatial implementation.
- `cargo test -q -p compositor --lib adjust::`: 26 passed.
- `cargo test -q -p compositor --test m5_26_gpu`: 7 passed (all six existing tests plus new spatial test), 7.82 seconds.

New integration fixture is 1040x16 so both L0 and L2 cross a 256-pixel tile boundary. It tests F32/U8/U16, opaque/transparent/partial-alpha rows, interpreter and waited-for specialization, strict f32 bit equality, spatial flat-field variation, and no tile-boundary reset. New unit tests cover horizontal/vertical variation, origin compatibility, repeatability under reversed traversal, extreme wrapping coordinates, amplitude bounds, and position independence when dither is disabled for all three interpolation methods.

Formatted only owned Rust files using `rustfmt --edition 2024 --config skip_children=true`; exec.rs remains a single call-site edit. No commits. Full workspace gate left to parent. Baseline non-quiet Cargo invocation printed existing LibRaw C++ warnings; final targeted quiet test runs had no warnings/errors.

## Files changed by this subtask

- crates/compositor/src/adjust.rs
- crates/compositor/src/adjust/extended_tests.rs
- crates/compositor/src/render/exec.rs (one call site only)
- crates/compositor/src/resident/doc.wgsl
- crates/compositor/src/resident/adjustments.wgsl
- crates/compositor/tests/m5_26_gpu.rs (new test only)
- tools/orchestrate/wp/M5-26/dither-notes.md
