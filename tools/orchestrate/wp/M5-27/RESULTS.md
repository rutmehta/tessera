# M5-27 verification

Implemented Adaptive Wide Angle sphere reprojection plus constrained least-squares mesh and serializable displacement operation, Vanishing Point homography atlas/tear-off/paste/clone/stroke geometry, and brush clone/heal source adapter. Formulas and limits are in crates/transform/TRANSFORM.md.

## Gate

Ran from this worktree with CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-27:

```
cargo test -p transform -p lens -p brush --release && cargo clippy -p transform -p lens -p brush --all-targets -- -D warnings && cargo fmt --check
```

Exit 0. 112 tests passed, 0 failed, 1 existing ignored benchmark. Clippy and workspace formatting passed. Existing vendored LibRaw C++ build warnings remain in the log, not new Rust lint failures. Full output: gate.log.

Separate release measurement reruns: measurements.log. The inaccurate-focal synthetic grid measures the emitted inverse displacement field at independent between-constraint positions. A real Metal device executes the existing compositor resident transform path, not a CPU fallback. Tests cover nearest, bilinear, bicubic and Lanczos3 at mip levels 0 and 1.

## Interface decisions and limits

- TransformOp is a struct in this repository, so the new variant is Operation::Displacement, consumed by TransformOp::apply and TransformOp::displacement.
- Existing rectilinear lens profiles are used unchanged. Manual equidistant fisheye is explicit; no claim of imported fisheye profile support.
- VanishingPoint is the serde tool payload exported from transform::vanishing. No engine-api DocOp or UI registration, which is outside the allowed paths.
- Brush adapter provides plane-space source mapping with alpha-correct resampling and clone/heal integration. Brush input points and tip footprints remain canvas-space; the atlas stroke helper supplies plane-space centers with continuous phase. Perspective-deformed tip footprints are not provided.
- Adaptive output is capped at 16,777,216 lattice vertices; the brush adapter prepares a mapped source/mask capped at 16 MP. No interactive full-resolution timing claim.
- No commits or pushes. Changes are confined to the allowed repository paths.

RESULT: PASS
