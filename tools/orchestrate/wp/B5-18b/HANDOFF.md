# B5-18b handoff — Camera Raw Filter: viewport previews, 1:1 detail fixes, engine name

Branch `wp/B5-18b` on origin/main `dd2cf300`. Rust: `crates/tessera-ffi/src/document/filters.rs` only (no engine crate,
board.json or Cargo.lock change). Swift: the backend call for the detail pane, the Camera Raw sheet, the generic
filter sheet and the B5-18 name mapping (removed). Bindings regenerated (`smart_filter_detail`).

## What changed
1. **Level-aware Camera Raw (follow-up 1 of B5-18).** `camera_raw` stays an adapter id but no longer counts for
   `full_resolution()`: `Spec::run` develops the image it is given (any level, any region) and counts the pixels.
   `camera_raw_halo(spec)` classifies the settings:
   - local (white balance, tone, curves, colour/HSL/grading/point colour/LUT: per pixel; Detail sharpening/NR:
     `pipeline_cpu::detail_halo`; Texture/Clarity guided filters: 24 px) → the stack's halo, so previews, bakes and
     the detail pane render the **visible region + halo at the view level**;
   - whole-image (lens profile / CA analysis / manual distortion or vignetting / defringe, Dehaze statistics,
     vignette, grain, lens blur, geometry, local masks, raw-only stages) → `None`, i.e. the **whole canvas of the
     view level** like any other whole-image filter (the detail pane uses the ≤4 MP level and reports it).
   Apply on pixel layers is unchanged: whole canvas, level 0. Smart-object applies/re-edits check the stack on the
   level with ≤ 262 144 px instead of the whole canvas (settings, document ICC and renderer are still validated;
   out-of-domain values still leave no history).
   The sheet's neutral base now turns lens profile and CA removal off (Camera Raw's defaults for non-raw images;
   the sheet has no Optics panel), so a default Camera Raw Filter is local and previews only the viewport.
2. **Detail pane re-edit (follow-up 3).** New FFI `smart_filter_detail(layer, index, filter_json, x, y, w, h)`
   (`StackEdit::Replace`, sharing `detail()` with `filter_detail`). Swift `filterDetail(layer:smartIndex:…)` routes
   to it; both the Camera Raw sheet (pane no longer hidden while re-editing) and the generic filter sheet pass
   their `smartIndex`, so a re-edit shows the filter once.
3. **Detail pane colour (follow-up 4).** `filter_detail` writes sRGB-encoded colour (alpha linear), matching the
   `sRGB` tag of `FilterSheetModel.image`; the canvas shows the same linear samples as extended linear sRGB.
4. **Engine name (follow-up 2).** `Spec::name()` returns "Camera Raw Filter" for `camera_raw` (history labels and
   `SmartFilterRecord.name`); `CameraRawFilter.displayName` and its two call sites are removed.

## Numbers (24 MP 6000 × 4000 pixel layer, release, `bench_camera_raw_24mp`; heap peak by counting allocator)
| case | before | after |
|---|---|---|
| fit (L2) preview, local settings, warm | 9392 ms, 3662 MB | 451 ms, 219 MB |
| fit (L2) preview, vignette (whole-image), warm | 5256 ms, 2564 MB | 199 ms, 143 MB |
| 100 % 1600 × 1000 preview, local, warm | 8401 ms, 3662 MB | 606 ms, 244 MB |
| 100 % preview, vignette (whole-image), warm | 8415 ms, 2564 MB | 5402 ms, 2564 MB (whole canvas at level 0, by design) |
| detail 280 × 280, local | 9114 ms, 3662 MB | 36 ms, 21 MB |
| detail 280 × 280, vignette | 5820 ms, 2564 MB (L0) | 946 ms, 145 MB (L2) |
| apply, full resolution | 8563 ms, 3662 MB | 10198 ms, 3662 MB (unchanged path; timing noise) |
Raw output: `evidence/bench-before.txt`, `evidence/bench-after.txt`. What remains of the fit-level peak is mostly the
level-0-sized proxy raster the presentation needs (`upsampled`), shared by every filter preview.

## Tests (RED first: they call the new APIs / expect the new encoding)
`crates/tessera-ffi/tests/document_camera_raw_preview.rs` (counting global allocator; tests serialised):
- `preview_work_scales_with_the_viewport_not_the_canvas`: same 256 × 192 window on 768 × 512 and 2304 × 1536
  canvases develops the same pixels (76 260 = window + halo), heap peak < 24 MB and not above the small canvas + 8 MB;
  vignette settings at the fit level develop exactly the level-2 canvas.
- `preview_matches_the_full_render_in_the_visible_region`: 100 % crop preview vs full apply ≤ 2.5/255 (tone, presence,
  sharpening, NR, saturation); apply develops w × h pixels; vignette fit preview vs full apply at level 1 ≤ 4/255.
- `detail_pane_renders_the_tile_and_matches_the_canvas`: camera_raw and Gaussian detail tiles at level 0, work ≤ tile +
  halo, pane decoded from sRGB matches the canvas preview of the same region within 0.01.
- `re_edit_detail_replaces_the_saved_smart_filter`: Gaussian and exposure smart filters; `smart_filter_detail` equals the
  single application, `filter_detail` stacks, bad index is an error.
- `engine_names_the_camera_raw_filter_and_checks_smart_filters_on_a_small_level`.
- `document_filters.rs::filter_detail_is_a_one_to_one_crop` updated to the sRGB encoding.
Swift: `DocumentCameraRawTests` (mean helper decodes sRGB; neutral draft has lens off; new
`testReEditDetailPaneShowsTheFilterOnce`; title test now passes with no Swift mapping).

## Gates
- `cargo test --locked --release -p tessera-ffi`: 46 test binaries, 500 passed, 0 failed, 23 ignored (no load-sensitive
  failures this run). `--test document_camera_raw_preview`: 5 passed, 1 ignored (the bench).
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`: clean.
- `apps/mac/build-ffi.sh`: OK (bindings: `smartFilterDetail`).
- `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK — 776 XCTest tests, 3 skipped, 0 failures (+5 swift-testing).
- `Support/make-app.sh debug` + `codesign --verify --deep --strict`: OK.
- Background app self-test (`open -g -n build/Tessera.app --args --new-document --camera-raw-selftest <dir>
  --nonactivating`): the app launched with no window, so the self-test (started from the document view) never ran;
  killed after 7 min, nothing on screen touched. Not re-attempted with a visible window (no focus taking).

## Not verified / follow-ups
- Whole-image settings at 100 % still develop the whole level-0 canvas (seconds on 24 MP). Making them viewport-sized
  needs the Develop renderer's region/tile path for RGB layers (image-core `render_tiles` over a cached RawImage),
  an engine-side change.
- The preview proxy is a level-0-sized raster even for a level-2 preview (~100–200 MB on 24 MP, all filters); a
  level-native presentation would need a compositor change.
- No on-screen check (no computer use); the pane colour fix is verified numerically against the canvas.
