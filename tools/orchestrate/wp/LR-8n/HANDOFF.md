# LR-8n HANDOFF: relinked RGB originals use the stored frame

Base: `origin/main` f77aae5d (batch 55, Smart Previews). Branch `wp/LR-8n`.
Source finding: REV-SP-B S1 (relinked RGB originals were read in the rotated
frame while their Smart Preview was read in the stored frame).

## Findings

### 1. The gap (REV-SP-B S1), confirmed
`RawImage::open_with_catalog_orientation` passed the catalog orientation
to `RgbSource::open_with_orientation`, and the decoder rotated the pixels
(`image-core/src/rgb.rs`, `apply_orientation`). The image then reported
orientation 1. The same photo's LinearRaw Smart Preview stays in the stored
frame and reports the catalog orientation. A rotated phone JPEG imported
offline and then relinked therefore read its imported (stored-frame) crop
and masks in the rotated frame:
- orientations 6 and 8: the crop aspect was transposed;
- orientation 3: the crop and masks moved to the content opposite them (180 degrees).

The new parity test's control reproduces the old frame. Aspect is
transposed for 6 and 8. For 3, cropped-content correlation is -0.12; the
stored frame gives 0.9999.

### 2. How Lightroom stores crop and mask coordinates on rotated JPEGs
Lightroom normalizes them in the unrotated, stored image, for every format:
- darktable `src/develop/lightroom.c` (`dt_lightroom_import`) reads
  `tiff:Orientation` and flips/swaps CropLeft/Top/Right/Bottom to reach its
  post-flip frame. The code applies this to every image. Only the colorin
  step is gated on `dt_image_is_raw`, so no RAW/JPEG special case exists for
  crop. Re-checked for this lane against darktable master.
- John Ellis (LR SDK) puts the origin of crop and local-adjustment
  coordinates at the upper-left of the image "before any orientation"
  (cited in `~/tessera-evidence/rulings/REV-SP-B.out.md`, frame analysis).
- Lightroom Smart Preview DNGs carry an Orientation tag over unrotated
  pixels (LR-8b private sample, same ruling).
- The repo's `crates/import-lrcat/README.md` "Smart Previews and catalog
  orientation" section states the stored-frame convention (updated here to
  include RGB).
- Not verified: no Adobe-rendered pixel check exists for an RGB original.
  The evidence is third-party reverse engineering plus synthetic tests.

### 3. Ordinary RGB imports on main: inconsistent, NOT changed (coordinator decision)
- Ordinary (non-catalog-oriented) RGB imports: `RawImage::open` calls
  `RgbSource::open`, and the decoder consumes EXIF orientation
  (`rgb.rs` `open_with_orientation(path, None)` uses `decoder.orientation()`).
  `from_rgb` then reports orientation 1. Crop, masks, Upright and retouch for
  an ordinary JPEG/TIFF/HEIC are stored in the rotated (displayed) frame.
  Ordinary RAW imports use the sensor (stored) frame.
- Inside Tessera this is self-consistent: Develop, thumbnails and export
  agree for an ordinary JPEG. The new test asserts this unchanged behaviour
  (`ordinary import, EXIF n` in `lr8m_frame.rs`).
- User impact:
  1. **Lightroom catalog import, online RGB originals** (`lrcat.rs`, OfflineProxy-only
     `lightroom_orientation`: "Ordinary originals retain main's decoder
     orientation"). Imported Lightroom crop, CropAngle and masks are stored-frame
     values, but they are applied in the rotated frame. On a phone JPEG with EXIF 6 or 8,
     the crop aspect is transposed and the masks sit on the wrong content. On EXIF 3,
     both are mirrored through the centre. The user sees this as soon as they import a
     catalog of rotated phone photos whose originals are online.
  2. **The same catalog photo diverges by availability.** Offline at import
     (proxy, then relink) it is now correct. Online at import it uses the rotated
     frame. The two recipes are not interchangeable.
  3. **XMP interop.** `engine-api/src/recipe/crs.rs` maps `crs:CropLeft`,
     etc. verbatim, and `sidecar/src/develop.rs` reads and writes crs without
     an orientation transform. Adobe XMP edits on a rotated JPEG therefore land
     rotated in Tessera, and Tessera-written XMP for a rotated JPEG is
     mis-framed in Lightroom/ACR. For RAW they agree.
  4. **Copy/paste/sync settings** between a RAW and a rotated JPEG (or between
     JPEGs with different EXIF orientations) places crop and masks in different
     frames.
  5. Related, pre-existing: online catalog originals (RAW and RGB) ignore
     the catalog orientation. A rotation done in Lightroom is lost unless the
     photo came in as a proxy.
- Changing ordinary RGB imports to the stored frame would re-frame existing
  Tessera edits on rotated JPEGs. It would need a recipe migration (rotate the
  stored normalized geometry once) and is out of this lane's scope.

## Fix
| Item | Code | Test |
|---|---|---|
| Relinked RGB original read in stored frame; catalog orientation is display orientation (EXIF ignored) | `image-core/src/source.rs` `open_with_catalog_orientation` + `stored_rgb` (JPEG/TIFF/HEIC/PNG and working-space DNG) | `image-core/tests/lr8m_frame.rs::lr8n_relinked_rgb_original_uses_the_stored_frame_like_its_smart_preview` (un-ignored, completed: EXIF 3/6/8 x catalog 1-8 pixel-equal to an upright import; ordinary-import behaviour pinned unchanged) |
| Proxy -> relink keeps crop and masks on the same content, crop aspect unchanged | same | `image-core/tests/lr8n_relinked_rgb.rs::lr8n_rotated_jpeg_proxy_edits_stay_on_the_same_content_after_relink` (orientations 3/6/8, 1280x852 JPEG vs 640x426 LinearRaw proxy, normalized-grid content correlation 0.9999, mask-effect correlation 0.9999, 4% mask-shift control 0.86, rotated-frame control) |
| Export orients stored-frame RGB once, at the end | `pipeline-cpu` `RenderSource::StoredRgb { image, orientation }` (rendered exactly as `Rgb`); `export` `source_orientation`, AI-mask segmentation orientation, batch sizing; `pipeline-adobe` matches; `tessera-ffi/src/export.rs` `Source::StoredRgb` + `display_size` | `tessera-ffi` `preview::tests::lr8n_relinked_rgb_original_export_and_thumbnail_agree_with_develop` (3/6/8: Develop extent = stored crop 72x44, thumbnail == oriented Develop exactly, PNG export same dimensions, mean 0.245 / max 1 levels vs Develop) |
| Thumbnail agrees with Develop | none needed: `render_imported` already returns `metadata().orientation` | same test |
| Docs | `crates/import-lrcat/README.md` (RGB covered; ordinary/online RGB limit disclosed) | none |

No change to ordinary RGB imports, Cargo.lock, board.json or goldens.
Print/documents (`document/io.rs`) use the same `Source::open`, so they also orient once.

## Commits
- `dcec03fc` test(LR-8n): RED. The image-core tests failed as expected
  ("display orientation" in `lr8m_frame`; "the catalog orientation is the
  display orientation on both sides" in `lr8n_relinked_rgb`). In that commit
  the tessera-ffi test referred to `export::` (which resolves to
  `crate::export`) and did not compile; fixed in `89d1c82c`. The tessera-ffi
  RED is instead established by the independent reviewer's mutation runs on
  8a30f032 (REV-LR-8n, answer 3), all against
  `lr8n_relinked_rgb_original_export_and_thumbnail_agree_with_develop`:
  M1 (revert `source.rs` + ffi `export.rs` to main) fails at the display
  orientation assertion; M2 (revert only ffi `export.rs`) fails export vs
  Develop for orientation 3 (mean 31.9, max 116; tip 0.245 / 1); M3 (export
  `source_orientation(StoredRgb)` -> 1) fails the same assertion.
- `035f9533` fix(LR-8n)
- `89d1c82c` test(LR-8n): rotated-frame control; tighten export bound
- (this HANDOFF commit)

## LR-8n2 follow-up (REV-LR-8n, merge-ready verdict)
| Review item | Change | Evidence |
|---|---|---|
| S1 export AI-mask segmentation orientation for `StoredRgb` untested | `export/tests/ai_masks.rs::lr8n_stored_rgb_subject_export_segments_displayed_pixels_and_masks_the_stored_frame`: fake segmenter on `StoredRgb` 32x24, orientation 6. It asserts that the segmenter receives the display-oriented 24x32 input (the same contract as the RAW test `raw_subject_export_maps_display_mask_back_to_active_sensor_area`) and that the raster lands in the stored frame (displayed left half = stored bottom half; mask spans the stored width) | Passes on the fix. Under the reviewer's mutation M4 (`ai_masks.rs` `StoredRgb` orientation -> 1) it FAILS: `segmentation input is display-oriented (orientation 6)`, left (32, 24), right (24, 32). The mutation was reverted. |
| N1 ordinary-import pin checked only the extent | `lr8m_frame.rs`: for each of EXIF 3/6/8, the ordinary JPEG import renders pixel-equal to a lossless upright PNG holding the decoder's rotated content (crop, mask, Upright and lens) | Passes |
| N5 tessera-ffi RED | Cited the reviewer's M1-M3 above | none |
| S2 relink guard (stored aspect transposed against the Smart Preview `default_crop`) | **Not done; recorded as a follow-up.** Relink is decided by `catalog::source_path`, which runs on every thumbnail, export, Develop open and cull. A guard there needs a header read of the original on each call (LibRaw for RAW), a cached verdict keyed by file identity, a tie rule for near-square images, and a user-visible warning channel. That is not cheap, and refusing a relink silently would be unsafe. The old code had the same exposure, so this is not a regression. | none |
| S3 disclosures | MCP (`tessera-mcp` `pixels.rs`/`preview.rs`) is not catalog-aware and out of scope. It renders the proxy through `RgbSource::open` in the rotated frame, as it has since LR-8m, and never reaches a relinked original. `lrcat_fidelity.rs` (~250) renders JPEG originals in the rotated frame. If the S5 Adobe-pixel harness adds a rotated phone JPEG, that path must use the stored frame or it will report a false mismatch. | none |
| N2-N4 | Not changed (pre-existing scale-axis choice for orientations 5-8; double decode of working-space DNG; rotated HEIC unverified, with none in the user's catalog) | none |

### LR-8n2 gates (on 8205a888; origin/main still f77aae5d, no rebase needed)
| Gate | Result |
|---|---|
| `cargo test --release --no-fail-fast -p export -p image-core -p tessera-ffi` | PASS: 1008 passed, 0 failed, 47 ignored |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | PASS |
| `cargo fmt --all -- --check` | PASS |

These are test-only commits, and no production code changed since 8a30f032,
so the workspace, FFI and Swift gates below still apply.

## Gates (on 89d1c82c, after `cargo clean --release -p image-core -p pipeline-cpu -p pipeline-adobe -p export -p tessera-ffi`)
| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | PASS: 3499 passed, 0 failed, 67 ignored across 677 test binaries (load avg 11-20; no wall-clock failures, no reruns) |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | PASS (exit 0) |
| `cargo fmt --all -- --check` | PASS (exit 0) |
| `cd apps/mac && ./build-ffi.sh` | PASS; worktree clean afterwards (no bindings drift) |
| `tools/orchestrate/swift-gate.sh` | SWIFT GATE OK (XCTest 996 executed, 3 skipped, 0 failures; Swift Testing 5 passed) |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | PASS (only the usual linker deployment-target warning for blake3_neon.o) |

Focused runs: `lr8m_frame` 4/4, `lr8m_relink_parity` 2/2 (real `sample.dng`
present), `lr8n_relinked_rgb` 1/1; tessera-ffi `lr8n_`, `all_catalog_orientations`,
`sp_int*` and `lr13d*` filters all pass.

## Open items for the coordinator
- S2 relink guard (see the LR-8n2 table).
- Decide whether ordinary and online-catalog RGB imports should move to the
  stored frame (finding 3). That change would need a one-time recipe geometry migration.
- No Adobe-rendered check for an RGB original. REV-SP-B S5's private 12-pair
  harness could include a rotated, cropped phone JPEG that was imported offline.
