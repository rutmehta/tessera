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

The fifth column is a literal synthetic Lua value, mandatory for `translated` and `approximate`
rows. Nested rows supply the complete top-level key value. For approximations
the guard verifies fields, exact source, info reason and zero warnings. The cheap `translation_matrix` integration test imports each such row via
`lua_develop::parse`, checks retention and diagnostics, and verifies the recipe
JSON pointer exists. Approximate rows require an exact retained literal, an
info-level `approximate: ` diagnostic, and zero warnings. It also checks inventory coverage against KEY_MAP and named
extended curves. A negative control proves that falsely claiming PointColors is
translated fails. Test-only fixture rows prove the `approximate` checks, one
negative per condition (field, source, diagnostic, warnings); LR-7 rows are
`approximate` until their Adobe convention is verified. Add representative structured fixtures when promoting a
structure; a scalar or empty payload is not proof of full structure coverage.

| Adobe key | Existing recipe path or missing field | Lane | Status | Synthetic Lua value |
| --- | --- | --- | --- | --- |
| `*` | MISSING: unknown future Adobe property; classify before mapping | LR-7 | retained | — |
| `AutoTone` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoToneDigest` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoToneDigest*` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoToneDigestNoSat` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `AutoWhiteVersion` | MISSING: Adobe auto-analysis state/digest; resolved sliders belong in /settings/tone | LR-2 | unsupported-diagnostic | — |
| `Brightness` | `/settings/tone/legacy_pv2010/brightness` | LR-2 | approximate | `75` |
| `ChromaticAberrationB` | `/settings/lens/legacy_ca_blue` | LR-7 | approximate | `-25` |
| `ChromaticAberrationR` | `/settings/lens/legacy_ca_red` | LR-7 | approximate | `35` |
| `Clarity` | /settings/tone/clarity (candidate only; PV2010 operator differs) | LR-2 | unsupported-diagnostic | — |
| `CompatibleVersion` | MISSING: Adobe application compatibility metadata (not /process_version) | LR-7 | unsupported-diagnostic | — |
| `Contrast` | `/settings/tone/legacy_pv2010/contrast` | LR-2 | approximate | `50` |
| `ConvertToGrayscale` | `/settings/color/monochrome/enabled` | LR-2 | approximate | `true` |
| `CurveRefineSaturation` | MISSING: tone-curve saturation refinement | LR-2 | unsupported-diagnostic | — |
| `DepthBasedCorrections` | /settings/locals/adjustments (MaskKind::Depth) | LR-4 | unsupported-diagnostic | — |
| `DepthMapInfo` | MISSING: imported depth resource reference/calibration; LensBlur.depth_model identifies a model | LR-2 | unsupported-diagnostic | — |
| `EnableDistractionRemoval` | MISSING: cloud removal result/resource and execution semantics | LR-7 | unsupported-diagnostic | — |
| `GenerativeRemove` | requires Adobe cloud; not translatable | LR-7 | unsupported-diagnostic | — |
| `GenerativeFill` | requires Adobe cloud; not translatable | LR-7 | unsupported-diagnostic | — |
| `Exposure` | `/settings/tone/legacy_pv2010/exposure` | LR-2 | approximate | `1` |
| `ExtendedToneCurveName2012` | MISSING: HDR-domain curve/name; /settings/tone/curves is normalized SDR | LR-2 | retained | — |
| `ExtendedToneCurvePV2012` | `/settings/tone/curves_extended/rgb` | LR-2 | approximate | `{0,0,255,300,510,600}` |
| `ExtendedToneCurvePV2012Blue` | `/settings/tone/curves_extended/blue` | LR-2 | approximate | `{0,0,255,300,510,600}` |
| `ExtendedToneCurvePV2012Green` | `/settings/tone/curves_extended/green` | LR-2 | approximate | `{0,0,255,300,510,600}` |
| `ExtendedToneCurvePV2012Red` | `/settings/tone/curves_extended/red` | LR-2 | approximate | `{0,0,255,300,510,600}` |
| `FillLight` | `/settings/tone/legacy_pv2010/fill_light` | LR-2 | approximate | `30` |
| `GrainSeed` | MISSING: explicit seed in /settings/effects/grain | LR-2 | unsupported-diagnostic | — |
| `GrayMixerAqua` | `/settings/color/monochrome/mixer/aqua` | LR-2 | approximate | `25` |
| `GrayMixerBlue` | `/settings/color/monochrome/mixer/blue` | LR-2 | approximate | `25` |
| `GrayMixerGreen` | `/settings/color/monochrome/mixer/green` | LR-2 | approximate | `25` |
| `GrayMixerMagenta` | `/settings/color/monochrome/mixer/magenta` | LR-2 | approximate | `25` |
| `GrayMixerOrange` | `/settings/color/monochrome/mixer/orange` | LR-2 | approximate | `25` |
| `GrayMixerPurple` | `/settings/color/monochrome/mixer/purple` | LR-2 | approximate | `25` |
| `GrayMixerRed` | `/settings/color/monochrome/mixer/red` | LR-2 | approximate | `25` |
| `GrayMixerYellow` | `/settings/color/monochrome/mixer/yellow` | LR-2 | approximate | `25` |
| `HighlightRecovery` | `/settings/tone/legacy_pv2010/recovery` | LR-2 | approximate | `20` |
| `IncrementalTemperature` | MISSING: relative white-balance delta; /settings/white_balance uses absolute controls | LR-2 | unsupported-diagnostic | — |
| `IncrementalTint` | MISSING: relative white-balance delta; /settings/white_balance uses absolute controls | LR-2 | unsupported-diagnostic | — |
| `LensBlur` | `/settings/effects/lens_blur` | LR-6 | retained | — |
| `LensProfileIsEmbedded` | MISSING: embedded-profile/Look-vignette override semantics in /settings/lens | LR-7 | unsupported-diagnostic | — |
| `MaskGroupBasedCorrections` | `/settings/locals/adjustments` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumRange="0.1 0.3 0.7 0.9"}}}}}` |
| `MaskGroupBasedCorrections/CorrectionRangeMask` | `/settings/locals/adjustments/0/components/0/range` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumRange="0.1 0.3 0.7 0.9"}}}}}` |
| `MaskGroupBasedCorrections/Flipped` | `/settings/locals/adjustments/0/components/0/invert` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/CircularGradient",MaskID="synthetic",Left=0.2,Right=0.8,Top=0.1,Bottom=0.9,Flipped=false}}}}` |
| `MaskGroupBasedCorrections/Mask/Background` | `/settings/locals/adjustments` | LR-5 | approximate | `{ { LocalExposure2012 = 1, CorrectionMasks = { { What = 'Mask/Background' } } } }` |
| `MaskGroupBasedCorrections/Mask/CircularGradient` | `/settings/locals/adjustments/0/components/0/radii` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/CircularGradient",MaskID="synthetic",Left=0.2,Right=0.8,Top=0.1,Bottom=0.9,Flipped=false}}}}` |
| `MaskGroupBasedCorrections/Mask/Gradient` | `/settings/locals/adjustments/0/components/0/start` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/Gradient",MaskID="synthetic",FullX=0,FullY=0,ZeroX=1,ZeroY=0}}}}` |
| `MaskGroupBasedCorrections/Mask/Image` | `/settings/locals/adjustments` | LR-5 | approximate | `{ { LocalExposure2012 = 1, CorrectionMasks = { { What = 'Mask/Image', MaskSubType = 2, MaskDigest = 'synthetic-resource' } } } }` |
| `MaskGroupBasedCorrections/Mask/Object` | `/settings/locals/adjustments` | LR-5 | approximate | `{ { LocalExposure2012 = 1, CorrectionMasks = { { What = 'Mask/Object', Left = 0.2, Top = 0.2, Right = 0.8, Bottom = 0.8 } } } }` |
| `MaskGroupBasedCorrections/Mask/Paint` | `/settings/locals/adjustments/0/components/0/strokes` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/Paint",Radius=0.1,Flow=0.5,CenterWeight=0.5,MaskValue=1,Dabs={"d 0.5 0.5"}}}}}` |
| `MaskGroupBasedCorrections/Mask/People` | `/settings/locals/adjustments` | LR-5 | approximate | `{ { LocalExposure2012 = 1, CorrectionMasks = { { What = 'Mask/People' } } } }` |
| `MaskGroupBasedCorrections/Mask/Sky` | `/settings/locals/adjustments` | LR-5 | approximate | `{ { LocalExposure2012 = 1, CorrectionMasks = { { What = 'Mask/Sky' } } } }` |
| `MaskGroupBasedCorrections/Mask/Subject` | `/settings/locals/adjustments` | LR-5 | approximate | `{ { LocalExposure2012 = 1, CorrectionMasks = { { What = 'Mask/Subject' } } } }` |
| `MaskGroupBasedCorrections/MaskActive` | `/settings/locals/adjustments/0/components/0/enabled` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/Gradient",MaskID="synthetic",FullX=0,FullY=0,ZeroX=1,ZeroY=0,MaskActive=false}}}}` |
| `MaskGroupBasedCorrections/MaskBlendMode` | `/settings/locals/adjustments/0/components/0/combine` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/Gradient",MaskID="synthetic",FullX=0,FullY=0,ZeroX=1,ZeroY=0,MaskBlendMode=1}}}}` |
| `MaskGroupBasedCorrections/Masks` | `/settings/locals/adjustments/0/components/0/group` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/Group",Masks={{What="Mask/Gradient",MaskID="synthetic",FullX=0,FullY=0,ZeroX=1,ZeroY=0}}}}}}` |
| `OverrideLookVignette` | MISSING: embedded-profile/Look-vignette override semantics in /settings/lens | LR-7 | unsupported-diagnostic | — |
| `PerspectiveAspect` | `/settings/geometry/transform/aspect` | LR-7 | translated | `1` |
| `PerspectiveHorizontal` | `/settings/geometry/transform/horizontal` | LR-7 | translated | `1` |
| `PerspectiveRotate` | `/settings/geometry/transform/rotate` | LR-7 | translated | `1` |
| `PerspectiveScale` | `/settings/geometry/transform/scale` | LR-7 | translated | `105` |
| `PerspectiveUpright` | `/settings/geometry/upright/mode` | LR-7 | translated | `1` |
| `PerspectiveVertical` | `/settings/geometry/transform/vertical` | LR-7 | translated | `1` |
| `PerspectiveX` | `/settings/geometry/transform/offset_x` | LR-7 | translated | `1` |
| `PerspectiveY` | `/settings/geometry/transform/offset_y` | LR-7 | translated | `1` |
| `PointColors` | `/settings/color/point_colors` | LR-1 | approximate | `{{ SrcHue=0, SrcSat=0.9, SrcLum=0.5, HueShift=0.5 }}` |
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
| `Shadows` | `/settings/tone/legacy_pv2010/blacks` | LR-2 | approximate | `5` |
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
| `Blacks` | `/settings/tone/legacy_pv2010/blacks` | LR-2 | approximate | `5` |
| `Recovery` | `/settings/tone/legacy_pv2010/recovery` | LR-2 | approximate | `20` |
| `MaskGroupBasedCorrections/CorrectionRangeMask/LumRange` | `/settings/locals/adjustments/0/components/0/luminance_bounds` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumRange="0.1 0.3 0.7 0.9"}}}}}` |
| `MaskGroupBasedCorrections/CorrectionRangeMask/Type=2` | `/settings/locals/adjustments/0/components/0/range` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=2,LumRange="0.1 0.3 0.7 0.9"}}}}}` |
| `MaskGroupBasedCorrections/CorrectionRangeMask/Type=3` | `/settings/locals/adjustments/0/components/0/range` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=3,DepthMin=0.2,DepthMax=0.8}}}}}` |
| `MaskGroupBasedCorrections/CorrectionRangeMask/Type=1` | `/settings/locals/adjustments/0/components/0/samples` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=1,ColorAmount=0.5,PointModels={"0.2 0.3 0.4 0.5 0.5 0"}}}}}}` |
| `MaskGroupBasedCorrections/CorrectionRangeMask/PointModels` | `/settings/locals/adjustments/0/components/0/samples` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=1,ColorAmount=0.5,PointModels={"0.2 0.3 0.4 0.5 0.5 0"}}}}}}` |
| `MaskGroupBasedCorrections/CorrectionRangeMask/AreaModels` | `/settings/locals/adjustments/0/components/0/samples` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/RangeMask",CorrectionRangeMask={Type=1,ColorAmount=0.5,AreaModels={"0.2 0.3 0.4 0.5 0.5 0"}}}}}}` |
| `MaskGroupBasedCorrections/Mask/Paint/Dabs` | `/settings/locals/adjustments/0/components/0/strokes` | LR-4 | approximate | `{{CorrectionMasks={{What="Mask/Paint",Radius=0.1,Flow=0.5,CenterWeight=0.5,MaskValue=1,Dabs={"d 0.5 0.5"}}}}}` |


## Per-lane representation notes

- **LR-1:** `PointColor` retains native source LCH and shifts; optional `selection`
  carries imported HSL and independent sample-relative H/S/L feather limits.
  LR-1b translates supported SDK/19-number swatches, supplies
  reference defaults for absent range tables, and skips all-−1 placeholders.
  LR-1c classifies these as approximate, retains exact source and appends shared
  info diagnostics. Point selection precedes B&W; nonempty points require schema
  v4. Variance and unknown fields remain retained.
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
- **LR-4:** disabled components, nested trees (at most eight component levels),
  four-bound display luminance, scalar depth, individual brush dabs and color
  models decode with source retained and info diagnostics. Adobe blend codes,
  pre-geometry coordinates, radial rotation in normalized coordinates, feather,
  dab flow/hardness and sample colors remain approximations. Legacy flat envelopes
  preserve their pinned bytes and remain retained; new audited forms use the
  approximation contract. New nondefault fields conditionally write schema 4.
- **LR-5:** `MaskComponent.adobe_ai` holds opaque resource identity, category,
  regeneration state and an optional mask-store key. Apply resolves caller-owned
  grayscale PNG/TIFF into bounded image-owned pins; preview/export read those pins.
  Missing resources use the existing subject/sky/background/prompted backend.
  Person sub-parts use subject with a per-part info limitation. Adobe conventions
  remain approximate, with exact source retained. Unknown subtypes remain opaque.
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

## LR-4c interpretation and evidence boundary

The LR-4b branch rows have been reconciled above. Its earlier translated claims
for LumRange, Type 2/3 and Flipped are now `approximate`; recognized Dabs and
Type 1 color samples now populate renderable fields under the same contract.
Malformed/unknown tokens remain source-retained with an unsupported diagnostic.
LR-5 now handles recognized Mask/Image forms approximately; see its rows above. No Adobe-rendered chart was used to assert equivalence.
See [LR-4c handoff](../../tools/orchestrate/wp/LR-4/HANDOFF.md).

LR-4e preserves native/previously saved luminance masks in linear light. Adobe
range imports carry `luminance_domain: "display"`, which conditionally requires
schema 4. Parametric gradient/radial `MaskValue` is retained with an explicit
approximation diagnostic: strength is not reproduced (unit selection is used).
See [LR-4e handoff](../../tools/orchestrate/wp/LR-4e/HANDOFF.md).
