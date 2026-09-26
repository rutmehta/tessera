# M5-24 implementation handoff

Status: required gate and supplied acceptance fixtures PASS, with API/model limits
below. No commits made. The prior unsupported lens options now have a calibrated
implementation.

## Implemented

- `merge::layers` with seeded alignment, union bounds and per-source TransformOps.
- Auto/Perspective homographies and Collage similarities connect through neighboring registered views; Reposition translation uses existing direct alignment.
- Cylindrical/Spherical use editable approximate WarpMeshes with sampled 0.05 px tolerance and assumed focal length equal to the largest reference dimension.
- Sequential binary graph-cut panorama ownership, real-coverage-aware multiband blending and editable additive RGB corrections.
- Multiscale Gaussian/Laplacian focus measures and editable focus masks.
- `DocOp::{AutoAlignLayers, AutoBlendLayers, Photomerge}`. Retained source pixels/masks, existing transform stage, canvas union, one-node Photomerge undo/redo and atomic errors.
- Host CAF adapter avoiding filters→compositor dependency cycle, tested against actual `filters::caf::fill` with union-hole-only output.
- Explicit `Document::rasterized_layers_for_export()` proxy preserves separate root layers and masks in serialized/read-back PSD bytes.
- Per-input `LensCorrection` calibration implements radial distortion and vignette
  removal. Registration uses rectified temporary inputs; one composed WarpMesh
  renders the original source. Vignette gain planes are editable clipped Multiply
  rasters before geometry. Invalid or missing calibration fails atomically.

## Verification

Ran directly with CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-24:

`cargo test -p merge -p compositor --release && cargo clippy -p merge -p compositor --all-targets -- -D warnings && cargo fmt --check`

Latest run: exit 0. 201 tests passed, 10 ignored, 0 failed. `final-gate.log` contains
raw output (including pre-existing LibRaw C++ build warnings). Release tests,
clippy with -D warnings, and workspace fmt --check all passed. Prior logs are
retained as historical evidence, not the current result.

The retry separated global rigid registration from HDR-specific per-tile residual refinement. Layer transforms previously discarded the local offsets after paying to compute them. HDR keeps the original refinement and early-return behavior; a regression test checks identical global parameters, retained HDR tile offsets, flat-image behavior and invalid exposure rejection.

The ignored 3×24MP benchmark passes: alignment/render 1.012007750 s, blend
3.168050667 s, total 4.180058417 s (`final-benchmark.log`). This is the existing
Reposition-mode benchmark, not a claim for all projections, optional calibration,
or the full compositor DocOp.

`final-quality.log`: three shifted/rotated/scaled crop RMS 0 / 0.0014268 / 0.0036006 px;
seam gradient MAE 0.0050487; overlap MAE 0.0012212. Three-source Gaussian-blur focus
ownership 99.7806%; multiscale/noise fixture 99.1444%.

New tests observed failing before the lens implementation: `lens-red.log`,
`lens-document-red.log`, `distortion-red.log`. Passing counterparts and
`lens-roundtrip.log` exercise actual corrected compositor output, editable gain
layers, native and PSD roundtrips, and history rollback. `lens-registration.log`
measures three differently calibrated, vignetted shifted sources at RMS
0.003693 / 0.034394 / 0.018650 px and verifies deterministic transforms and pixels.

`document-panorama.log`: three-source Photomerge compositing matches standalone merge output on opaque pixels, three nontrivial masks survive real PSD serialization, proxy rasterization leaves source document editable, one undo removes the complete group.

`validation-green.log`: real CAF union-hole fill, rollback on invalid options, locked/duplicate source rejection, native format roundtrip, Photomerge history and single-layer PSD roundtrip.

## API contracts and model limits

- Lens removal requires caller-supplied centered radial calibration in the documented
  axis-normalized convention and linear RGB. No automatic profile discovery,
  scene-based calibration, tangential/decentered or fisheye model is included.
- The prior benchmark failure is resolved for the supplied Reposition fixture. Other alignment modes and full-document merge are not covered by that timing assertion.
- Auto-Align is restricted to independent root pixel layers. Nested/already-transformed sources are not supported. Root groups/clipping/locks that require moving can be rejected.
- Projection focal length is assumed, not calibrated. Auto currently uses projective estimation with rigid fallback rather than automatic projection selection.
- Spatial tone/seam correction is an editable clipped additive raster inside a smart object, not an `Adjustment` enum variant.
- Graph cuts use a coarse grid capped at 160 samples along its longest edge; not globally optimal multilabel optimization. Bounds and registration limitations are in `crates/merge/LAYERS.md`.
- PSD export requires explicit layered rasterization; direct unsupported smart-filter export behavior is unchanged.

No files in compositor render/ or resident/ were edited. No target directory was created in the worktree.
