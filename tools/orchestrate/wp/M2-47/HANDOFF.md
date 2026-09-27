# M2-47 integration handoff (round 2)

Round 2 closes the prior LinearRaw ingestion, panorama geometry, and orientation gaps. See CURRENT-STATUS.md for the final gate result. No UI changes, commits, or pushes are part of this run.

## Host API

`Engine.photo_merge(image_ids, MergeOptions, PhotoJobListener)` and `Engine.enhance(image_ids, EnhanceOptions, PhotoJobListener)` return `PhotoJob`. `status()` is nonblocking; `wait()` is blocking and must not run on the main actor or inside callbacks. Cancellation is cooperative between kernels and before publication; CAF also receives the job cancellation flag. Callbacks run on the worker outside catalog locks. Errors carry stage/message; outputs carry saved paths and indexed IDs.

`Engine.merge_preview` is synchronous, off-main-actor, read-only, at most 512px. It returns warnings and no JPEG if overlap/geometry cannot be solved. Its camera-channel/WB/exposure rendition remains an explicitly warned approximation, not the color-managed develop renderer.

HDR panorama uses explicit `bracket_sizes` in input order, never filename inference. `exposure_values` supplies positive sensor exposures for LinearRaw brackets without EXIF exposure tags. Ordinary RAW uses shutter/ISO/aperture.

Unique no-clobber names use `-HDR`, `-Pano`, `-HDR-Pano`, `-Enhanced-NR`, `-Enhanced-SR`, or `-Enhanced-NR-SR`, then `-2`, etc. An fsynced same-directory temporary DNG is persisted without clobbering. Editable sidecars, catalog indexing/change notifications, and persistent ordered stacks follow publication. `photo_stack(image_id)` returns the derived image first and unions intersecting existing stacks. File publication and catalog changes are not a single transaction: a later indexing/sidecar failure preserves an already-written DNG and reports failure.

## Round 2 ingestion

`raw_decode::linear_dng` classifies PhotometricInterpretation 34892 and reads bounded native float32/uint16 DNG interchange. CFA DNG stays on the raw path. `RawImage::open` and `RgbSource::{recognizes,open}` route native LinearRaw to working RGB, so develop, grid, and export all accept outputs without changing export.rs or render/resident files.

`RgbSource::from_linear_dng` inverts ColorMatrix1, derives the scene white from AsShotNeutral, Bradford-adapts to D65, and enters linear Rec.2020 at the RGB/Demosaic boundary. It preserves finite signed/HDR values and consumes orientation exactly once. PreviewStore uses the same calibrated ingestion and upright cache key. The reader supports single-IFD/single-uncompressed-strip DNG 1.4 interchange with D65 calibration, not arbitrary third-party tiled/compressed/SubIFD LinearRaw layouts; unsupported data is an error.

Merge/enhance applies all eight EXIF orientations in camera space before alignment/inference, permuting pixels without interpolation and retaining calibration. The published DNG is upright/orientation 1.

## Round 2 panorama

`PanoramaOptions.boundary_warp` is 0..100. Two separable ruled meshes move the external boundary toward the existing rectangular canvas while deforming interior content. Zero is identity, not crop. Mesh warp precedes optional crop and synthesis. Interior holes remain unsupported until fill; coverage continues to describe source support, not fabricated support. Homographies remain pre-warp and are not a full inverse nonlinear output map.

Auto samples registered boundary rays and selects spherical above 80 degrees vertical FOV, else cylindrical above 100 degrees horizontal FOV, else perspective. Vertical wins if both exceed thresholds. FFI preview reports the resolved projection. Explicit curved modes require `focal_pixels`; Auto without calibration estimates a 60-degree horizontal FOV for the first upright view and warns. Supplying calibrated `focal_pixels` avoids that estimate.

Fill Edges invokes actual `filters::caf::fill` with a float RGBA raster and inverse-coverage mask, preserving source-covered pixels and HDR values. Direct `merge -> filters` would cycle through `filters -> compositor -> merge`, so production uses dependency-inverted `panorama_with_fill` and `hdr_panorama_with_fill` callbacks from FFI (which already depends on filters). Merge's tests depend on filters and exercise the same adapter. The old no-adapter entry points explicitly error if synthesis is needed; nearest-covered extension is no longer advertised or used as Fill Edges.

## Enhance and remaining limitations

Raw Details is explicitly out of scope by the coordinator decision. The compatibility field remains but true is rejected before scheduling, documented in ml-enhance/README.md. Ordinary demosaic is not mislabeled learned Raw Details.

NR/SR uses pinned DRUNet/Real-ESRGAN x2 with a reversible calibrated camera-to-sRGB adapter. Zero NR bypasses models bit-exactly. Nonzero inference rejects HDR/negative/out-of-sRGB-gamut model input rather than clipping. `allow_model_download` defaults false; cache-only loads remain offline if a cached file disappears. Download progress is stage-level start/verified-ready, not byte-level percentages. Production weights are not downloaded in tests; model-dependent tests skip cleanly when absent.

## Verification coverage

- FFI merge -> asynchronous grid preview -> real develop session/non-black render -> exposure edit -> persisted/reopened settings -> JPEG export.
- Native float32 and uint16 ingestion, calibration, signed/HDR preservation, CFA routing, endian/truncation validation, all orientations, preview cache/edit behavior.
- Boundary warp strengths/canvas/interior displacement/identity, angular Auto threshold boundaries, CAF comparison against actual filters::caf, crop/fill precedence, invalid adapter output, HDR panorama fill entry point.
- FFI projection/warp/fill publication, cancellation, stacking/index/change feed, naming, offline weights, Raw Details rejection, and all eight orientation permutations through enhancement.

All Cargo commands use CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-47. Shared integration hotspots for the coordinator: Cargo.lock, generated Swift/C bindings, and previews/src/raw.rs. Image-core changes are confined to source.rs/rgb.rs and tests as requested.
