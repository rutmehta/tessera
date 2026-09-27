# M2-46 report: Export dialog — new formats, size limit, watermark

## Built
- **TesseraCore model** (`Sources/TesseraCore/Export/ExportSettings.swift`, new `ExportWatermark.swift`):
  - `ExportSettings.format` is now `OutputFormat` (JPEG, PNG, TIFF, AVIF, JPEG XL `jpeg_xl`, DNG) with extensions,
    per-format bit depths (AVIF 8/10/12, TIFF/JXL 8/16, DNG 32) and `usesQuality`. `FileFormat` (JPEG/PNG/TIFF) is kept
    unchanged for File ▸ Export Flat… in document mode (its exhaustive switch lives in `Document/`, outside this WP).
  - New fields `avif_speed`, `max_file_bytes` (`maxFileKilobytes`, 1 KB = 1,000 bytes) and `watermark`; missing keys
    decode to defaults, nil fields are omitted, so the engine's `deny_unknown_fields` JSON round-trips exactly.
  - `ExportWatermark`: text (text, font file, size, colour, rotation) or graphic (PNG path, scale), plus opacity,
    3 × 3 anchor and inset; encodes only the chosen kind's fields (engine tagged enum). Installed single-face .ttf/.otf
    font list via CoreText (.ttc excluded: engine reads face 0), default Arial; `problem` (missing files / empty text);
    placement arithmetic identical to `apply_watermark`.
  - `normalizeForFormat` keeps only engine-valid combinations; disabled-with-reason strings for lossy JXL, HDR,
    colour space on JXL/DNG, watermark on DNG; DNG "baked edits" explanation; summary covers the new formats.
- **Sheet** (`Sources/Tessera/Export/ExportSheet.swift`, `ExportController.swift`, new `ExportWatermarkViews.swift`):
  six-format bar; AVIF quality/bit depth/speed; JXL "Lossless" + bit depth; DNG note; JPEG "Limit file size to [ ] KB";
  disabled HDR checkbox (AVIF/JXL) and locked colour space with reasons; Watermark section (None/Text/Graphic, font
  pop-up + Other…, size %, colour, rotation, PNG chooser, scale, opacity, `AnchorPicker` 3 × 3 grid, inset) with a
  static placement preview and **Render with Engine** (a real 480 px PNG `export_batch` of the first selected photo
  with the watermark, into a temp folder). Drafts keep watermark / size-limit values across kind and toggle switches.
  Controller validation also rejects missing font/PNG files before the engine runs.
- DESIGN.md: "Export sheet additions" paragraph. ACCEPTANCE.md: section AA, steps 500–509 + identifier appendix.
- No Rust edits; `build-ffi.sh` produced no binding changes.

## Verified
- Gate `./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing`: exit 0,
  **247 XCTest tests, 0 failures, 0 skipped**; swift-testing 5 tests passed. ThemeLint green.
- New `ExportFormatsWatermarkTests` (7 tests): settings→engine JSON (exact watermark keys, `normalize_export_settings`
  round trip for every format), format-switch validity + engine refusals, watermark problems/placement, presets
  (new fields persist across relaunch; a hand-written M2-20 preset file and old UserDefaults JSON load unchanged;
  re-saving keeps them equal), real exports: AVIF 10-bit (ImageIO decodes, 480×320), JPEG XL (signature + ImageIO
  decode — `public.jpeg-xl` is readable on this macOS), DNG (IFD0: DNGVersion 1.4, 480×320, 32-bit float), JPEG size
  limit (≤ budget of 1/3 the unlimited size, > budget/4, decodes; impossible 200-byte budget fails, nothing written),
  text watermark brightens bottom-right only, graphic watermark red at centre and untouched corner, DNG + watermark
  refused. Existing `ExportPrintTests` (9) unchanged and passing.

## Gaps (engine, shown disabled with the reason)
- Lossy JPEG XL; HDR output (PQ/HLG/gain maps); JPEG XL in non-sRGB spaces; watermark on DNG; colour space for DNG
  (always linear Rec. 2020); size limit for non-JPEG formats.
- No engine watermark-preview API: the preview is a static placement drawing, plus an on-demand real export at 480 px
  (only for selection targets; album targets have no photo id on the Swift side, the button says so).
- Apple ImageIO does not decode the engine's linear float DNG (reads no size); the test checks the TIFF/DNG tags.
- Switching away from JPEG turns the size limit off (re-ticking restores the value); switching to DNG drops the
  watermark (choosing Text/Graphic again restores it).

## Not verified
- The sheet was not launched and screenshotted (ACCEPTANCE steps 501–509 are manual); engine preview in the running
  app, font pop-up length/performance with many installed fonts, and RAW-sized export timings are untested.
