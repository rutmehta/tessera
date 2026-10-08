# ENG-8 handoff: camera built-in lens corrections from maker notes

Branch `wp/ENG-8` from main `42bfbc6d`. Worker: Claude Opus 5.5.
Product decision (user, binding, batch 58 / ENG-7): lens corrections must
"match Lightroom". ENG-7b made built-in DNG opcode corrections apply in every
lens mode; this lane does the same for corrections stored in proprietary
maker notes.

## 1. Research

### Formats

| Camera family | Where | Format | Implementations read |
|---|---|---|---|
| Fujifilm X (RAF) | FujiIFD (tag 0xF000 of the TIFF that starts the RAF CFA section): `GeometricDistortionParams` 0xF00B, `ChromaticAberrationParams` 0xF00F, `VignettingParams` 0xF010, all (S)RATIONAL. `CropMode` 0x104D in the Fujifilm maker note of the embedded JPEG's EXIF | X-Trans I/II/III: 23/31/23 values = [?, 11 knots, 11 values]; CA = [?, 10 knots (no 0 knot), 10 red, 10 blue]. X-Trans IV/V: 19/29/19 = [?, 9 knots, 9 values]; CA [?, 9 knots, 9 red, 9 blue, ?]. Knots are source (uncorrected) radii in half-diagonal units; distortion in percent (output radius = r / (1 + d/100)); CA = relative red/blue radius offset; vignetting = relative illumination in percent. CropMode 2/4 (1.25x crop) scales knots by 1.25 | ExifTool `FujiFilm.pm` (RAF tags 0xF00B/0xF00F/0xF010, CropMode 0x104D); darktable `src/common/exif.cc` `_check_lens_correction_data` + `src/iop/lens.cc` `_init_coeffs_md_v2` (algorithm from F. Witherden, darktable PR #7092); RawTherapee `rtengine/lensmetadata.cc` `FujiMetadataLensCorrection` (adapted from darktable 4.6) |
| Sony (ARW) | SubIFD/SR2SubIFD tags `VignettingCorrParams` 0x7032, `ChromaticAberrationCorrParams` 0x7035, `DistortionCorrParams` 0x7037 (int16s) | [n, n values] / [2n, n red, n blue]; distortion d·2⁻¹⁴, CA c·2⁻²¹, vignetting 2^(0.5 − 2^(v·2⁻¹³ − 1)); darktable places knots at (i + 0.5)/(n − 1) (empirical) | ExifTool `Exif.pm` (forum6509, forum7640); darktable; RawTherapee |
| Olympus / OM (ORF) | ImageProcessing 0x150A (distortion, 4 floats: k2, k4, k6, corner radius), 0x150C (CA, 6 floats) | polynomial in output radius | darktable; RawTherapee |
| Panasonic (RW2) | IFD0 0x0119 `DistortionInfo` (16 int16) | Ru = Rd + s·(a·Rd³ + b·Rd⁵ + c·Rd⁷) (trou/panasonic-rw2 notes) | darktable (recent); RawTherapee has it **disabled** ("not yet working properly") |

LibRaw 0.22.2 (vendored) parses none of these tags, so this lane adds a small
bounded parser in raw-decode. lensfun is a profile database and has no
maker-note support.

### What Lightroom applies

- **Fujifilm: applied, not disableable (older bodies).** Adobe community
  threads on X-T2/X-T3 show "Built-in Lens Profile applied" and users report
  it cannot be turned off; Lightroom applies distortion, vignetting and CA from
  the RAF. Newer bodies (X-T5) show "Camera Settings" with a selectable lens
  but still apply the camera's correction by default. Sources:
  community.adobe.com "raf from fujifilm x t5 not loading built in lens
  profile distortion vignette until zoomed to 100" (966766); "no support for
  fujifilm x t2 lenses" (986176); dpreview "ACR not supporting Fujifilm
  lenses" (post 67156799).
- **Sony: only for some camera/lens combinations.** Adobe threads ("Sony
  built-in lens profile applied", a6500 automatic lens corrections) show the
  built-in profile for RX compacts and particular lenses (e.g. 16-70, 10-18),
  applied "at Sony's behest"; other E-mount lenses use ordinary (disableable)
  LCP profiles. Every ARW carries the parameters, even with in-camera
  correction off (the NEX-6 fixture: `DistortionCorrection Off`), and which
  combinations Lightroom treats as built-in is not public.
- **Micro Four Thirds: applied, not disableable** (well known), but there is
  no ORF/RW2 fixture to verify the formulas against, and RawTherapee disabled
  its Panasonic implementation as inaccurate.

### Scope decision

Implemented: **Fujifilm** (both layouts, CropMode). Clear format, two
independent implementations, a real fixture (X-E2S RAF) and evidence that
Lightroom always applies it. Deferred: Sony (Lightroom's application is
lens-dependent and undocumented; applying it to every ARW would over-correct
relative to Lightroom), Olympus/OM and Panasonic (no fixture; Panasonic
formula disputed).

## 2. Design

- `raw_decode::maker_lens::extract_raf_lens` reads the RAF directory, then
  bounded windows (≤ 256 KiB) of the CFA-section TIFF and the embedded JPEG.
  Layout and consistency checks follow darktable (knots identical across the
  three tags, counts 23/31/23 or 19/29/19) plus plausibility bounds; anything
  malformed yields no correction and never fails the decode.
  `RawMetadata::maker_lens: Option<MakerLens>` (enum, one Fujifilm variant)
  is filled by `RawSource::open`.
- `pipeline_cpu::maker_lens::sample` turns it into a `lens::CalibrationSample`
  in the active-area frame (half-diagonal metric via `coordinate_scale`):
  - geometry: the exact inverse of the source-radius spline, times
    darktable's autoscale (largest channel ratio over the frame-boundary
    radii, so no empty edges), fitted by deterministic Householder least
    squares to the sample's radial ratio `distortion_scale + odd0·r + k1·r² +
    odd1·r³ + k2·r⁴ + k3·r⁶`;
  - CA: red/blue radius ratios `c0 + c1·r² + c2·r⁴` at the source radius;
  - vignetting: relative illumination `1 + v0·r² + v1·r⁴ + v2·r⁶` at the
    source radius (applied before distortion, as in darktable/RawTherapee).
- Resolution (`lens_resolve`): wherever built-in corrections apply (Auto,
  None, Embedded, unavailable named profile; ENG-7b `uses_built_in`), DNG
  opcode lists win; otherwise the maker-note sample resolves as
  `CorrectionSource::MakerNote`. An available profile and AutoCalibrated keep
  their own source. The two built-in sources are never combined.
- Stage order is the existing resolved-lens order (the same as a profile):
  lateral CA on sensor-frame camera RGB before the matrices, vignetting after
  white balance, distortion in the composed geometry map after Effects.
  Because it is a calibration sample, the resident GPU path runs it through
  the existing `LensPlan` (CaPlan / VignettePlan / MapPlan) with no shader
  change.
- Remove CA (ENG-7b semantics): maker-note CA applies whatever the switch,
  like built-in per-plane DNG warps; the CA amount slider still scales it
  and 0 disables it. Distortion/vignetting amount sliders scale it like
  opcodes.
- `image_core::resident_lens_supported` excludes maker-note raws (the
  lens-free resident path would drop the correction in mode None);
  restricted raw admission refuses them like opcode raws.
- Lens notices count the maker-note correction as built-in.
- Preview disk cache: render epoch 3 (revision domain v3), so grid previews
  rendered before ENG-8 are not served.

## 3. Smart Previews (item 5)

- A Tessera Smart Preview is the camera-linear active area after the raw
  prefix. Generation runs `camera_linear_prefix`, which applies the resolved
  lateral CA (now including the maker-note CA) to the pixels before the
  downsample; the tail replays only vignetting and geometry from the stored
  correction (`camera_linear_tail_plan` uses `prefix_baked = true`). So CA is
  baked once and never replayed; distortion/vignetting are replayed once.
  `eng8_maker_note_correction_round_trips_without_double_application` checks
  the proxy against the original render (≤ 1e-5) with an exaggerated CA.
- The container records the original's maker-note parameters in
  `metadata.maker_note` (Fujifilm containers only; `{"kind":"none"}` when the
  raw has none; other cameras' containers are byte-identical to before). On
  reopen the sample is re-derived from them and must equal the stored one.
- A Fujifilm container from before ENG-8 lacks the member: it is reported
  **Stale** ("regenerate from original") in every mode that applies built-in
  corrections, as ENG-7c did for opcode raws; offline edits still sync. In
  AutoCalibrated or with an available profile it stays valid.
- External Lightroom Smart Preview DNGs: maker notes are read only from
  native RAF files, never from DNGs, so a DNG proxy never gets the maker-note
  correction on top of whatever Lightroom baked or wrote as opcodes (an
  external DNG with opcode lists is already refused as a proxy). Not verified
  on a real Lightroom Smart Preview of a RAF (none available here).

## 4. Item table (finding → code → test)

| Item | Code | Test |
|---|---|---|
| Read Fujifilm maker-note corrections | `raw-decode/src/maker_lens.rs`, `RawMetadata::maker_lens`, `RawSource::open` | `raw-decode` unit `maker_lens::tests` (synthetic RAF: both layouts, CropMode 1/2/4/8, big-endian, 12 malformed cases); `raw-decode/tests/maker_lens.rs` (X-E2S RAF equals ExifTool's values; the other four fixtures carry none) |
| Spline → calibration sample | `pipeline-cpu/src/maker_lens.rs` | `pipeline-cpu/tests/maker_lens.rs::maker_note_sample_matches_the_darktable_spline_model`: independent transcription of darktable `_init_coeffs_md_v2` + autoscale; X-E2S geometry max 0.281 px / rms 0.085 px at 4896×3264, CA max 0.071 px, vignetting max 0.15 %; synthetic barrel 1.25x crop: 0.074 px, 0.020 px, 0.09 % |
| Applies in every built-in mode; opcodes win; profile/AutoCalibrated keep their source | `lens_resolve.rs::resolve_with` | `maker_note_correction_applies_in_every_built_in_mode`, `dng_opcodes_take_precedence_and_are_never_combined` |
| Built-in CA regardless of Remove CA | `ResolvedLens::ca_enabled` (map, ca_active, geometry_active) | `built_in_ca_applies_whatever_the_remove_ca_switch` (resident LensPlan has CA with the switch off; CA amount 0 disables) |
| Notices | `lens_notice` | `notices_name_the_built_in_correction` |
| Real RAF is corrected (Auto = None) | — | `raw_fixtures_apply_maker_note_corrections_where_present` (built-in vs stripped L3 max diff 0.73 scene-linear) |
| Independent sanity check vs the camera | — | `raf_correction_matches_the_cameras_embedded_jpeg_geometry`: per-tile shifts of the render against the camera's embedded JPEG (1920×1280), after the best global scale + shift: residual **0.382 px** with the correction (30 tiles) vs **1.098 px** without (27 tiles), *in pixels of the 1920-wide JPEG* (≈0.97 px vs ≈2.8 px at full 4896 size; units corrected in ENG-8b, S3). The fitted global scale is **+0.14 %** with the correction and −1.43 % without: the autoscaled framing matches the camera's own to 0.14 %. Bound fixed before the corrected run: < 0.6× uncorrected and < 1 JPEG px |
| Engine parity L0 exact / L3 contract model (ENG-6) | — | `image-core/tests/maker_lens.rs` (RAF, Auto / None / Auto+Remove CA: L0 diff 0; L3 ≤ 1e-5 and display 0; differs from the stripped raw) |
| GPU resident path in mode None | `image_core::resident_lens_supported` | `pipeline-gpu/tests/maker_lens.rs` (synthetic strong correction L0/L2 and RAF L3: GPU vs CPU ≤ 1 code value; RED: RAF None L3 differed by 197) |
| Restricted admission | `raw_admission.rs` | `unsupported_orientation_and_correction_metadata_refuse` |
| Smart Previews | `smart_preview_codec.rs` | `eng8_maker_note_correction_round_trips_without_double_application`, `eng8_maker_note_member_only_for_fujifilm`, `eng8_legacy_fujifilm_container_is_stale_and_mismatches_are_rejected` |
| Preview cache | `previews` epoch 3 / domain v3 | `previews_cached_under_an_earlier_render_epoch_miss` (epoch 1 and 2 directories miss) |
| Export band cost with CA | `export/src/gpu.rs` `CA_BYTES_PER_PIXEL = 18` | existing `five_fixture_*` export tests (RAF web pyramid declined to tiles before); max actual/planned per camera: CR3 0.989, ARW 0.985, RAF 0.975, NEF 0.984, DNG 0.984; `five_fixture_device_peak_within_budget` (run alone) passes, RAF peak ≤ 379 MiB |
| Docs | `docs/RELEASE-NOTES.md`, `pipeline-cpu/LENS_M2.md` | — |

RED evidence: `0432e55e` (parser: 4/5 unit tests and the fixture test
failed), `9d72630f` (pipeline: 6/7 failed; the opcode-precedence test passed
trivially), `9c246d7a` (codec 3/3, GPU 2/2, admission 1 failed; the image-core
L0/L3 tests passed already because the CPU stage graph runs the resolved
lens), `ce7d79e3` (previews: const assert fails to compile).

## 5. Golden and expectation audit

User decision ("match Lightroom") is the reason for every change below.
First full run after the implementation (`e0eb19b0`): 3593 passed, **5
failed**, all addressed in `4ba4c35d`:

| Test / golden | Before | After / resolution |
|---|---|---|
| `pipeline-cpu/tests/golden.rs` `fuji-raf.png` | lens-off render, bit-identical | The RAF now applies its built-in correction even in lens mode None: 247 441 of 249 696 pixels changed, max 225/255 (geometry moves edges). The goldens are documented as the immutable *optics-off* render, so the test (and `examples/regenerate_goldens.rs`) now removes `maker_lens` explicitly; **`fixtures/golden/fuji-raf.png` is unchanged**, and the other four goldens are untouched and still bit-identical. If the coordinator prefers the golden to pin the corrected render instead, regenerate only `fuji-raf.png` with the strip removed. |
| `lens_default.rs::raw_fixtures_default_applies_no_estimated_geometry` | RAF Auto = None, no sample | Expectation changed: a `MakerNote` source is skipped like `Embedded` (camera data, not an estimate); `tests/maker_lens.rs` covers it |
| `pipeline-gpu/tests/fixtures.rs::fixture_level3_tolerance_per_operator_and_output` | per-operator audit on all fixtures | The RAF renders through the fused lens plan, so some stages bypass the audit hook ("stage must be audited"). Its full-image comparisons still run on the corrected RAF (scene-linear 3.1e-6, ΔE2000 7.1e-4, display 1 — all within the unchanged tolerances); the operators are additionally audited on the same sensor data without the correction. No tolerance changed |
| `export` `five_fixture_web_scale_tolerance`, `five_fixture_exports_use_the_inline_effects_path_bit_identical_to_the_map` | RAF web pyramid on bands | Band plan underestimated CA scratch, RAF fell back to tiles. Fixed in the cost model (above); assertions unchanged |

No other test, pin, digest or tolerance changed. In particular the
smart-preview container pins, import-lrcat goldens, recipe hashes and the
CR3/NEF/ARW/DNG renders are unchanged: non-Fujifilm containers keep their
bytes (no `maker_note` member) and `maker_lens` is `None` for those raws.

Render output changed with unchanged expectations: every test that renders
the RAF fixture with Auto/None/Embedded lens settings (e.g. export band/full
chain tolerance tests, tessera-ffi develop fixture sessions, image-core
fixture L3 extremes). The ENG-6 L0/L3 parity tests in `image-core/tests/fixture.rs`
use `AutoCalibrated` and are unaffected; the new `image-core/tests/maker_lens.rs`
covers the default modes.

## 6. Gates

All final gates ran on `2c470937` after `cargo clean --release -p raw-decode
-p pipeline-cpu -p image-core -p pipeline-gpu -p export -p previews -p merge
-p pipeline-adobe -p tessera-ffi -p tessera-mcp`, target
`~/.cache/tessera-target/ENG-8`, fixtures symlinked,
`TESSERA_REQUIRE_RAW_FIXTURES=1`. The HANDOFF commit follows (docs only).

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | pass, exit 0: 700 test binaries, **3598 passed, 0 failed, 102 ignored**. No raw-fixture SKIPPED lines (only model-weight skips, unchanged). Load 9.3 at start, 14.9 at end; no wall-clock failures, no reruns |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | pass (exit 0) |
| `cargo fmt --all -- --check` | pass (exit 0) |
| `cd apps/mac && ./build-ffi.sh` | pass (exit 0); worktree clean afterwards (no bindings drift, no FFI surface change) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: XCTest 996 tests, 1 skipped, 0 failures; Swift Testing 5 tests in 2 suites passed |
| strict release `swift build --product Tessera` (`-strict-concurrency=complete -warnings-as-errors`) | pass ("Build of product 'Tessera' complete!") |
| `five_fixture_device_peak_within_budget` (ignored, run alone) | pass; RAF peak 351-379 MiB |

Earlier run on `e0eb19b0` (before the fallout fixes): 3593 passed, 5 failed
(see section 5). Clippy initially flagged `manual_map` and
`type_complexity` in the new tests (`2c470937`).

## 7. Not done / follow-ups

1. **Sony ARW** maker-note corrections (0x7032/0x7035/0x7037) are not
   applied: Lightroom applies Sony's built-in data only for some
   camera/lens combinations, which are not public. Applying them to every
   ARW would correct where Lightroom does not (the NEX-6 fixture has
   in-camera distortion correction Off). Needs a decision on a list of
   combinations, or evidence from Lightroom renders.
2. **Olympus/OM (ORF) and Panasonic (RW2)** distortion: formats known
   (darktable), Lightroom applies them, but there is no fixture to verify
   against and RawTherapee disabled Panasonic as inaccurate. Needs real
   files (and ideally Lightroom renders of them).
3. ~~Native AI-mask export of a Fujifilm raw~~ — **wrong in this
   section as first written** ("Develop is not affected", "the existing
   behaviour"): AI masks were misaligned in Develop too, and the export
   failure was new and unavoidable for every Fujifilm raw. Both fixed in
   ENG-8b below.
4. **X-Trans IV/V (19-value layout)** is parsed and applied like I-III.
   Lightroom applies the camera's correction by default on those bodies but
   lets users choose a lens profile ("Camera Settings"); whether its "None"
   then drops the built-in correction is not verified. No fixture.
5. **Lightroom Smart Preview DNGs** of RAFs: never given the maker-note
   correction (only native RAFs are parsed); not verified against a real
   Lightroom Smart Preview of a RAF.
6. The fitted polynomial differs from darktable's linear spline by at most
   0.28 px on the fixture (rms 0.085 px). Neither is necessarily
   Lightroom's exact curve; the embedded-JPEG check is the only comparison
   with the camera's own output.

---

# ENG-8b: review follow-up (REV-ENG-8 CHANGES REQUIRED)

Same branch `wp/ENG-8`, on top of `f1402008`; origin/main had not moved
(`42bfbc6d`), so no rebase. Worker: Claude Opus 5.5. The reviewer's S4
(driving Lightroom Classic) was not done, per the coordinator: nothing in
Lightroom-managed catalogs or folders was opened or written.

## Root cause (B2) and what was verified

Every Tessera renderer that draws local adjustments for a raw applies them
**before** the composed geometry stage, where the lens warp of a calibration
sample (maker-note, profile, estimate) and manual distortion run:

- the Native reference (`render.rs`: locals, Lens Blur, effects, then
  `geometry_mapped`);
- Develop's stage graph, for **both** processes. The review expected the
  Adobe process to be unaffected (its reference, `pipeline_adobe`, applies
  locals after a warped base render). But Develop's renderer draws Adobe
  recipes through the stage graph, and thumbnails (`render_imported`) and
  Adobe exports (`adobe_render`, ENG-10) use that renderer too. Measured on
  the RAF: a raster segmented from the warped default render lands with IoU
  **0.952** in an Adobe-process thumbnail. The same procedure gives 1.000
  on the four other fixtures, which have no warp. With the fix the RAF
  measures 0.992.

AI masks, however, were segmented from the default (warped) render: Develop
(`AiMaskJob::compute`) and export (`ready_hooks`). Fix:
`pipeline_cpu::mask_segmentation_settings(metadata)` is the as-shot default
with `lens.distortion_scale = 0`. That removes exactly the post-local warp.
It is kept at 100 when the raw has DNG opcode lists, whose warp runs in the
raw prefix before the locals (so it never was misaligned). Vignetting and
CA do not move content and stay. Develop and export use it for both
processes.

## B1

`ai_masks::render_with_hooks` no longer fails on a lens warp. It used to
finish the pre-local render with the public geometry operator, which has no
lens warp, and errored when one existed. Now such a render goes through
`render_hooked_native`: the full reference pipeline
(`render_linear_scaled_with_local_hook`) with the export's rasters at the
local barrier, Lens Blur at its depth barrier (the supplied provider, same
estimate as before), and the same tone map as the old path. Like the old
path, it applies local adjustments but not retouch spots. Every caller
shares this path: file export (`render_one_cancellable`, FFI), print and
documents (`render_pixels_with_notes`), DNG export, and the denoise/Lens
Blur hook (`depth::render`). Non-warped AI exports keep the old path and
bytes.

## Item table (finding → code → test)

| Item | Code | Test (RED → green) |
|---|---|---|
| B2 frame helper | `pipeline-cpu/src/lens_resolve.rs::mask_segmentation_settings` | `maker_lens.rs::mask_segmentation_settings_leave_out_post_local_warps_only` |
| B2 reference A/B (reviewer's check) | — | `maker_lens.rs::raf_ai_mask_from_segmentation_settings_lands_on_its_content`: IoU no correction 0.9979 / corrected + new input **0.9953** / corrected + old input 0.9671 (RED: 0.9671). The 0.0026 residual is the smoothing used to make the threshold mask robust (applied before the warp for the raster, after it for the target); with a radius of 12 it is 0.0014, while the old input still loses 0.018 |
| B2 Develop | `tessera-ffi/src/masks.rs` (`AiMaskJob::compute`) | `masks.rs::eng8b_alignment_tests::develop_ai_mask_on_the_raf_lands_on_its_content`: Develop's own job with a threshold segmenter, then Develop's renderer with the mask hooks, −3 EV: IoU **0.9944** (RED 0.9541) |
| B2 thumbnails / Adobe process | (same input, shared renderer) | `preview.rs::eng8b_thumbnail_tests::adobe_process_thumbnail_ai_mask_on_the_raf_lands_on_its_content`: Adobe-process recipe with a stored imported raster, through `render_imported`: IoU **0.9923** (RED 0.9523) |
| B2 export | `export/src/ai_masks.rs::ready_hooks` | `export/tests/eng8b_ai_masks.rs::raf_ai_mask_export_lands_on_its_content`: IoU 0.9873 with the correction vs 0.9888 on the same raw without it. The raster is segmented at 1/3 scale, so neither reaches 1. RED: the export failed |
| B1 all fixtures × modes | `render_hooked_native` | `eng8b_ai_masks.rs::ai_mask_exports_succeed_on_every_fixture_and_lens_mode_and_match_develop`: 5 fixtures × Auto/None/recipe default, constant segmenter, 16-bit TIFF vs Develop's renderer with the same raster. Every export succeeds; max/mean (8-bit levels): CR3 0.977/0.333, **RAF 0.977/0.333**, NEF 1.062/0.273, DNG 0.981/0.311, ARW 0.996/0.330. Bounds are ENG-9's: 1.0 max + 0.1 Native grey-point exception, 0.35 mean. The NEF's 1.062 is on the unchanged non-warp path. RED: the RAF failed in all three modes |
| B1 print/documents and DNG | (shared path) | `eng8b_ai_masks.rs::raf_ai_mask_print_and_dng_export_succeed` (RED: failed) |
| B1 Lens Blur / denoise hook | `render_hooked_native` | `export/src/depth.rs::eng8b_tests::ai_mask_with_lens_warp_and_lens_blur_renders` (RED: failed). Denoise shares the same call (the denoiser is passed through); it needs model weights and is not run separately |
| B1 old expectation | — | `export/tests/ai_masks.rs`: `ai_lens_warp_fails_explicitly_instead_of_exporting_misaligned_masks` → `ai_lens_warp_exports_through_the_hook_aware_renderer` (succeeds, the mask applies) |
| S1 | `docs/RELEASE-NOTES.md`, HANDOFF §7.3 | — |
| S2 | `smart_preview_codec.rs`: a stored sample ≠ today's derivation → Stale "regenerate from original" | `smart_preview_codec.rs::eng8_legacy_fujifilm_container_is_stale_and_mismatches_are_rejected` (two changed samples are Stale; source/parameter inconsistencies stay invalid and are *not* Stale) |
| S3 | §4 above; the JPEG test now prints units and the fitted scale | `raf_correction_matches_the_cameras_embedded_jpeg_geometry`: 0.382 vs 1.098 JPEG px, scale +0.14 % vs −1.43 % (reproduces the reviewer's values) |
| (7) checksum pin | — | `maker_lens.rs::raf_corrected_default_render_checksum`: blake3 of the 1/8 default (Auto) RAF render, `f5f82c27b09e60366a8f500d8232bdc7f250b6873b4ce36a698fc4d37348b729` (612×408; stable across thread counts) |
| N1–N4 | comments in `pipeline-cpu/src/maker_lens.rs`, `raw-decode/src/maker_lens.rs`, `raw-decode/src/lib.rs` | — |

RED commit `1bf94b9b` (all of the above failing, with a stub helper).
Fixes: `b50dbe80` (B1/B2), `107074ac` (S2), `b3c4ec96` (nits), `2044d898`
(S3 test output).

## Golden and expectation audit (ENG-8b)

- Changed expectation: `export/tests/ai_masks.rs` lens-warp test: it used to
  expect an error, now it expects a successful export (B1).
- Changed expectation: the ENG-8 codec test's "tampered sample" case is now
  Stale rather than invalid (S2).
- New pin: the corrected RAF render checksum (7).
- No golden, fixture image, tolerance or existing pin changed. The
  segmentation input changes only for raws whose resolved lens has a
  post-local warp. On main's five fixtures that is only the RAF, so the
  existing AI-mask tests and pins (synthetic or non-warped) are unaffected.

## Gates (ENG-8b)

Run on `2044d898` after `cargo clean --release -p raw-decode -p pipeline-cpu
-p image-core -p pipeline-gpu -p export -p previews -p merge -p
pipeline-adobe -p tessera-ffi -p tessera-mcp`, target
`~/.cache/tessera-target/ENG-8`, fixtures symlinked,
`TESSERA_REQUIRE_RAW_FIXTURES=1`. The commit that follows changes only the
release note and this HANDOFF.

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | pass, exit 0: 701 test binaries, **3607 passed, 0 failed, 102 ignored**. No raw-fixture SKIPPED lines. Load 5.7 at start, 13.4 at end; no wall-clock failures, no reruns |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | pass |
| `cargo fmt --all -- --check` | pass |
| `apps/mac/build-ffi.sh` | pass; no bindings drift (only the docs were modified in the worktree) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: XCTest 996 tests, 1 skipped, 0 failures; Swift Testing 5 passed |
| strict release `swift build --product Tessera` | pass |

## Residual risks / follow-ups (ENG-8b)

1. **Imported Lightroom rasters on warped raws.** Rasters imported from
   Lightroom (`mask_key`) are applied as stored, in the frame where the
   locals run (pre-warp). If Lightroom stores them in its lens-corrected
   frame, they are offset by the warp on Fujifilm raws and on any raw where
   a profile applies. Tessera-generated rasters are now correct. Mapping
   imported rasters through the warp needs evidence of Lightroom's raster
   frame (S4-type parity work, not done here).
2. **External Lightroom Smart Preview DNG proxies** with Adobe recipes
   render locals after the warp (`pipeline_adobe` reference,
   `smart_preview_render.rs`). Such proxies carry no maker-note data, so
   only a profile or an AutoCalibrated estimate on a proxy would misalign.
3. **Object prompts** (clicks and boxes for Object/Person masks) are
   normalized coordinates from the host. I did not check whether the host
   maps them through the lens warp. If it does not, prompts can point up to
   ~0.5 % off on a Fujifilm raw (the raster itself is aligned).
4. **Depth masks for Adobe-process recipes** use the pre-geometry depth
   plane. This is consistent with the stage graph, but not checked against
   the `pipeline_adobe` proxy path above.
5. S4 (Lightroom parity of geometry, framing, vignetting and CA) remains
   open.

---

# ENG-8c: re-review follow-up (REV2-ENG-8 CHANGES REQUIRED)

Same branch `wp/ENG-8`, on top of `0aaae71c`; origin/main had not moved
(`42bfbc6d`), so no rebase. Worker: Claude Opus 5.5. Coordinator ruling on
NB1: option (b), apply the maker-note geometric warp (and its CA) in the raw
prefix, where DNG opcode warps run.

## Design change

ENG-8/8b applied the Fujifilm correction as a calibration sample: CA before
the matrices, vignetting after white balance, distortion in the composed
geometry map after the local adjustments. Every tool that maps a click
through crop and orientation only (`MaskSpace.toMask/fromMask`: brushes,
gradients, Object/Person prompts, overlays and handles, healing spots)
therefore landed up to ~27 px (full size) off on Fuji raws.

ENG-8c makes it an `Embedded` built-in stage (`embedded_lens::MakerPrefix`).
It is one resample of demosaiced camera RGB in the full sensor frame, in
`camera_linear_prefix`, right where the opcode stages run:
- green source `q = p·(1 + d·(F(r) − 1))`, with F the fitted radial ratio;
- red/blue source `q·(1 + c·(ratio(ρ) − 1))`;
- the sample at each channel's source multiplied by the vignetting gain
  there, `1 + v·(1/I − 1)`.

The fit (`pipeline_cpu::maker_note_sample`) is unchanged. The model applies
vignetting before distortion, as darktable does. Consequences:

- The pre-geometry frame (locals, masks, prompts, segmentation, retouch,
  crop, Upright) is the corrected, displayed frame. With default settings
  the geometry stage is the identity on the RAF
  (`raf_pre_geometry_frame_is_the_displayed_frame`: pre-geometry render ==
  final render, bit for bit).
- `CorrectionSource::MakerNote` is removed. The correction resolves as
  `Embedded` (no sample) wherever built-in corrections apply, and opcode
  lists still win.
- GPU: the resident sensor-frame CA stage carries it (`CaPlan.maker`,
  `MakerPlan`; `lens.wgsl` evaluates the same expressions for all three
  channels with the gain), so Fuji raws stay on the resident GPU path in
  Develop and in band exports. A displacement beyond the resident halo
  (max displacement + 2 > 32 px) falls back to the CPU chain, as for any CA
  plan; the X-E2S/18-55 needs about 31. Maker-only Embedded snapshots now get
  resident Smart Preview tails (fully baked).
- `pipeline_cpu::built_in_prefix` / `defer_post_local_distortion` draw the
  pre-geometry frame without removing a raw-prefix warp.
  `mask_segmentation_settings` uses them, so for raws it is the as-shot
  default again; it still removes profile/estimate warps.
- **Pre-existing bug found by the reviewer's suggested check** (DNG opcode
  warps in Develop): `develop_before_geometry` and Develop's depth input set
  `distortion_scale = 0` to defer the composed warp, which also removed
  opcode stage warps (the same scale). Develop drew opcode raws unwarped
  while export warped them: 1.25 off the reference on a synthetic
  WarpRectilinear. Both now use `defer_post_local_distortion`. This affects
  main for DNGs with OpcodeList3 warps.
- AI-mask export (NS1): raw-prefix built-in corrections take the old fast
  path again (`render_linear_before_geometry` already includes them). Only
  sample warps (profile, estimate) and manual distortion use
  `render_hooked_native`, which stays for those (B1 generalised). RAF:
  **4.4 s** (main 4.3 s, ENG-8b 17.2 s; load 8.5).
- N2: `render_hooked_native` applies local adjustments but not retouch
  spots, like the old path. Retouch plus AI masks is rejected before either
  path (`retouch_float`: "retouch export with AI masks, denoise or depth is
  not supported"), so no export drops spots silently. It now applies only
  to profile, estimate and manual-distortion warps, not Fuji.
- Export band plan: a maker prefix stage costs 24 B per sensor pixel
  (`MAKER_BYTES_PER_PIXEL`; 18 B left the RAF at 1.045 actual/planned).
  After: CR3 0.989, ARW 0.985, RAF 0.975, NEF 0.984, DNG 0.984; device peak
  RAF 336–365 MiB.
- Smart Previews: source `Embedded`, with the derived model recorded as the
  correction sample, so a different derivation is Stale. ENG-8/8b
  containers (source `MakerNote`: CA baked, geometry in the tail) are
  **Stale**, "regenerate from original". Pre-ENG-8 Fuji containers stay
  Stale as before.

## Crop coordinates on Fuji raws

Crop rectangles are normalised in the frame the geometry stage receives.
- **vs main:** main drew Fuji raws uncorrected, so a crop stored on main now
  frames corrected content. It moves by up to ~26 px (0.54 % of the width)
  at mid-radius and 0 at the centre and corners. The release note already
  said this ("Existing Fujifilm edits shift slightly"), and it now
  explicitly names crops saved before this release.
- **vs ENG-8/8b:** the crop framed the same corrected content (the warp was
  in the geometry map before the crop), so nothing moves apart from
  interpolation. That code was never released.

## Item table (finding → code → test, RED in `b447959f`)

| Item | Code | Test |
|---|---|---|
| NB1 radial / brush placement (the reviewer's probe) | prefix stage | `pipeline-cpu/tests/maker_lens.rs::raf_hand_drawn_masks_land_where_drawn`: drawn at (0.85, 0.80), level 2: radial **0.62 px**, brush **0.63 px**. RED: did not compile (new API); the reviewer measured 6.83 px on ENG-8b |
| NB1 Object prompt through Develop's AI job | prefix stage | `tessera-ffi masks.rs::develop_object_prompt_on_the_raf_lands_where_clicked`: **0.04 px** (RED 6.80 px) |
| NB1 healing/clone spot | prefix stage | `develop_retouch_spot_on_the_raf_lands_where_drawn`: 1.03 px vs 1.01 px on the same raw without the correction (spot-geometry floor). RED: 6.74 vs 1.01 |
| NB1 overlay / handle round trip | prefix stage | `raf_overlay_handle_and_effect_coincide_under_a_crop`: with a crop, overlay (raster through `crop_plane`) vs effect **0.23 px**, handle (`fromMask` formula) vs effect **0.10 px** (RED: overlay off) |
| Frame identity | prefix stage | `raf_pre_geometry_frame_is_the_displayed_frame` (bit-identical) |
| AI masks IoU: Develop / thumbnail / export / reference | — | Develop **1.0000** (8b 0.9944), Adobe thumbnail **1.0000** (8b 0.9923), export 0.9894 vs 0.9888 without the correction, reference 0.9981 vs floor 0.9979 |
| Develop applies opcode warps | `image-core render.rs`, `tessera-ffi develop.rs` | `image-core/tests/builtin_lens.rs::develop_applies_opcode_stage_warps_from_metadata`: L0 == reference, L2 warped, `with_metadata` included (RED: 1.25 off) |
| Engine/CPU parity | — | image-core `maker_lens` L0 diff 0, L3 ≤ 1e-5 / display 0 (L3 model updated to keep raw-prefix warps); pipeline-gpu `maker_lens`: GPU vs CPU ≤ 1 code value, RAF on the resident path (`can_render_resident`) |
| Export timing / parity | `export/src/ai_masks.rs` | `eng8b_ai_masks.rs`: all 5 fixtures × 3 modes succeed within ENG-9 bounds (RAF 0.977/0.333); timings printed (RAF 4.4 s) |
| Smart Preview compatibility | `smart_preview_codec.rs` | `eng8_legacy_fujifilm_container_is_stale_and_mismatches_are_rejected` (+ ENG-8b container Stale), round trip without double application (proxy vs original ≤ 1e-5) |
| Camera-JPEG check | — | unchanged result: 0.380 vs 1.098 JPEG px, scale +0.14 % vs −1.43 % |
| Mask segmentation settings | `lens_resolve.rs` | Fuji now keeps `distortion_scale` 100; opcode raws 100; others 0 |

## Golden and expectation audit (ENG-8c)

- Re-pinned with a reason: the corrected RAF checksum, from
  `f5f82c27…b729` (ENG-8b, Lanczos geometry map) to
  `679f717bc145015be43eba09ba57ac0d7e06ea04162e456adb20d700183af3cc` (prefix
  bilinear resample).
- Changed expectations, all from the design change: the resolution source
  (`Embedded`, no sample) in `maker_lens.rs` and `lens_default.rs`; the
  resident plan carries the correction as `ca.maker` with no post-local
  map or vignette; Smart Preview source `Embedded`; mask segmentation keeps
  distortion for Fuji; the reference IoU A/B (default render ==
  segmentation input now).
- The L3 contract model (`image-core/tests/common/preview.rs`) keeps
  raw-prefix warps. Before, it removed opcode warps too, which hid the
  Develop bug above.
- No golden PNG, tolerance or other pin changed.

## Gates (ENG-8c)

Run on `48fed710` after `cargo clean --release -p raw-decode -p pipeline-cpu
-p image-core -p pipeline-gpu -p export -p previews -p merge -p
pipeline-adobe -p tessera-ffi -p tessera-mcp`, target
`~/.cache/tessera-target/ENG-8`, fixtures symlinked,
`TESSERA_REQUIRE_RAW_FIXTURES=1`. The commit that follows changes only the
release note and this HANDOFF.

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` | pass, exit 0: 701 test binaries, **3613 passed, 0 failed, 102 ignored**. No raw-fixture SKIPPED lines. Load 10.2 at start, 56 at end; no wall-clock failures, no reruns |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | pass |
| `cargo fmt --all -- --check` | pass |
| `apps/mac/build-ffi.sh` | pass; no bindings drift (only the docs were modified) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: XCTest 996 tests, 1 skipped, 0 failures; Swift Testing 5 passed |
| strict release `swift build --product Tessera` | pass |
| `five_fixture_device_peak_within_budget` (ignored, run alone) | pass; RAF 336–365 MiB |

## Residual risks / follow-ups (ENG-8c)

1. A Fuji lens with more distortion than the resident halo allows
   (displacement > ~30 px at full size, e.g. wide zooms) renders through the
   CPU chain: correct, but slower. No such fixture.
2. The CPU prefix resample is bilinear, like opcode warps; the old geometry
   map was Lanczos-3. Slight softening of the warped areas; the camera-JPEG
   check is unchanged.
3. Imported Lightroom rasters (R1), external Smart Preview DNG proxies on
   the Adobe path (R2) and Adobe depth masks (R4) remain open as in ENG-8b.
   Object prompts (old R3) are fixed.
4. S4 (Lightroom parity) remains open.

---

# ENG-8d: third review follow-up (REV3-ENG-8 CHANGES REQUIRED)

Same branch `wp/ENG-8`, on top of `26f9c4ca`. Worker: Claude Opus 5.5.

## Item table (finding → code → test; RED `6a3f6b82`)

| Item | Code | Test and numbers |
|---|---|---|
| **NB2** bilinear prefix resample softened every Fuji photo | `embedded_lens::lanczos3` / `lanczos3_taps` (normalized separable Lanczos-3, 6×6 edge-clamped taps, the geometry map's kernel); the same taps in the `lens.wgsl` maker branch (`94a775ae`) | `pipeline-cpu/tests/maker_lens.rs::raf_prefix_resample_keeps_fine_detail`. The reviewer's method: green plane, √-encoded, full resolution, default settings, against the same render with distortion and CA amounts at 0 (the stage then samples integer positions). Per ring, **before (bilinear) → after (Lanczos-3)**: Laplacian² 0.315 / 0.305 / 0.324 / 0.325 / 0.318 → **0.699 / 0.653 / 0.691 / 0.699 / 0.683**; gradient² 0.70 / 0.68 / 0.73 / 0.71 / 0.70 → **0.97 / 0.93 / 0.98 / 0.98 / 0.95**. Bound: Laplacian² ≥ 0.6. ENG-8b's Lanczos geometry map measured 0.64–0.69 (reviewer). `lanczos_tests::fast_taps_equal_the_direct_kernel` (≤ 1e-9) |
| Checksum re-pin | — | `raf_corrected_default_render_checksum`: `679f717b…f3cc` (ENG-8c bilinear) → `b66ad609b736d817acd529ae8019722b4f21057252cdc30b66ed8e5cacbc5e8c` (Lanczos-3). Unchanged by the fast-weight form |
| DNG opcode warps | not changed | They run in `Embedded::apply`: separate, CPU-only code with CFA-phase sampling (stage 0 samples the mosaic on its phase lattice), bilinear as on main. It is not the same stage machinery as the maker kernel, and no opcode pin moved. Moving them to Lanczos is a follow-up |
| **NS3** the 32 px resident limit vs real Fuji lenses | `CaPlan::halo` (displacement + kernel reach: bilinear 2, maker 4), `MAX_CA_HALO = 384`; `ca_halo`, `demosaic_ca_batch`; `resident_render::ca_fits` in `supports_resident` and the `render_region` / `render_progressive` decisions (`245618ed`) | `pipeline-gpu/tests/maker_lens.rs::gpu_raf_with_wide_zoom_barrels_stays_resident`: −4 % barrel 72 px (halo 76) and −6 % barrel 104 px (halo 108) on the RAF stay resident (`can_render_resident` true; `render_output_metrics` is Some, the resident path), GPU vs CPU **≤ 1 code** at L2 and L3, GPU L2 **0.30 / 0.31 s**. A −40 % barrel (486 px, beyond the limit) reports `can_render_resident` false and renders on the reference chain (1.66 s), within 1 code, without an error (N1) |
| NS3 latent GPU bug found | `pipeline-gpu resident.rs::assemble`: at most 16384 spans per gather dispatch | The gather shader indexes spans by `workgroup_id.x` only, and `record()` folds > 65535 workgroups into a second dimension that the shader ignores. Wide halos clamp many one-pixel runs at the frame edges, so spans were skipped and duplicated: GPU vs CPU 124 (halo 76) and 207 (halo 136) code values before the fix, 1 after. Unreachable with the old 32 px limit (≤ ~32k spans) |
| NS3 export | (band plan unchanged: sensor rows include the halo) | `export gpu::tests::raf_wide_zoom_barrels_export_on_bands_within_budget`: −4 % and −6 %, full chain and both web sizes on the band path, no band over its share (max live+readback 156–172 MiB of 191), full-res vs CPU 1 code; max actual/planned 0.986 over 128 bands |
| N2 GPU test bounds | `pipeline-gpu/tests/maker_lens.rs` | 3 → **1** code value (measured 0–1) |
| NS4 release note | `docs/RELEASE-NOTES.md` | crop shift: about 0.5 % of the width on the X-E2S sample (kit zoom, mid focal length), 1–3 % with wide-angle lenses and the wide end of zooms |

## Timing

Under load 7–12. Fuji AI-mask export (PNG, the legacy fast path, CPU
prefix): **4.8 s** with Lanczos-3, vs 4.4 s bilinear (ENG-8c), 4.3 s on
main (reviewer) and 17.2 s in ENG-8b. The CPU Lanczos uses three
trigonometric calls per axis instead of twelve; the direct form took 5.2 s.
Other fixtures are unchanged (CR3 4.1 s, ARW 3.9 s). Develop L2 on the GPU
with the fixture's own correction is unchanged in kind (resident); −4 % and
−6 % barrels take 0.30–0.31 s.

## Golden and expectation audit (ENG-8d)

- Re-pinned: the corrected-RAF checksum (above), reason NB2.
- Tightened: the GPU/CPU parity bounds in `pipeline-gpu/tests/maker_lens.rs`
  (3 → 1).
- No other pin, golden or tolerance changed. All opcode-warp tests are
  unchanged.

## Gates (ENG-8d)

The full test gate ran on `245618ed` after `cargo clean --release -p raw-decode
-p pipeline-cpu -p image-core -p pipeline-gpu -p export -p previews -p merge
-p pipeline-adobe -p tessera-ffi -p tessera-mcp`, target
`~/.cache/tessera-target/ENG-8`, fixtures symlinked,
`TESSERA_REQUIRE_RAW_FIXTURES=1`. Clippy then failed on `lanczos3_weight`
(used only by the test). `41ae09c2` makes it `#[cfg(test)]`: no behaviour change.
Clippy and fmt were rerun on `41ae09c2`, and so was pipeline-cpu
(258 passed, 0 failed).

| Gate | Result |
|---|---|
| `cargo test --release --workspace --no-fail-fast` (`245618ed`) | pass, exit 0: 701 test binaries, **3617 passed, 0 failed, 102 ignored**. No raw-fixture SKIPPED lines. Load 7.6 at start, 17.3 at end; no wall-clock failures, no reruns |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | first run failed (dead_code, above); pass on `41ae09c2` |
| `cargo fmt --all -- --check` | pass |
| `apps/mac/build-ffi.sh` | pass; no bindings drift (only the docs were modified) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: XCTest 996 tests, 1 skipped, 0 failures; Swift Testing 5 passed |
| strict release `swift build --product Tessera` | pass |
| `five_fixture_device_peak_within_budget` (ignored, run alone, `41ae09c2`) | pass; RAF 326–361 MiB |

## Residual / follow-ups (ENG-8d)

1. DNG opcode warps stay bilinear (main's behaviour, CPU-only `Embedded::apply`).
2. Corrections beyond `MAX_CA_HALO` (384 px, e.g. extreme fisheye-like
   profiles) render on the reference chain per render (correct, slower).
3. R1, R2, R4 and S4 remain open as before.
