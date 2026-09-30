# B5-29b handoff: Lua develop settings (LrC 15.5)

Branch `wp/B5-29b` on top of `wp/B5-29` `3acf4204` (A reviews both together). Changes only in
`crates/import-lrcat`: new `src/lua_develop.rs`, format dispatch `import_lrcat::develop` in `src/lib.rs`,
`tests/lua_develop.rs`, synthetic rows in `tests/data/lrc155/*.lua`. No FFI/Swift API change, no new deps.

RED commit `c963be39`: 10 tests, all failing against a stub API (parity, structures, unknown keys, mapping
table, escapes, malformed literals, format detection, depth, size/value limits, catalog import).

## Design (option a)
- `develop(image, text, pv)` picks the format by the first non-space token: `<` -> `xmp::parse` (unchanged);
  `s` then `=` -> `lua_develop::parse`; anything else -> decode error. Decode errors are prefixed `image <id>:`.
- `lua_develop::read`: literal-only, iterative (explicit stack, no recursion). Accepts `s =` + one literal
  (+ optional `;`): nested tables, `name =` / `["str"] =` / `[num] =` keys, "/' strings with Lua 5.4 escapes
  (`\a\b\f\n\r\t\v\\\"\'`, escaped newline, `\z`, `\xXX`, `\ddd`, `\u{X}`; result must be UTF-8), decimal
  numbers (finite only; no hex), `true`/`false`/`nil`. Rejected: identifiers as values, calls, operators,
  comments, long brackets, extra statements, duplicate keys.
- Limits: input <= 4 MiB (`MAX_INPUT_BYTES`, checked before parsing), nesting <= 32 (`MAX_DEPTH`; real rows
  nest <= 6), <= 500,000 values (`MAX_VALUES`). Tests: 10^6-deep nesting and oversized strings/arrays fail
  cleanly; 1 MiB string and depth 32 pass.
- Mapping: `KEY_MAP` (one-to-one, 199 entries: all 143 `CrsKey` names + 56 documented crs properties the
  recipe does not map). Mapped keys are rendered in Adobe's XMP shape (scalars as attributes, numeric
  `*Curve*` lists as `"x, y"` Seq, keyed tables as `rdf:Description`, lists as `rdf:Seq`, language-keyed
  tables as `rdf:Alt`) and decoded by the unchanged `xmp::parse`, so Lua and XMP rows share one translation.
  Unmapped keys, or values with no XMP shape, become `image <id>: <Key>: unknown Lua develop key; source
  preserved` report entries (not aborts), and the whole literal is kept in `recipe.unknown["lrcat_develop_lua"]`.
- Parity tests: the same edit as XMP and as Lua gives an identical `Recipe` (minus the retained
  `sidecar_xmp` text) and identical warnings, for a global edit and for Look + gradient mask.

## Real catalog (read-only copy only; counts only)
Copy sha256 prefix `eb60e744dbec2547`, unchanged after both runs. Original under ~/Pictures not opened.

`tessera import lrcat --inspect <copy>`: exit 0, 158.5 s wall, 5.2 GB peak RSS.
images 21,656, virtual copies 0, folders 121, keywords 32, albums 18, album groups 3, smart albums 3,
stacks 220, faces 9,293, schema 1504001.

`tessera import lrcat --apply --dest <scratch>`: exit 0, 331.1 s wall, 4.8 GB peak RSS; 21,656 recipes
written (3.0 GB bundle, deleted afterwards).
- Develop rows: 21,615 Lua rows parsed, 0 Lua decode errors; 41 empty rows imported as unedited.
- Report: 570,311 entries; 570,298 per-image develop entries across 21,615 images; 13 others
  (12 smart collections + 1 slideshow skipped, as in B5-29).
- Unknown Lua keys (entries): UprightFourSegmentsCount 21,615; UprightTransformCount 21,615;
  EnableDistractionRemoval 9,553; FilterList 8,029; ExtendedToneCurvePV2012 / Red / Green / Blue 737 each;
  AILook 615; Preset 146; CropConstrainAspectRatio 47; CustomTemperature 42; CustomTint 42; RemoveAreas 42;
  AutoToneDigestPV2 27; ExtendedToneCurveName2012 2; CustomLensProfile{Digest,DistortionScale,Filename,
  IsEmbedded,Name,VignettingScale} 2 each.
- Mapped but unsupported by the recipe (XMP path, same as an XMP row): e.g. ConvertToGrayscale,
  OverrideLookVignette, ToneCurveName2012, Upright* , Version, RedEyeInfo 21,615 each; Brightness/Contrast/
  Exposure/Shadows 21,239; CurveRefineSaturation 18,248; SDR* 17,133; LensProfileIsEmbedded 12,042.
- Structures the shared codec cannot translate (retained): MaskGroupBasedCorrections 653 (Mask/Image 616,
  radial Flipped 31, Aggregate 4, RangeMask 2); LensBlur 417 (Adobe focal range/bokeh); RetouchAreas 330;
  RetouchInfo 327; PointColors 277. 44 mask groups translated with the fidelity note. Look decoded on every
  row that has one (no Look warnings).
- 27 legacy rows report `number outside CRS range` for the 2012 sliders (as XMP would); not investigated.

## Follow-ups
- B5-29c: index per-image tables (files, develops, history, snapshots, faces, keywords, collections, GPS)
  by image id once up front; the per-image linear scans make import quadratic (~160 s inspect, 5 GB RSS).
- Report volume: ~26 entries per edited image, dominated by unsupported-but-harmless keys. Consider
  aggregating per key (count + first image) in the plan report.
- Decide which unknown keys deserve mapping (ExtendedToneCurve*, EnableDistractionRemoval, RemoveAreas).

## Gates
cargo test --release -p import-lrcat (all targets) and -p tessera-ffi --test lrcat / --lib lrcat;
clippy --all-targets -D warnings -p import-lrcat; fmt --all --check: pass. build-ffi.sh + swift-gate.sh:
SWIFT GATE OK (861 XCTest tests, 3 skipped, 0 failures; 5 swift-testing tests).
