# M2-47 integration handoff

## Implemented host API

`Engine.photo_merge(image_ids, MergeOptions, PhotoJobListener)` and `Engine.enhance(image_ids, EnhanceOptions, PhotoJobListener)` return `PhotoJob`. `status()` is nonblocking; `wait()` is blocking and must not be called from a callback or the main actor. `cancel()` is cooperative. Native decoding, merge kernels, model download and an ONNX invocation already in flight are not interrupted; cancellation is checked before publication. A publication already committed is retained and reported. Callbacks run on the worker, outside catalog locks. Errors contain a stage and message. Completed outputs have paths and indexed IDs.

`Engine.merge_preview` runs synchronously off the main actor, decodes sources, downsamples the merge inputs to at most 512px, and returns a JPEG at most 512px per edge plus warnings. Failed overlap/geometry produces a warning with no JPEG. Preview does not create files. Its rendition is currently a camera-channel/WB/exposure approximation, explicitly warned, not the color-managed develop renderer.

Merge supports HDR, perspective/cylindrical/spherical panorama, and explicit-group HDR panorama through the existing merge core. `bracket_sizes` partitions the ordered input for HDR panorama. Never infer bracket groups from filenames. `exposure_values` can explicitly supply positive sensor exposures for native float DNGs lacking EXIF exposure tags. Ordinary RAW HDR uses shutter/ISO/aperture. Curved projections require calibrated `focal_pixels`.

Unique no-clobber filenames use `-HDR`, `-Pano`, `-HDR-Pano`, `-Enhanced-NR`, `-Enhanced-SR`, or `-Enhanced-NR-SR`, then `-2`, etc. Publication uses an fsynced same-directory temporary file and no-clobber persistence. Float DNG pixels/calibration round-trip through the bounded native DNG reader. Recipes are embedded and written as indexed editable sidecars. Catalog change notifications use the existing M2-28 feed.

`Engine.photo_stack(image_id)` returns persistent ordered members, derived first. The new FFI-owned `photo_stack_member` table unions intersecting existing groups rather than dropping earlier enhancements. It is catalog-local, not a portable stack-sidecar interchange format. New records appear in `list_images`; the existing general RAW thumbnail/develop reader is not extended in this package to consume native LinearRaw pixels.

Enhancement uses the pinned DRUNet/Real-ESRGAN x2 models with a reversible calibrated camera-to-sRGB adapter. NR operates in linear sRGB, SR in encoded sRGB, then the output is converted back to camera space and stored with original calibration. Source recipe edits are retained. `allow_model_download` defaults false. Cached loads are strictly cache-only, including if a file disappears between lookup and load. Download progress is truthful stage-level start/ready, not byte-level percentage. Zero NR bypasses models and is bit-exact even for HDR samples.

## Acceptance gaps / explicit limitations

This is NOT full completion of the requested WP:

- Nonzero Boundary Warp returns an explicit unsupported error. The existing merge crate does not have a boundary mesh/TPS warp. It is not silently mapped to crop.
- Raw Details returns an explicit unsupported error. No learned demosaic model or supported runtime contract exists in the provided enhancement crate. Ordinary demosaic is not relabeled Raw Details.
- Fill Edges retains the merge core's nearest-covered extension, with a preview warning, rather than content-aware inpainting.
- Auto projection currently selects Perspective and reports that policy in preview warnings.
- Source orientation other than 1 is explicitly rejected, rather than writing wrongly tagged pixels.
- Nonzero enhancement rejects HDR/negative/out-of-sRGB-gamut model input, without silent clipping; this model adapter is not a scene-linear HDR denoiser.
- Existing general grid thumbnails/develop need a LinearRaw-capable reader outside the allowed files; native DNG catalog indexing and merge preview work independently.
- Indexing/sidecar/stack failure after file publication leaves the completed DNG in place and returns an error (including saved path for scan failure). Already indexed outputs remain in the job status. Disk and SQLite are not one transaction.
- Model-dependent tests are conditional on existing weights. No production weights were downloaded by this task.

## Verification

Focused integration test command: `cargo test -p tessera-ffi --test merge --test enhance --release` passed. Coverage includes float HDR values, read-only preview, new catalog rows/change feed, stack persistence and repeated enhancement, unique names, cancellation before publication, validation/missing exposure/overlap warnings, all three panorama projections, grouped HDR panorama, zero-NR bit preservation, offline model errors, and orientation rejection.

Initial missing-API compilation failure is in `red-check.log`; the orientation regression was observed failing before its fix in `enhance-red.log`. Model adapter and cache tests were also developed with missing-API failures first. The final full-gate result will be recorded separately after real execution.

All builds use `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-47`. No commits or pushes were made.
