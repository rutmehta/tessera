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
| Independent sanity check vs the camera | — | `raf_correction_matches_the_cameras_embedded_jpeg_geometry`: per-tile shifts of the render against the camera's embedded JPEG (1920×1280), after the best global scale + shift: residual **0.382 px** with the correction (30 tiles) vs **1.098 px** without (27 tiles). Bound fixed before the corrected run: < 0.6× uncorrected and < 1 px |
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
3. **Native AI-mask export of a Fujifilm raw** now reports "AI masks with
   lens warps require a hook-aware lens renderer" (`export/src/ai_masks.rs`),
   the existing behaviour for any raw with a lens correction (opcodes,
   profiles). Develop is unaffected. Before ENG-8 such an export rendered
   without the correction. Switching native AI-mask exports to the
   hook-aware `render_develop` path is a separate lane.
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
