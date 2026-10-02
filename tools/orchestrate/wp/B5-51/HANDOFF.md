# B5-51: app Lightroom import stress profile

Local branch `wp/B5-51`. The harness is a macOS-only ignored release unit test in
`crates/tessera-ffi/src/lrcat_profile.rs`, alongside the app bridge. It adds no
production behavior, dependencies, bindings, lockfile, or board changes.

## Reuse

Set `TESSERA_LRCAT_PROFILE` to a scratch catalog copy and `TESSERA_APP_DIR` to an
existing, empty, dedicated temporary directory. The harness rejects other locations.
Do not point either variable at user media or Lightroom-managed storage.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-51
export CARGO_BUILD_JOBS=3
cargo test --release -p tessera-ffi --lib \
  lrcat::lrcat_profile::profile_from_env -- \
  --exact --ignored --nocapture --test-threads=1
```

Run in isolation. The ignored test suppresses dependency stdout/stderr and panic
payloads while importing; only its numeric aggregate JSON is emitted afterwards.
The Rust test runner still prints its own fixed test status. No issue reason,
example, error, image identifier, or catalog string is serialized. Key names come
only from the compiled Adobe `KEY_MAP` allowlist; unlisted retained keys are counted
together without names. Warning categories use a fixed allowlist and an Other bucket.

## Measurement boundaries

- **Open:** `Engine::open` and the app's `Engine::open_lrcat`, including the importer's
  own read-only temporary copy, translation, metadata, summary, and disk spool.
- **Inspect/preview:** `summary`, `plan`, and `fidelity_sample` with the app options.
- **Apply:** the app's `apply`, including retained bundle and library publication.
- **Report:** aggregate report reduction, one-recipe-at-a-time spool audit, and
  read-only source develop-row/key counting. This is a Rust aggregate report phase;
  it does not time Swift Markdown rendering or any UI.

Every root is relocated into a nonexistent child of the fresh app directory, and
all resolved image/folder paths are checked before filesystem probing. Consequently
this measures the app's **missing-originals** flow: no originals are opened, no
rendered fidelity samples, no photo sidecars, and no image indexing. The full source
bundle is still persisted by the real app apply method. This boundary is necessary
to avoid visiting user media or writing beside Lightroom originals. It must not be
presented as full online-photo import performance.

RSS is Darwin `getrusage(RUSAGE_SELF).ru_maxrss` in bytes, the process lifetime
high-water mark sampled after each phase, not an independent resettable phase peak.
Wall times exclude the filesystem byte census. App bytes are logical regular-file
sizes present at phase end; added bytes are the change from phase start, not physical
I/O, allocated blocks, or cumulative write traffic. Importer temporary spool/copies
outside the app directory are excluded. Open includes engine initialization.

Develop rows count physical source rows. Key counts use the last develop row per
image joined to imported image IDs. The **translated** column counts recognized
`CrsKey` source keys on recipes with recorded imported edits, excluding explicit
ignored diagnostics. It is a translation-routing count, not proof that every value
changed a recipe field or matches Adobe rendering. **Approximate** counts recipes
with an explicit approximate diagnostic for that key, once per image/key.
**Retained** counts per-property verbatim source entries, once per image/key.
These columns overlap and include inactive/default settings. The unaudited count
covers cells on decoded images whose source the bounded key audit cannot parse;
undeveloped or failed-import images are excluded from translation-key counting. Unknown retained key
names are never emitted. Group sizes use the exact lengths of the app report's
`unsupported` and `approximate` lists; approximate groups can be zero when all
originals are missing even though source recipes contain approximate diagnostics.

## Single-run aggregates


| Phase | Wall seconds | Peak RSS bytes | App bytes at end | App bytes added |
|---|---:|---:|---:|---:|
| Open | 9.375684 | 631,767,040 | 403,576 | 403,576 |
| Inspect/preview | 0.888016 | 631,767,040 | 403,576 | 0 |
| Apply | 1.176791 | 631,767,040 | 521,368,015 | 520,964,439 |
| Report | 2.914210 | 672,186,368 | 521,368,015 | 0 |

| Aggregate | Count |
|---|---:|
| Images | 21,656 |
| Collections | 18 |
| Collection sets | 3 |
| Smart collections | 3 |
| Keywords | 32 |
| Images with imported edit history | 21,615 |
| Missing originals | 21,656 |
| Fidelity samples | 0 |
| Develop rows | 21,656 |
| Photo sidecars imported | 0 |
| Photos indexed | 0 |
| Skipped photos | 21,656 |
| Not fully supported groups | 151 |
| Approximate translations groups | 0 |
| Suppressed report examples | 140 |
| Unaudited develop rows | 0 |
| Retained key occurrences outside the allowlist | 21,509 |
| Allowlisted Adobe keys | 176 |
| Translated key occurrences | 1,225,118 |
| Approximate key occurrences | 6 |
| Allowlisted retained key occurrences | 593,360 |
| Real profile runs | 1 |
| Catalog content unchanged | 1 |
| Catalog and sibling contents unchanged | 1 |
| Temporary app directory removed | 1 |

| Warning category | Occurrences |
|---|---:|
| Catalog | 13 |
| Develop settings | 576,956 |
| Faces | 9,293 |
| History | 30,460 |
| Smart collections | 2 |
| Stacks | 220 |

Warning occurrences sum the app issue counts, not unique images or report rows.
Source approximation counts cover all recipes; report approximation groups cover
only written/resumed photos. Their different scopes explain the zero report group
size despite nonzero source approximations.

## Comparison with B5-29c

Use the latest follow-up measurements in the earlier handoff. Those were separate
streaming CLI processes; these are consecutive app FFI phases in one process.

| Measurement | B5-29c wall seconds | B5-51 wall seconds | B5-29c peak RSS bytes | B5-51 peak RSS bytes |
|---|---:|---:|---:|---:|
| Inspect versus open plus inspect/preview | 9.780000 | 10.263700 | 536,821,760 | 631,767,040 |
| Apply | 11.130000 | 1.176791 | 647,823,360 | 631,767,040 |

Both catalog profiles contain 21,656 images. Open plus inspect/preview remains
about ten seconds. Whole-run peak RSS remains below 0.8 decimal GB. The lower apply
time is **not a speedup claim**: this run has no available originals and writes no
photo sidecars, while B5-29c wrote the CLI recipe bundle. App output sizes and report
counts also reflect different flows and newer translation code. Single observations
are not a statistical performance comparison. The older historical B5-29c section
recorded 9.81/11.46 seconds and 591,904,768/764,100,608 bytes before its follow-up.

## Verification

- Synthetic release profile passed before the harness commit and real run. It checks
  catalog counts, missing-original behavior, nonzero translated-key counts, app bytes,
  report grouping, output privacy, unknown-category/key suppression, and rejection of
  a nonempty destination.
- `cargo test --release -p import-lrcat -p tessera-ffi`: 688 passed, 31 ignored,
  0 failed; includes the new synthetic test and both existing streaming scale gates.
- `cargo clippy --release --all-targets -p import-lrcat -p tessera-ffi -- -D warnings`:
  passed. Existing vendored C++ deprecation notices are emitted by the LibRaw build.
- `cargo fmt --all --check`: passed.
- Requested seven-pattern privacy diff scan: 0 matches.
- The real release profile ran once, in its own test process after the gates. Source
  catalog and sibling content hashes matched before/after; no hashes or source names
  are recorded here. The entire task-owned app directory was deleted and its absence
  verified after the run. Importer-owned temporary files are dropped with the import.
- No app launch, push, dependency, lockfile, or board change.
- Harness commit: `8750280f` (`test(B5-51):`). This handoff is the `docs(B5-51):` commit.

## Adobe-key aggregates

| Adobe key | Translated | Approximate | Retained |
|---|---:|---:|---:|
| AutoLateralCA | 21,615 | 0 | 0 |
| AutoTone | 0 | 0 | 27 |
| AutoToneDigest | 0 | 0 | 1,477 |
| AutoToneDigestNoSat | 0 | 0 | 1,477 |
| AutoWhiteVersion | 0 | 0 | 52 |
| Blacks2012 | 21,615 | 0 | 0 |
| Brightness | 0 | 0 | 21,239 |
| CameraProfile | 21,615 | 0 | 0 |
| CameraProfileDigest | 21,615 | 0 | 0 |
| Clarity2012 | 686 | 0 | 0 |
| ColorGradeBlending | 21,615 | 0 | 0 |
| ColorGradeGlobalHue | 21,615 | 0 | 0 |
| ColorGradeGlobalLum | 21,615 | 0 | 0 |
| ColorGradeGlobalSat | 21,615 | 0 | 0 |
| ColorGradeHighlightLum | 21,615 | 0 | 0 |
| ColorGradeMidtoneHue | 21,615 | 0 | 0 |
| ColorGradeMidtoneLum | 21,615 | 0 | 0 |
| ColorGradeMidtoneSat | 21,615 | 0 | 0 |
| ColorGradeShadowLum | 21,615 | 0 | 0 |
| ColorNoiseReduction | 19,562 | 0 | 0 |
| ColorNoiseReductionDetail | 9 | 0 | 0 |
| CompatibleVersion | 0 | 0 | 1,245 |
| Contrast | 0 | 0 | 21,239 |
| Contrast2012 | 21,615 | 0 | 0 |
| ConvertToGrayscale | 0 | 0 | 21,615 |
| CropAngle | 177 | 0 | 0 |
| CropBottom | 808 | 0 | 0 |
| CropLeft | 189 | 0 | 0 |
| CropRight | 215 | 0 | 0 |
| CropTop | 834 | 0 | 0 |
| CurveRefineSaturation | 0 | 0 | 18,248 |
| DefringeGreenAmount | 21,615 | 0 | 0 |
| DefringeGreenHueHi | 21,615 | 0 | 0 |
| DefringeGreenHueLo | 21,615 | 0 | 0 |
| DefringePurpleAmount | 21,615 | 0 | 0 |
| DefringePurpleHueHi | 21,615 | 0 | 0 |
| DefringePurpleHueLo | 21,615 | 0 | 0 |
| Dehaze | 506 | 0 | 0 |
| DepthBasedCorrections | 0 | 0 | 16 |
| DepthMapInfo | 0 | 0 | 417 |
| Exposure | 0 | 0 | 21,239 |
| Exposure2012 | 21,615 | 0 | 0 |
| FillLight | 0 | 0 | 27 |
| GrainAmount | 10 | 0 | 0 |
| GrainFrequency | 7 | 0 | 0 |
| GrainSeed | 0 | 0 | 10 |
| GrainSize | 21,615 | 0 | 0 |
| GrayMixerAqua | 0 | 0 | 1 |
| GrayMixerBlue | 0 | 0 | 1 |
| GrayMixerGreen | 0 | 0 | 1 |
| GrayMixerMagenta | 0 | 0 | 1 |
| GrayMixerOrange | 0 | 0 | 1 |
| GrayMixerPurple | 0 | 0 | 1 |
| GrayMixerRed | 0 | 0 | 1 |
| GrayMixerYellow | 0 | 0 | 1 |
| HDREditMode | 17,133 | 0 | 0 |
| HDRMaxValue | 17,133 | 0 | 0 |
| HighlightRecovery | 0 | 0 | 27 |
| Highlights2012 | 21,615 | 0 | 0 |
| HueAdjustmentAqua | 152 | 0 | 0 |
| HueAdjustmentBlue | 148 | 0 | 0 |
| HueAdjustmentGreen | 154 | 0 | 0 |
| HueAdjustmentMagenta | 21 | 0 | 0 |
| HueAdjustmentOrange | 57 | 0 | 0 |
| HueAdjustmentPurple | 136 | 0 | 0 |
| HueAdjustmentRed | 163 | 0 | 0 |
| HueAdjustmentYellow | 215 | 0 | 0 |
| IncrementalTemperature | 0 | 0 | 1,966 |
| IncrementalTint | 0 | 0 | 1,966 |
| LensBlur | 17,133 | 0 | 17,133 |
| LensManualDistortionAmount | 21,615 | 0 | 0 |
| LensProfileDigest | 12,042 | 0 | 0 |
| LensProfileDistortionScale | 12,042 | 0 | 0 |
| LensProfileEnable | 21,615 | 0 | 0 |
| LensProfileFilename | 12,042 | 0 | 0 |
| LensProfileIsEmbedded | 0 | 0 | 12,042 |
| LensProfileName | 12,042 | 0 | 0 |
| LensProfileSetup | 21,615 | 0 | 0 |
| LensProfileVignettingScale | 12,042 | 0 | 0 |
| Look | 17,032 | 0 | 0 |
| LuminanceAdjustmentAqua | 145 | 0 | 0 |
| LuminanceAdjustmentBlue | 154 | 0 | 0 |
| LuminanceAdjustmentGreen | 156 | 0 | 0 |
| LuminanceAdjustmentMagenta | 135 | 0 | 0 |
| LuminanceAdjustmentOrange | 170 | 0 | 0 |
| LuminanceAdjustmentPurple | 135 | 0 | 0 |
| LuminanceAdjustmentRed | 150 | 0 | 0 |
| LuminanceAdjustmentYellow | 186 | 0 | 0 |
| LuminanceNoiseReductionContrast | 21,615 | 0 | 0 |
| LuminanceNoiseReductionDetail | 11 | 0 | 0 |
| LuminanceSmoothing | 526 | 0 | 0 |
| MaskGroupBasedCorrections | 697 | 0 | 697 |
| OverrideLookVignette | 0 | 0 | 21,615 |
| ParametricDarks | 14 | 0 | 0 |
| ParametricHighlightSplit | 118 | 0 | 0 |
| ParametricHighlights | 8 | 0 | 0 |
| ParametricLights | 5 | 0 | 0 |
| ParametricMidtoneSplit | 126 | 0 | 0 |
| ParametricShadowSplit | 126 | 0 | 0 |
| ParametricShadows | 2 | 0 | 0 |
| PerspectiveHorizontal | 21,615 | 0 | 0 |
| PerspectiveRotate | 21,615 | 0 | 0 |
| PerspectiveScale | 21,615 | 0 | 0 |
| PerspectiveUpright | 1 | 0 | 0 |
| PerspectiveVertical | 21,615 | 0 | 0 |
| PerspectiveX | 21,615 | 0 | 0 |
| PerspectiveY | 21,615 | 0 | 0 |
| PointColors | 17,132 | 0 | 17,132 |
| PostCropVignetteAmount | 415 | 0 | 0 |
| PostCropVignetteFeather | 11 | 0 | 0 |
| PostCropVignetteHighlightContrast | 1 | 0 | 0 |
| PostCropVignetteMidpoint | 58 | 0 | 0 |
| PostCropVignetteRoundness | 44 | 0 | 0 |
| PostCropVignetteStyle | 1 | 0 | 0 |
| ProcessVersion | 21,615 | 0 | 0 |
| RangeMaskMapInfo | 0 | 0 | 8 |
| RedEyeInfo | 0 | 0 | 21,615 |
| RetouchAreas | 330 | 0 | 330 |
| RetouchInfo | 11,543 | 0 | 11,543 |
| SDRBlend | 0 | 0 | 17,133 |
| SDRBrightness | 0 | 0 | 17,133 |
| SDRClarity | 0 | 0 | 17,133 |
| SDRContrast | 0 | 0 | 17,133 |
| SDRHighlights | 0 | 0 | 17,133 |
| SDRShadows | 0 | 0 | 17,133 |
| SDRWhites | 0 | 0 | 17,133 |
| Saturation | 21,615 | 0 | 0 |
| SaturationAdjustmentAqua | 150 | 0 | 0 |
| SaturationAdjustmentBlue | 157 | 0 | 0 |
| SaturationAdjustmentGreen | 162 | 0 | 0 |
| SaturationAdjustmentMagenta | 401 | 0 | 0 |
| SaturationAdjustmentOrange | 196 | 0 | 0 |
| SaturationAdjustmentPurple | 166 | 0 | 0 |
| SaturationAdjustmentRed | 189 | 0 | 0 |
| SaturationAdjustmentYellow | 468 | 0 | 0 |
| Shadows | 0 | 0 | 21,239 |
| Shadows2012 | 21,615 | 0 | 0 |
| SharpenDetail | 21,615 | 0 | 0 |
| SharpenEdgeMasking | 21,615 | 0 | 0 |
| SharpenRadius | 21,615 | 0 | 0 |
| Sharpness | 19,648 | 0 | 0 |
| SplitToningBalance | 19 | 0 | 0 |
| SplitToningHighlightHue | 28 | 0 | 0 |
| SplitToningHighlightSaturation | 26 | 0 | 0 |
| SplitToningShadowHue | 69 | 0 | 0 |
| SplitToningShadowSaturation | 62 | 0 | 0 |
| Temperature | 11,663 | 0 | 0 |
| Texture | 671 | 0 | 0 |
| Tint | 11,663 | 0 | 0 |
| ToggleStyleAmount | 0 | 0 | 4 |
| ToggleStyleDigest | 0 | 0 | 4 |
| ToneCurveName2012 | 0 | 0 | 21,615 |
| ToneCurvePV2012 | 21,615 | 0 | 0 |
| ToneCurvePV2012Blue | 21,615 | 0 | 0 |
| ToneCurvePV2012Green | 21,615 | 0 | 0 |
| ToneCurvePV2012Red | 21,615 | 0 | 0 |
| UprightCenterMode | 0 | 1 | 21,615 |
| UprightCenterNormX | 0 | 1 | 21,615 |
| UprightCenterNormY | 0 | 1 | 21,615 |
| UprightDependentDigest | 0 | 0 | 1 |
| UprightFocalLength35mm | 0 | 1 | 21,615 |
| UprightFocalMode | 0 | 1 | 21,615 |
| UprightFourSegmentsCount | 0 | 0 | 21,615 |
| UprightPreview | 0 | 0 | 21,615 |
| UprightTransformCount | 0 | 0 | 21,615 |
| UprightTransform_0 | 0 | 0 | 1 |
| UprightTransform_1 | 0 | 1 | 1 |
| UprightTransform_2 | 0 | 0 | 1 |
| UprightTransform_3 | 0 | 0 | 1 |
| UprightTransform_4 | 0 | 0 | 1 |
| UprightTransform_5 | 0 | 0 | 1 |
| UprightVersion | 0 | 0 | 21,615 |
| Version | 0 | 0 | 21,615 |
| Vibrance | 21,615 | 0 | 0 |
| WhiteBalance | 21,615 | 0 | 0 |
| Whites2012 | 21,615 | 0 | 0 |
