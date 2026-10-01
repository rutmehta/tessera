# Lightroom develop translation matrix

Source-only inventory at `87ff1ff173e4d6d2053a534b7cfd9343aef906e1` (parent
`e6c3e5da`). All examples are invented. This is an implementation plan, not a
render-parity claim. No catalog or image data was used.

## Reading the table

Paths refer to serialized `Recipe` in `crates/engine-api/src/recipe/`.
A candidate path does not imply semantic equivalence. `MISSING:` identifies a
field or operator the existing recipe cannot express. Lane assignments for
unnamed residual features are LR-2 for tone/color and LR-7 for geometry/metadata;
the coordinator should review these assignments before implementation.

- `retained`: exact source is kept, including partially decoded structures and
  inactive values. It does not mean nothing renders.
- `unsupported-diagnostic`: unsupported source yields a diagnostic and is kept.
  Diagnostics depend on shape/value: nil Lua values may skip codec warnings;
  identity extended curves and their names are retained without warnings.
- `translated`: the valid synthetic example populates an existing field without
  per-key retained source/diagnostics. This does not assert pixel parity for Adobe.
- `approximate`: the valid synthetic example populates the recipe path (its value
  differs from an empty row's), the exact source stays in
  `recipe.unknown["lrcat_develop_source"]`, at least one info-level entry for the
  key is recorded through `import_lrcat::diagnostics::push_approximate` (stored in
  `recipe.unknown["lrcat_translation_diagnostics"]`, read with `entries()`), and
  the import has zero warnings. The in-app import report lists these keys in a
  separate "Approximate translations" group. Promote to `translated` only with
  evidence from an Adobe-rendered synthetic chart or a public DNG+XMP.

The table enumerates every unmapped KEY_MAP member, the five pending structures,
all named extended curves, legacy CRS CA keys, brief-named families and nested
mask diagnostic cases. Wildcards cover open-ended property families, not invented
finite enumerations. Slash-qualified rows are nested contexts, not top-level Lua
keys. Other recognized KEY_MAP entries already translate (or feed AUX provenance);
the generic invalid/duplicate diagnostics below still apply to every one of them.

The fifth column is a literal synthetic Lua value, mandatory for `translated`
and `approximate` rows. The cheap `translation_matrix` integration test imports each such row via
`lua_develop::parse`, checks retention and diagnostics, and verifies the recipe
JSON pointer exists. It also checks inventory coverage against KEY_MAP and named
extended curves. A negative control proves that falsely claiming PointColors is
translated fails. Test-only fixture rows prove the `approximate` checks, one
negative per condition (field, source, diagnostic, warnings); no real row is
`approximate` until a lane converts. Add representative structured fixtures when promoting a
structure; a scalar or empty payload is not proof of full structure coverage.

| Adobe key | Existing recipe path or missing field | Lane | Status | Synthetic Lua value |
| --- | --- | --- | --- | --- |
| `*` | MISSING: unknown future Adobe property; classify before mapping | LR-7 | retained | — |
| `AutoTone` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoToneDigest` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoToneDigest*` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoToneDigestNoSat` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoWhiteVersion` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `Brightness` | MISSING: PV2010 brightness operator; /settings/tone/curves/rgb only an approximation | LR-2 | unsupported-diagnostic | — |
| `ChromaticAberrationB` | `/settings/lens/legacy_ca_blue` | LR-7 | translated | `-25` |
| `ChromaticAberrationR` | `/settings/lens/legacy_ca_red` | LR-7 | translated | `35` |
| `Clarity` | /settings/tone/clarity (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `CompatibleVersion` | MISSING: Adobe application compatibility metadata (not /process_version) | LR-7 | unsupported-diagnostic | — |
| `Contrast` | /settings/tone/contrast (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `ConvertToGrayscale` | MISSING: monochrome mode in /settings/color (saturation is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `CurveRefineSaturation` | MISSING: tone-curve saturation refinement | LR-2 | unsupported-diagnostic | — |
| `DepthBasedCorrections` | /settings/locals/adjustments (MaskKind::Depth) | LR-4 | unsupported-diagnostic | — |
| `DepthMapInfo` | MISSING: imported depth resource reference/calibration; LensBlur.depth_model identifies a model | LR-2 | unsupported-diagnostic | — |
| `EnableDistractionRemoval` | MISSING: cloud removal result/resource and execution semantics | LR-7 | unsupported-diagnostic | — |
| `Exposure` | /settings/tone/exposure (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `ExtendedToneCurveName2012` | MISSING: HDR-domain curve/name; /settings/tone/curves is normalized SDR | LR-2 | retained | — |
| `ExtendedToneCurvePV2012` | MISSING: HDR-domain curve/name; /settings/tone/curves is normalized SDR | LR-2 | unsupported-diagnostic | — |
| `ExtendedToneCurvePV2012Blue` | MISSING: HDR-domain curve/name; /settings/tone/curves is normalized SDR | LR-2 | unsupported-diagnostic | — |
| `ExtendedToneCurvePV2012Green` | MISSING: HDR-domain curve/name; /settings/tone/curves is normalized SDR | LR-2 | unsupported-diagnostic | — |
| `ExtendedToneCurvePV2012Red` | MISSING: HDR-domain curve/name; /settings/tone/curves is normalized SDR | LR-2 | unsupported-diagnostic | — |
| `FillLight` | /settings/tone/shadows (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `GrainSeed` | MISSING: explicit seed in /settings/effects/grain | LR-2 | unsupported-diagnostic | — |
| `GrayMixerAqua` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `GrayMixerBlue` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `GrayMixerGreen` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `GrayMixerMagenta` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `GrayMixerOrange` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `GrayMixerPurple` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `GrayMixerRed` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `GrayMixerYellow` | MISSING: monochrome channel mixer in /settings/color (HSL luminance is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `HighlightRecovery` | /settings/tone/highlights (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `IncrementalTemperature` | MISSING: relative white-balance delta; /settings/white_balance uses absolute controls | LR-2 | unsupported-diagnostic | — |
| `IncrementalTint` | MISSING: relative white-balance delta; /settings/white_balance uses absolute controls | LR-2 | unsupported-diagnostic | — |
| `LensBlur` | `/settings/effects/lens_blur` | LR-6 | retained | — |
| `LensProfileIsEmbedded` | MISSING: embedded-profile/Look-vignette override semantics in /settings/lens | LR-7 | unsupported-diagnostic | — |
| `MaskGroupBasedCorrections` | `/settings/locals/adjustments` | LR-4 | retained | — |
| `MaskGroupBasedCorrections/CorrectionRangeMask` | /settings/locals/adjustments (LuminanceRange/ColorRange/Depth) | LR-4 | retained | — |
| `MaskGroupBasedCorrections/Flipped` | /settings/locals/adjustments (MaskKind::Radial and MaskComponent.invert; semantics unresolved) | LR-4 | retained | — |
| `MaskGroupBasedCorrections/Mask/Background` | /settings/locals/adjustments (AI kinds; category only, raster fidelity missing) | LR-5 | retained | — |
| `MaskGroupBasedCorrections/Mask/CircularGradient` | /settings/locals/adjustments (Radial) | LR-4 | retained | — |
| `MaskGroupBasedCorrections/Mask/Gradient` | /settings/locals/adjustments (Linear) | LR-4 | retained | — |
| `MaskGroupBasedCorrections/Mask/Image` | MISSING: raster-backed MaskKind and resource reference | LR-5 | unsupported-diagnostic | — |
| `MaskGroupBasedCorrections/Mask/Object` | /settings/locals/adjustments (AI kinds; category only, raster fidelity missing) | LR-5 | retained | — |
| `MaskGroupBasedCorrections/Mask/Paint` | /settings/locals/adjustments (Brush) | LR-4 | retained | — |
| `MaskGroupBasedCorrections/Mask/People` | /settings/locals/adjustments (AI kinds; category only, raster fidelity missing) | LR-5 | retained | — |
| `MaskGroupBasedCorrections/Mask/Sky` | /settings/locals/adjustments (AI kinds; category only, raster fidelity missing) | LR-5 | retained | — |
| `MaskGroupBasedCorrections/Mask/Subject` | /settings/locals/adjustments (AI kinds; category only, raster fidelity missing) | LR-5 | retained | — |
| `MaskGroupBasedCorrections/MaskActive` | MISSING: per-component enabled flag (group enabled exists) | LR-4 | unsupported-diagnostic | — |
| `MaskGroupBasedCorrections/MaskBlendMode` | /settings/locals/adjustments (MaskComponent.combine) | LR-4 | retained | — |
| `MaskGroupBasedCorrections/Masks` | MISSING: nested group tree (components are flat) | LR-4 | unsupported-diagnostic | — |
| `OverrideLookVignette` | MISSING: embedded-profile/Look-vignette override semantics in /settings/lens | LR-7 | unsupported-diagnostic | — |
| `PerspectiveAspect` | `/settings/geometry/transform/aspect` | LR-7 | translated | `1` |
| `PerspectiveHorizontal` | `/settings/geometry/transform/horizontal` | LR-7 | translated | `1` |
| `PerspectiveRotate` | `/settings/geometry/transform/rotate` | LR-7 | translated | `1` |
| `PerspectiveScale` | `/settings/geometry/transform/scale` | LR-7 | translated | `105` |
| `PerspectiveUpright` | `/settings/geometry/upright/mode` | LR-7 | translated | `1` |
| `PerspectiveVertical` | `/settings/geometry/transform/vertical` | LR-7 | translated | `1` |
| `PerspectiveX` | `/settings/geometry/transform/offset_x` | LR-7 | translated | `1` |
| `PerspectiveY` | `/settings/geometry/transform/offset_y` | LR-7 | translated | `1` |
| `PointColors` | `/settings/color/point_colors` | LR-1 | retained | — |
| `RangeMaskMapInfo` | MISSING: Adobe range-mask resource mapping; candidate /settings/locals/adjustments | LR-4 | unsupported-diagnostic | — |
| `RedEyeInfo` | MISSING: red-eye correction operator in /settings/locals/retouch | LR-3 | unsupported-diagnostic | — |
| `RetouchAreas` | `/settings/locals/retouch` | LR-3 | retained | — |
| `RetouchInfo` | `/settings/locals/retouch` | LR-3 | retained | — |
| `SDRBlend` | MISSING: separate SDR rendition controls alongside /settings/output/hdr | LR-2 | unsupported-diagnostic | — |
| `SDRBrightness` | MISSING: separate SDR rendition controls alongside /settings/output/hdr | LR-2 | unsupported-diagnostic | — |
| `SDRClarity` | MISSING: separate SDR rendition controls alongside /settings/output/hdr | LR-2 | unsupported-diagnostic | — |
| `SDRContrast` | MISSING: separate SDR rendition controls alongside /settings/output/hdr | LR-2 | unsupported-diagnostic | — |
| `SDRHighlights` | MISSING: separate SDR rendition controls alongside /settings/output/hdr | LR-2 | unsupported-diagnostic | — |
| `SDRShadows` | MISSING: separate SDR rendition controls alongside /settings/output/hdr | LR-2 | unsupported-diagnostic | — |
| `SDRWhites` | MISSING: separate SDR rendition controls alongside /settings/output/hdr | LR-2 | unsupported-diagnostic | — |
| `Shadows` | /settings/tone/blacks (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `ToggleStyleAmount` | MISSING: style toggle state/digest; candidate /settings/camera_profile/look/amount | LR-2 | unsupported-diagnostic | — |
| `ToggleStyleDigest` | MISSING: style toggle state/digest; candidate /settings/camera_profile/look/amount | LR-2 | unsupported-diagnostic | — |
| `ToneCurve` | MISSING: legacy curve process semantics/name; candidate /settings/tone/curves | LR-2 | unsupported-diagnostic | — |
| `ToneCurveBlue` | MISSING: legacy curve process semantics/name; candidate /settings/tone/curves | LR-2 | unsupported-diagnostic | — |
| `ToneCurveGreen` | MISSING: legacy curve process semantics/name; candidate /settings/tone/curves | LR-2 | unsupported-diagnostic | — |
| `ToneCurveName` | MISSING: legacy curve process semantics/name; candidate /settings/tone/curves | LR-2 | unsupported-diagnostic | — |
| `ToneCurveName2012` | MISSING: curve preset name; points belong in /settings/tone/curves | LR-2 | unsupported-diagnostic | — |
| `ToneCurveRed` | MISSING: legacy curve process semantics/name; candidate /settings/tone/curves | LR-2 | unsupported-diagnostic | — |
| `Upright*` | MISSING: arbitrary Upright family members/solve state in /settings/geometry | LR-7 | retained | — |
| `UprightCenterMode` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightCenterNormX` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightCenterNormY` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightDependentDigest` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightFocalLength35mm` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightFocalMode` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightFourSegments*` | MISSING: arbitrary Upright family members/solve state in /settings/geometry | LR-7 | retained | — |
| `UprightFourSegmentsCount` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightFourSegments_0` | /settings/geometry/upright/guides (endpoint conversion required) | LR-7 | unsupported-diagnostic | — |
| `UprightFourSegments_1` | /settings/geometry/upright/guides (endpoint conversion required) | LR-7 | unsupported-diagnostic | — |
| `UprightFourSegments_2` | /settings/geometry/upright/guides (endpoint conversion required) | LR-7 | unsupported-diagnostic | — |
| `UprightFourSegments_3` | /settings/geometry/upright/guides (endpoint conversion required) | LR-7 | unsupported-diagnostic | — |
| `UprightPreview` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform*` | MISSING: arbitrary Upright family members/solve state in /settings/geometry | LR-7 | retained | — |
| `UprightTransformCount` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform_0` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform_1` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform_2` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform_3` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform_4` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform_5` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightVersion` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `Version` | MISSING: Adobe application compatibility metadata (not /process_version) | LR-7 | unsupported-diagnostic | — |

## Per-lane representation notes

- **LR-1:** `PointColor` has source LCH, hue/saturation/luminance shifts and one
  range scalar. It has no independent hue/saturation/luminance range bounds or
  feather field. Native round-trip support is not an Adobe PointColors decoder.
- **LR-2:** `ToneCurves` contains normalized rgb/red/green/blue/luminance curves
  and parametric controls. It lacks an extended HDR domain and curve names.
  PV2010 exposure/contrast/clarity and recovery/fill/black controls cannot be
  declared equivalent to current sliders without conversion and render tests.
  There is no monochrome mixer, legacy brightness operator, auto-analysis digest,
  imported depth payload or independent SDR rendition settings. `DepthMapInfo`
  needs coordination with LR-5/6; it is not a tone curve.
- **LR-3:** `RetouchOperation` supports heal/clone offsets, remove/skin, opacity,
  feather, enabled and targets made from mask components or mask IDs. Brush
  strokes carry pressure/radius/flow/erase. This can express spots/strokes once
  Adobe encoding and coordinate conventions are decoded. No dedicated red-eye
  operator or Adobe cloud-generated patch/resource is represented.
- **LR-4:** `LocalAdjustment` has group enabled/amount/invert/local params;
  `MaskComponent` has add/subtract/intersect and invert. Linear/radial/brush,
  luminance/color/depth range types exist. Components are flat: no nested group
  tree or per-component enabled flag. Group/nested range constraints, Flipped,
  unknown blend modes and Adobe paint encodings are currently diagnostic cases.
  Some gradient/radial groups already decode; unconditional source retention
  keeps the whole parent in `retained` until fidelity and the contract change.
- **LR-5:** Subject/sky/background/person/object/landscape/depth categories and
  model references exist. `MaskKind` has no imported raster variant/resource
  handle, and the recipe has no dedicated regenerated-mask diagnostic field.
  Category decoding alone does not import Adobe Mask/Image pixels. Resource
  discovery and mask-store integration remain downstream, using synthetic data.
- **LR-6:** `LensBlur` has amount, focus_range, bokeh string and depth_model.
  It lacks an imported depth-map handle/calibration, detailed Adobe bokeh controls
  and dedicated regenerated-depth provenance. A model reference is not a depth
  raster. Existing native/simple XMP decoding does not settle Adobe fidelity.
- **LR-7:** `GeometrySettings` has crop/orientation/constrain_crop, Upright
  mode/guides and seven transform sliders. There is no arbitrary 3x3 homography,
  saved solve center/focal metadata or multiple Upright solutions. Four-segment
  endpoints can target guides, but solve metadata cannot. Perspective sliders
  already translate; their existence does not implement UprightTransform_*.
  Cloud distraction removal needs an explicit user-facing unsupported explanation;
  today EnableDistractionRemoval receives the generic unknown/unsupported warning.

## Diagnostic and source-contract boundaries (29c compatibility)

Audited sources: `import-lrcat/src/lua_develop.rs` KEY_MAP,
EXTENDED_TONE_CURVE_KEYS, retain_source and to_xmp; `import-lrcat/src/xmp.rs`
parse; `import-lrcat/src/lib.rs` develop fallback/report handling;
`engine-api/src/recipe/{crs,settings,mask}.rs`; delegated
`sidecar/src/{develop,structures,masks}.rs`; `import-lrcat/README.md`.

For pending/unmapped keys, the actual live slot is
`recipe.unknown["lrcat_develop_source"]["properties"][adobe_key]`, tagged
`shape: lua-values` (exact Lua literal) or `xmp-fragments` (exact fragment).
Source retention is independent of whether a value is active or a subset decodes.
Lua also has `lrcat_develop_lua`, ordered `lrcat_develop_lua_entries` and positional
fallbacks. XMP keeps `sidecar_xmp` and per-key `crs:*` diagnostics. The two legacy
CA keys are recognized CRS Legacy targets: diagnostic retention can live under
`crs:*` even though retain_source does not include them in properties.

Any recognized key can be diagnostic-retained for malformed/out-of-range numeric
values, invalid choices/booleans/curves, duplicate/superseded values, process
version disagreement or unsupported structure encoding. These are value/shape
failures, not new Adobe key names; lane ownership follows the underlying key.
Unknown future string keys have the catch-all row. Numeric/positional Lua entries
are not Adobe develop key names. AUX Enhance* metadata uses provenance, not pixel
reconstruction; a same-named property incorrectly placed in CRS is unsupported.

Decode failure or missing process version retains the whole row as `raw-text`;
oversized cells use `cell-descriptor` (possibly truncated), not the properties
map. History/snapshots are source timelines, not translated develop properties.
These fallbacks and the raw XMP packet are excluded from the translated-row
contract; malformed examples should not be labeled translated fixtures.

LR-0 changes no decoder, mapping, recipe schema, dependencies or fixture goldens.
Existing recipe bytes therefore remain untouched, including unhandled keys.
Downstream lanes must keep untranslated inputs byte-identical, make small additive
codec edits, and update the matrix only with valid synthetic import evidence.
This guard is deliberately not a render, full catalog, XMP parity, or complete
Adobe-schema test; those remain acceptance work for the translation lanes.
