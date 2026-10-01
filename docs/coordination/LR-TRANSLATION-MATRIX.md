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
- `approximate`: recipe fields are populated, exact source remains in
  `lrcat_develop_source`, and `translation_diagnostics` records an info-level
  `approximate: <reason>` message. No user-facing warnings for that mapping.
  Unverified Adobe conventions must use this status.
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
JSON pointer exists. Approximate rows require an exact retained literal, an
info-level `approximate: ` diagnostic, and zero warnings. It also checks inventory coverage against KEY_MAP and named
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
| `ChromaticAberrationB` | `/settings/lens/legacy_ca_blue` | LR-7 | approximate | `-25` |
| `ChromaticAberrationR` | `/settings/lens/legacy_ca_red` | LR-7 | approximate | `35` |
| `Clarity` | /settings/tone/clarity (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `CompatibleVersion` | MISSING: Adobe application compatibility metadata (not /process_version) | LR-7 | unsupported-diagnostic | — |
| `Contrast` | /settings/tone/contrast (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `ConvertToGrayscale` | MISSING: monochrome mode in /settings/color (saturation is not equivalent) | LR-2 | unsupported-diagnostic | — |
| `CurveRefineSaturation` | MISSING: tone-curve saturation refinement | LR-2 | unsupported-diagnostic | — |
| `DepthBasedCorrections` | /settings/locals/adjustments (MaskKind::Depth) | LR-4 | unsupported-diagnostic | — |
| `DepthMapInfo` | MISSING: imported depth resource reference/calibration; LensBlur.depth_model identifies a model | LR-2 | unsupported-diagnostic | — |
| `EnableDistractionRemoval` | MISSING: cloud removal result/resource and execution semantics | LR-7 | unsupported-diagnostic | — |
| `GenerativeRemove` | requires Adobe cloud; not translatable | LR-7 | unsupported-diagnostic | — |
| `GenerativeFill` | requires Adobe cloud; not translatable | LR-7 | unsupported-diagnostic | — |
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
| `UprightCenterMode` | `/settings/geometry/upright/homography` | LR-7 | approximate | `1` |
| `UprightCenterNormX` | `/settings/geometry/upright/homography` | LR-7 | approximate | `0.25` |
| `UprightCenterNormY` | `/settings/geometry/upright/homography` | LR-7 | approximate | `0.75` |
| `UprightDependentDigest` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightFocalLength35mm` | `/settings/geometry/upright/homography` | LR-7 | approximate | `70` |
| `UprightFocalMode` | `/settings/geometry/upright/homography` | LR-7 | approximate | `1` |
| `UprightFourSegments*` | MISSING: arbitrary Upright family members/solve state in /settings/geometry | LR-7 | retained | — |
| `UprightFourSegmentsCount` | `/settings/geometry/upright/guides` | LR-7 | approximate | `4` |
| `UprightFourSegments_0` | `/settings/geometry/upright/guides` | LR-7 | approximate | `'0.1,0.1,0.2,0.9'` |
| `UprightFourSegments_1` | `/settings/geometry/upright/guides` | LR-7 | approximate | `'0.1,0.1,0.2,0.9'` |
| `UprightFourSegments_2` | `/settings/geometry/upright/guides` | LR-7 | approximate | `'0.1,0.1,0.2,0.9'` |
| `UprightFourSegments_3` | `/settings/geometry/upright/guides` | LR-7 | approximate | `'0.1,0.1,0.2,0.9'` |
| `UprightPreview` | MISSING: Adobe solve metadata or projective matrix in /settings/geometry | LR-7 | unsupported-diagnostic | — |
| `UprightTransform*` | MISSING: arbitrary Upright family members/solve state in /settings/geometry | LR-7 | retained | — |
| `UprightTransformCount` | solution inventory metadata retained | LR-7 | retained | — |
| `UprightTransform_0` | inactive Off solution retained; no rendered effect | LR-7 | retained | — |
| `UprightTransform_1` | `/settings/geometry/upright/homography` | LR-7 | approximate | `'1,0,0,0,1,0,0.2,0,1'` |
| `UprightTransform_2` | `/settings/geometry/upright/homography` | LR-7 | approximate | `'1,0,0,0,1,0,0.2,0,1'` |
| `UprightTransform_3` | `/settings/geometry/upright/homography` | LR-7 | approximate | `'1,0,0,0,1,0,0.2,0,1'` |
| `UprightTransform_4` | `/settings/geometry/upright/homography` | LR-7 | approximate | `'1,0,0,0,1,0,0.2,0,1'` |
| `UprightTransform_5` | `/settings/geometry/upright/homography` | LR-7 | approximate | `'1,0,0,0,1,0,0.2,0,1'` |
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
- **LR-7:** optional `geometry.upright.homography` stores a unit-image
  source-to-output map, tagged by `homography_mode`. Selected matrices,
  center/focal framing and complete four-segment guide sets are approximate.
  Imported matrices use row-major source-to-output order; when center/focal
  metadata is present, the assumed frame is `(u-cx, v-cy)/(f35/35)` on both axes.
  Missing centers default to 0.5 and missing focal length to 35mm. Mode flags
  signal saved frame metadata; their Adobe enum semantics remain unverified.
  The matrix is conjugated into unit coordinates at import. This does not
  establish Adobe's aspect-ratio or sensor-rotation convention. CA sign and
  radial units are also unverified. All exact source is retained and info-level
  diagnostics explain the approximation without user-facing warnings.
  Legacy CA applies only to Adobe PV1/2; zero values are absent. Mode/guide edits
  clear saved solutions. Cloud-generated pixels require Adobe's rendered output.

The same `approximate` rule applies to LR-1 PointColors and LR-2 legacy tone
or extended-curve notes: populating a recipe field alone is not evidence of Adobe
convention fidelity. Retained structures may remain `retained`; any promotion to
`translated` needs public DNG+XMP or Adobe-rendered synthetic reference evidence.

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
CA keys remain recognized CRS Legacy targets in the shared sidecar schema.
LR-7b's catalog extension translates valid finite -100..100 values into optional
`/settings/lens/legacy_ca_red` and `legacy_ca_blue`, removing consumed diagnostics.
Malformed values still retain `crs:*` diagnostics even though retain_source does
not include these keys in properties. Both translated rows have synthetic guard
values. CPU rendering uses independent radial R/B scale with green fixed; the
numerical scale is an explicitly documented approximation in the LR-7 HANDOFF,
not a claim of Adobe pixel parity.

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
