# M2-16: scene-linear photo merge

Synchronous reference implementation. `engine-api` is unchanged. All samples are unbalanced, black-subtracted linear camera RGB. No white balance, camera-to-working-space conversion, or display tone is baked into the merged pixels.

## API

- `from_cfa(&CfaImage, &RawMetadata)` uses pipeline-cpu's halo-aware MHC Bayer / X-Trans fallback demosaic and active sensor crop. It preserves `cam_xyz[0..3]` as XYZ-to-camera `ColorMatrix1`; it does **not** confuse it with the white-balanced camera-to-XYZ matrix. As-shot neutral is the reciprocal WB multipliers, normalized to green. Sensor orientation is retained; input raws must have compatible orientation.
- `hdr::Exposure::from_metadata` uses shutter × ISO / aperture². Missing, nonpositive, nonfinite, or extreme exposure ratios return errors rather than silently assuming equal exposures.
- `hdr::hdr(&[BracketFrame], &HdrOptions)` returns an image, editable native Recipe, reference-coordinate deghost mask, refined exposure ratios, and reference-to-source alignment maps. Set `reference` explicitly to a suitably exposed frame (default index 0). Input clip point is 1. Output units are the reference exposure's radiance scale.
- `pano::panorama(&[LinearImage], &PanoramaOptions)` returns an image, Recipe, measured coverage mask, projected origin, and input-to-first-view homographies. Supply sequential overlapping views, equal exposure, compatible camera calibration/WB, and `focal_pixels` for curved projections. Input ordering is not inferred.
- `hdr_panorama(&[Vec<BracketFrame>], &HdrOptions, &PanoramaOptions)` HDR-merges each group, normalizes differing reference exposures to the first group's scale, then stitches. Deghost masks remain in each bracket's reference coordinates.
- `write_dng(&mut writer, &image, &recipe)` writes a float32 LinearRaw DNG with the recipe as XMP. Low-level `dng::write` accepts caller-provided XMP.
- Re-read with `raw_decode::linear_dng::read(&mut Read + Seek)`. This is intentionally separate from `RawSource::decode_cfa`: a mosaic-free RGB image must not masquerade as a one-plane CFA.

## Algorithms

HDR uses downsampled log-luminance phase correlation with a ±3° rotation search, robust subpixel global refinement, and 32-pixel tile residual search (±1 pixel). Tile-center displacements interpolate continuously. Global translation is bounded to 25% of input dimensions; insufficient overlap is rejected. Small/textureless frames use identity alignment. Histogram refinement finds the median overlapping log exposure ratio within ±0.5 EV of EXIF (1/512 EV bins). The merge uses per-channel triangular well-exposedness weights and excludes sensor clipping. Missing warped samples are skipped. Fully clipped regions fall back to the shortest available exposure, which provides a lower bound rather than invented highlight detail.

Deghost None/Low/Medium/High uses reference consistency thresholds of disabled/35%/18%/8% plus a fixed read-noise allowance. Clipped and near-black samples are radiance intervals: clipping alone is not motion, but incompatible bounds can prove motion. Marked pixels use the reference exclusively, preserving its noise/clipping tradeoff. The returned bool mask is suitable for an overlay; it is not a learned semantic motion mask.

Panorama uses deterministic FAST-9, 256-bit BRIEF, mutual Hamming/ratio matching, normalized homography RANSAC, and robust direct photometric refinement. Perspective, cylindrical, and spherical projections are actual distinct ray mappings using centered principal point and square pixels. Laplacian image pyramids blend with Gaussian feather-mask pyramids. Invalid/disconnected/degenerate geometry and projective horizons are rejected.

Boundary-warp substitute: `auto_crop` finds the largest entirely source-covered rectangle. If crop is disabled, `fill_edges` performs nearest-covered flood fill, an explicitly temporary inpainting placeholder, **not content-aware fill or a mesh warp**. Without either option, uncovered pixels are zero. Coverage continues to distinguish source pixels from filled pixels. Do not export uncovered borders unless that is intended.

## DNG and recipe

DNG 1.4, classic little-endian TIFF, one uncompressed chunky float32 RGB strip, PhotometricInterpretation=LinearRaw, signed rational ColorMatrix1 under D65, AsShotNeutral, and XMP. Values, including negative demosaic overshoot and HDR samples above 1, are written unchanged. Matrix/neutral rationals have 1e-6 precision. The bounded reader also supports big-endian variants of this layout, not arbitrary third-party tiled/compressed/multi-IFD DNGs. No embedded preview is generated.

Native Recipe includes auto-tone settings based on log-average luminance (middle gray 0.18, exposure bounded ±10 EV, highlights -35, shadows +15). They are recorded through recipe history, not baked into pixels. Full JSON, including unknown fields/history, is embedded as `ts:Recipe`; standard CRS tone companions provide a best-effort external rendition. The caller assigns catalogue image ID and creation timestamp.

## Scope and limits

This is not a production gigapixel/out-of-core stitcher. Linear image/DNG limit: 64 Mi pixels. Panorama working output: 8 Mi pixels and 65,536 pixels per dimension. HDR: 2–64 exposures. Panorama: 1–128 views. DNG: 1 MiB XMP, bounded tag/IFD payloads. Registration is suited to small handheld motion and planar/distant scenes. Large rotations, strong parallax, lens correction, automatic frame ordering, bundle adjustment, graph-cut seams, panorama exposure compensation, and 360° wrap seams are not implemented. BRIEF is not rotation-invariant. Deghost reference choice matters for clipped/moving subjects. CPU performance and peak memory on large real-world brackets remain future optimization work.

FAST/BRIEF, RANSAC, and blending are implemented here under the crate's Apache-2.0 license. The new FFT dependency `rustfft` and new transitives `primal-check`, `strength_reduce`, `transpose` declare MIT OR Apache-2.0 in their package manifests; no GPL feature library was added.

## Engine contract follow-up (reported, not modified)

The engine currently exposes DNG as an export format, but has no photo-merge request/result contract. Integration needs:

1. Ordered source IDs and explicit bracket groups, reference frame, align toggle, deghost strength, histogram-refinement toggle.
2. Projection enum, focal length in pixels, auto-crop/fill policy, pyramid levels, and an eventual genuine boundary-warp amount (do not label this placeholder as a mesh warp).
3. Merge output path/image ID and stack membership/create-stack policy.
4. Linear camera-RGB decode source with dimensions, calibration illuminant, XYZ-to-camera matrix, as-shot neutral, and scene-referred HDR range. It must bypass CFA demosaic while retaining downstream camera-color/WB processing.
5. Coverage and deghost overlay coordinates, transforms, exposure diagnostics, cancellation/progress/job scheduling.
6. Import/export of embedded native `ts:Recipe` and catalogue provenance for all sources. The bounded reader returns XMP verbatim; existing generic sidecar import is not changed here.

## Verification

Run with `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-16`:

```
cargo test -p merge -p raw-decode --release && cargo clippy -p merge --all-targets -- -D warnings && cargo fmt --check
```

Synthetic tests cover noisy ±2 EV recovery (<2% worst relative error), moving objects including clipped motion, phase translation/rotation and tile residuals, estimated projective three-view panorama overlap PSNR >30 dB, HDR panorama with differing reference exposures, actual projection/crop/fill behavior, outlier RANSAC, float DNG re-read dimensions/calibration/samples/XMP, malformed/truncated inputs, and CFA-to-camera-RGB integration. See `tools/orchestrate/wp/M2-16/verification.log` for the executed gate.
