# B5-27 handoff: the filter 1:1 detail pane matches the canvas in every working profile

Branch `wp/B5-27` from `origin/main` `031eaa56`. No engine crate touched (color-mgmt, compositor, filters
unchanged), no new FFI export or signature change (regenerated `TesseraFFI.swift` differs only in the
`filter_detail` doc comment and its uniffi checksum), Cargo.lock and board.json untouched.

## What the canvas actually does (checked before choosing the approach)
- The renderer writes the document's samples straight into the RGBA8 canvas surfaces: CPU `render::quantize`
  (`v * 255 + 0.5`), GPU `present.wgsl` `textureStore` into `rgba8unorm`. Nothing converts the profile and nothing
  applies a transfer curve ("Colours are the document's own encoding", `ResidentRenderer::present`).
- Document samples are the file's encoded values in its working profile: import is `to_rgba32f()` with the ICC kept
  as the document profile, export writes `q8(v)`.
- The host samples those surfaces as `rgba8Unorm_srgb` into an `extendedLinearSRGB` `CAMetalLayer`, so a canvas byte
  is shown as that sRGB byte. A Display P3 document is shown the same way (its values are not colour-managed on the
  canvas).

## The bug was bigger than P3
B5-18b made the pane write `srgb_encode(sample)` into an sRGB-tagged CGImage, on the premise that the canvas shows
samples as linear light. It does not, so the pane was **brighter than the canvas in every document**, sRGB
included (a Gaussian-blur pane differed from the canvas by up to 74/255 in an sRGB document). The B5-18b test
missed it because it compared the pane decoded as sRGB with the renderer's f32 samples, not with what the canvas
displays.

## Fix
- `crates/tessera-ffi/src/document/filters.rs` `detail()` (shared by `filter_detail` and `smart_filter_detail`,
  every filter including the Camera Raw Filter): the pane gets the canvas's bytes for the region. `srgb_u8` is
  replaced by `canvas_u8` (`clamp(0,1) * 255 + 0.5`, non-finite to 0), for colour and alpha. That is "encoded in the
  document's working profile" (the samples already are), with no new colour pipeline.
- Swift `FilterSheetModel.image` keeps the sRGB colour space on purpose: it is how the canvas decodes the same bytes,
  so the pane and the canvas show identical colours for sRGB and P3 documents. Only its doc comment changed.
  Tagging the pane with the document's ICC was rejected: it would make a P3 pane colour-correct while the canvas
  next to it is not, which is a mismatch.

## Tests
- RED `d409895e` (+ `d1bacc37`, which checks the P3 document by its samples because the built-in P3 ICC's
  description is "RGB built-in"). New `document_camera_raw_preview::detail_pane_matches_the_canvas_in_srgb_and_display_p3_documents`:
  16-bit PNGs with embedded sRGB / Display P3 ICC (saturated colour, outside sRGB when read as P3); asserts the P3
  document is not sRGB and its samples were not converted on open; for Gaussian blur and a Camera Raw Filter, the
  pane's bytes vs the canvas bytes of the same region (`read_presented_level` quantized as the canvas does).
  **Tolerance:** at most 1 byte (8-bit rounding) and at most 0.005 in display light after decoding both as the host
  does (sRGB). Pre-fix failure, both profiles:
  `srgb.png {"id":"gaussian_blur","params":{"radius":2}}: pane vs canvas bytes differ by 74`
  `p3.png {"id":"gaussian_blur","params":{"radius":2}}: pane vs canvas bytes differ by 74` (P3 case run first).
- Updated to the canvas contract (RED in the same commit): `document_filters::filter_detail_is_a_one_to_one_crop`
  (was "detail vs crate 72"), the `pane()` helper in `document_camera_raw_preview.rs` (raw bytes / 255; B5-18b's
  `detail_pane_renders_the_tile_and_matches_the_canvas` and `preview_at_100_percent_and_the_detail_pane_keep_the_detail_effects`
  failed with 0.286 against the old code), and Swift `DocumentCameraRawTests.mean` (averages the pane's samples;
  Camera Raw treats samples as linear, so +1 EV still doubles them).

## Behaviour change to flag
sRGB documents are **not** unchanged: their pane was too bright since B5-18b and now matches the canvas (it is back
to the pre-B5-18b byte encoding, keeping B5-18b's level/region and re-edit work).

## Remaining limitation (canvas, not the pane)
The canvas does not colour-manage non-sRGB documents: a Display P3 document's values are displayed as if sRGB (less
saturated than in a colour-managed viewer). The pane now follows the canvas exactly, so when the canvas gets a
profile-aware present (the compositor already has `DisplayDestination` / ICC LUT output), the pane must switch with it:
either run the same present on its bytes or tag the CGImage with the document ICC.
Separately, the Camera Raw Filter (`filters::camera_raw`, "compositor samples are already linear") treats document
samples as linear while import, export and the canvas treat them as encoded. That is Machine A's to judge.

## Gates
- `cargo test --release -p tessera-ffi`: 535 passed, 0 failed, 28 ignored.
- `cargo clippy --release -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --all --check`: clean.
- `apps/mac/build-ffi.sh`: OK (bindings: doc comment + checksum only, committed).
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (855 XCTest, 3 skipped, 0 failures; 5 Swift Testing passed).
