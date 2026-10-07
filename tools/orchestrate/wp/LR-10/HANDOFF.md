# LR-10: Adobe default rendering and LR-8c compatibility

## Status

Implementation, the 12-pair measurement, and all final clean release
Rust/Clippy/Swift gates are complete and passing. The measurement target is
partially met: eleven pairs improve, while pair 12 regresses slightly.
No app was installed or launched. Local commits only; no board.json change.
No private pixels, source names/paths, or private-content hashes are in commits.

## Integration rebase

The pre-existing integration at `5f7ff0ab` rebased `840e2c88` onto
`origin/wp/LR-9-default-noise` (`46b1bf54`) without squashing.
Conflicts were reconciled as follows:

- `crates/tessera-ffi/src/lrcat_profile.rs`: retained LR-9 audit keys,
  classifications and no-op policy accounting.
- `crates/tessera-ffi/src/lrcat.rs`: retained combined mask/depth resolution,
  rollback, and LR-8 OfflineProxy admission.
- `crates/pipeline-cpu/src/lib.rs`: unioned local-adjustment and context-aware
  render exports.
- `crates/import-lrcat/tests/golden.rs`: retained the upstream expected digest.
- The LR-3 synthetic metadata initializer received neutral defaults for LR-8's
  catalog_orientation and baseline_exposure fields.

The schema predicate union, matrix rows, LR-9 policy table, and LR-7 single Import
history entry are retained. The only Cargo.lock differences against the rebase
base remain the two approved raw-decode edges: jxl-oxide and zune-jpeg 0.5.15.

## LR-8c: ordinary import boundary

Tests first: `6c2dace3` and `6e6c7286`; fixes: `660ff7db` and `548e2434`.
Catalog orientation is no longer inserted into every serialized imported recipe.
It is retained only when applying an offline-proxy outcome, keeping proxy owners
upright after relinking. Ordinary originals continue to use main's decoder
orientation and resource extents. A non-square combined mask/depth regression
proved that an ordinary original's catalog rotation must not swap its resource
extent. Both mask and depth promotion remain intact.

The two required import goldens passed without changing their expected bytes.
No LR-8b golden re-pin was present relative to the rebase base. The new test
compares complete serialized import plans with and without a neighboring Smart
Previews bundle, and checks both against the original import digest. Existing
all-eight-orientation proxy/relink/export tests pass. The regenerated Swift diff
was whitespace-only and was discarded, including after final generation.

## LR-10 implementation

- Bounded TIFF/DNG profile extraction supports IFD0 inheritance and camera
  SubIFDs, both byte orders, CFA originals and external LinearRaw DNG sources.
  It does not read strips/tiles. Synthetic tests reject cycles and prohibit pixel
  reads during metadata extraction.
- Explicitly resolved external DCPs win. Imported Adobe recipes and recipes
  naming Adobe profiles otherwise use embedded profiles. Native rendering stays
  on the existing operator path. This fallback is not the missing Adobe binary
  profile; no Adobe Color/Standard files are bundled.
- White balance uses camera neutral before camera-to-ProPhoto conversion and
  HueSatMap. ForwardMatrix and ColorMatrix-inverse paths are tested. Dual tables
  interpolate with reciprocal temperature. Native preprocessing baseline gain
  is undone, then baseline + profile offset + user exposure is applied once,
  between HueSatMap and LookTable.
- HSV trilinear interpolation, hue wrap, and sRGB V encoding have independent
  hand-computed tests. Profile tone follows the LookTable, using hue-preserving
  channel-extrema interpolation. The absent scene-referred tone curve uses all
  1,025 published ACR3 samples, with linear interpolation.
- Output-referred DNGs (ColorimetricReference=1) use identity when their curve is
  absent and perform no automatic shadow subtraction, as in the public SDK.
  Unknown calibration illuminants use the first calibration. This distinction
  was exposed by measurement and verified with synthetic regression tests.
- The contact harness now reports signed mean RGB delta and can recompute metrics
  from captured contacts without changing pixels or recipes.

### Exact versus approximate

See [DCP.md](../../../../crates/pipeline-adobe/DCP.md) for the detailed contract.
The tags, stage order, table encoding/interpolation, exposure addition, public
ACR3 values, and output-referred/None black policies are explicit implementations.
Auto black uses the SDK ramp shape with fixed shadows=5 and unit
ShadowScale/Stage3Gain. It is approximate, not Lightroom's image-dependent
shadows heuristic, and does not add the SDK's separate negative-exposure shoulder.
As-shot neutral is direct; its CCT interpolation uses a bounded nearest-locus
estimate rather than SDK Robertson. Custom tint retains Tessera's Duv convention.
Existing basic tone/detail/demosaic/optics and imported-operator approximations
remain. Nonidentity camera calibration/analog-balance signature matching, custom
spectral illuminants, and HDR profile extensions are not added. No pair-specific
fitting or visual tuning was performed.

### Public specification and SDK citations

- [DNG 1.7.1 specification](https://helpx.adobe.com/content/dam/help/en/photoshop/pdf/DNG_Spec_1_7_1_0.pdf):
  profile tags pp. 49–57; black/exposure/encoding pp. 62–65; transforms/tables
  pp. 100–104; ColorimetricReference pp. 46–47.
- [dng_render.cpp](https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_render.cpp):
  default ACR3 table, exposure ramp, output-referred defaults.
- [dng_reference.cpp](https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_reference.cpp):
  RefBaselineRGBTone hue-preserving interpolation.
- [dng_color_spec.cpp](https://android.googlesource.com/platform/external/dng_sdk/+/de700ad461e35af50b28b861943a0b0753b10929/source/dng_color_spec.cpp):
  normalized camera neutral and first-calibration fallback.

## Measurements

Same 12 deterministic private pairs and existing 64×64 Triangle comparison.
Luminance uses encoded 8-bit RGB coefficients 0.2126/0.7152/0.0722. RGB deltas are
signed Tessera-minus-Lightroom per-channel means, also in 8-bit units. Reference
JPEGs pass through the existing preview color-management decoder. Outputs remain
only in the coordinator-approved scratch directory.

| Pair | Luminance MAD before | After | Mean RGB delta before | After |
|---:|---:|---:|---|---|
| 1 | 31.889 | 12.485 | (-28.132, -33.618, -25.798) | (-7.360, -11.035, -6.668) |
| 2 | 10.996 | 8.414 | (-4.273, -6.674, -4.049) | (-6.306, -8.534, -5.235) |
| 3 | 31.302 | 8.688 | (-32.634, -31.237, -22.287) | (-5.196, -6.594, 1.276) |
| 4 | 48.191 | 31.660 | (-44.089, -48.606, -56.157) | (-29.674, -31.767, -36.449) |
| 5 | 12.399 | 7.149 | (0.214, 0.711, 0.426) | (-6.390, -5.281, -5.135) |
| 6 | 35.687 | 20.278 | (-31.161, -36.014, -33.898) | (-12.145, -16.797, -14.967) |
| 7 | 21.128 | 19.112 | (-23.287, -17.132, -12.302) | (-2.630, 0.075, 3.048) |
| 8 | 40.230 | 14.844 | (-6.514, -48.010, -62.446) | (-0.249, -17.913, -27.170) |
| 9 | 39.126 | 15.748 | (-18.983, -43.815, -51.988) | (-9.170, -16.966, -21.970) |
| 10 | 32.582 | 17.956 | (-30.577, -30.854, -29.956) | (-9.430, -10.619, -9.826) |
| 11 | 47.489 | 27.812 | (-46.539, -47.837, -46.338) | (-25.674, -27.645, -26.482) |
| 12 | 47.894 | 49.074 | (-27.575, -31.123, -32.340) | (-17.154, -20.227, -20.585) |

Mean luminance MAD: **33.243 → 19.435** (41.54% lower). Eleven of twelve pairs
improved. Pair 12 regressed by 1.180; its orientation/aspect comparison flag was
already false before the change and remains false. All other pairs have that
flag true in both runs. The strict no-regression target is therefore not claimed
for all twelve pairs. No registration change or pair exclusion was used to
improve the reported result.

## Gates

Initial LR-8c full gate: 2,029 passed, zero failed, 69 suite-declared ignored;
workspace Clippy and fmt passed; SWIFT GATE OK (920 XCTest, 3 skipped, 0 failures;
5 Swift Testing tests); strict release build passed. The subsequent resource-
extent regression passed after its additional fix.

LR-10 focused validation: pipeline-adobe suite, synthetic embedded CFA/LinearRaw
and named-profile dispatch, staged/standalone renderer agreement, Develop
admission and signed measurement tests passed. Final clean release Rust gate: 2,046 passed, zero failed, 70 suite-declared
ignored across all 13 requested crates, with no command-level exclusions.
Workspace Clippy (`--all-targets -- -D warnings`) and fmt passed.
UniFFI generation passed; its whitespace-only Swift change was discarded.
SWIFT GATE OK: 920 XCTest tests, 3 skipped, zero failures; all 5 Swift Testing
tests passed. Strict release build with complete concurrency and warnings as
errors passed (135.93 seconds). The existing linker warning about the BLAKE3
object macOS 26.2 deployment version versus the macOS 15.0 link target remains;
there were no Swift compiler warnings or errors.

No Develop pixel golden or existing import-golden expectation was changed.
The repository's RAW fixtures are present, including its DNG; the native pixel
check is not relying on the test's missing-fixture skip.

## A-owned engine files touched in this continuation

All engine files changed on top of `5f7ff0ab`, including tests and documentation:

- `crates/image-core/src/adobe.rs`
- `crates/image-core/src/render.rs`
- `crates/image-core/src/source.rs`
- `crates/image-core/tests/embedded_adobe.rs`
- `crates/import-lrcat/src/lib.rs`
- `crates/import-lrcat/tests/golden.rs`
- `crates/import-lrcat/tests/orientation.rs`
- `crates/pipeline-adobe/DCP.md`
- `crates/pipeline-adobe/src/dcp.rs`
- `crates/pipeline-adobe/src/dcp_acr3.rs`
- `crates/pipeline-adobe/src/dcp_embedded.rs`
- `crates/pipeline-adobe/src/lib.rs`
- `crates/pipeline-adobe/src/render.rs`
- `crates/pipeline-adobe/tests/dcp_render.rs`
- `crates/pipeline-adobe/tests/embedded.rs`
- `crates/tessera-ffi/src/develop.rs`
- `crates/tessera-ffi/src/lrcat.rs`
- `crates/tessera-ffi/src/lrcat_combined_tests.rs`
- `crates/tessera-ffi/src/lrcat_profile.rs`

The integration rebase also reconciled the A-owned pipeline-cpu export union and
FFI LR-3 metadata initializer listed above; those were already in `5f7ff0ab`.
No raw-decode production file changed in this continuation.
