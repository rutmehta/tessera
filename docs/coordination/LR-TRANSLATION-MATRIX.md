# LR-0 source inventory for coordinator review

Base inspected: `68264c74c365d8e7a4c33ddaecebf934f9305b08`. Source inspection only; no catalog, builds, tests, repository edits, or commits. This inventory does NOT establish Adobe pixel parity or Adobe private payload semantics. Proposed mappings below require review before implementation.

## Exhaustiveness boundary and architecture

`crates/import-lrcat/src/lua_develop.rs:43-274` contains 199 explicit Lua mappings. Comparing names against `crates/engine-api/src/recipe/crs.rs` yields 143 CrsKey names, all present in KEY_MAP, plus exactly **56 passthrough names with no CrsKey**. All 56 appear explicitly in the matrix below. Two of the 143 CrsKey names (`ChromaticAberrationR/B`) have Legacy rather than a field target; eight Enhance names are informational. Presence in CrsKey is not proof of Adobe payload support or CPU rendering.

Translation ownership is Lua -> synthetic XMP -> shared `sidecar::XmpPacket::to_recipe`; import-lrcat/xmp is the adapter and diagnostics validator, not a second codec (`lua_develop.rs:644-699`, `xmp.rs:25-198`, `sidecar/src/develop.rs:80-129,214-332`). Add Adobe translations in sidecar and table names in CrsKey/KEY_MAP consistently. Coordinate all Lua/XMP edits with Machine B.

An unlimited set of arbitrary Lua keys is classified unknown (`lua_develop.rs:677-680`); no finite source inventory can enumerate keys absent from both source and provided fixtures. Brief-only absent names/families are separately listed below. Synthetic fixture `Preset` and `SyntheticFutureKey` are unknown (`tests/lua_develop.rs:96-111`); do not invent semantics for these. Every other unrecognized future key goes to LR-7 diagnostic coverage until documented semantics permit reassignment. The source cannot establish aggregate real-catalog counts or the brief's 44-group coverage figure.

## A. Every KEY_MAP passthrough property (56)

All rows in this table currently pass through Lua serialization but have no CrsKey and emit `unsupported property` in `import-lrcat/src/xmp.rs:129-131`. Paths are candidate destinations, not implemented claims. `provenance` means `/provenance/properties`, which already exists but needs an explicit informational mapping decision. Every name in braces expands individually.

| Adobe key(s) | Candidate existing recipe path or missing type | Lane | Status / unresolved semantics |
|---|---|---|---|
| AutoTone | `/settings/tone` if regeneration requested; otherwise provenance | LR-2 | No stored automatic-tone request/result type; distinguish authoring flag from already baked slider values. Never apply twice. |
| AutoToneDigest, AutoToneDigestNoSat | provenance | LR-2 | Digest metadata, not a pixel operation; classification must be confirmed. |
| AutoWhiteVersion | provenance | LR-2 | Version metadata; do not infer a white-balance operation. |
| Brightness | `/settings/tone/curves/rgb` candidate | LR-2 | No legacy brightness operator; old-PV transfer function unknown. |
| Clarity | `/settings/tone/clarity` candidate | LR-2 | Existing field/operator; legacy-PV equivalence unproven. |
| CompatibleVersion | provenance | LR-7 | Compatibility metadata; no rendering field required if confirmed informational. |
| Contrast | `/settings/tone/contrast` or `/settings/tone/curves/rgb` | LR-2 | Existing field/operator; legacy-PV mapping unproven. |
| ConvertToGrayscale | Missing monochrome-mode type | LR-2 | Saturation=-100 does not establish Adobe grayscale equivalence. |
| CurveRefineSaturation | Missing curve-saturation refinement type | LR-2 | Existing tone curves do not encode this behavior. |
| DepthBasedCorrections | `/settings/locals/adjustments` candidate | LR-4 | Container semantics/grammar unverified; depth runtime dependency LR-6. |
| DepthMapInfo | `/settings/effects/lens_blur/depth_model` is only model provenance; missing imported depth asset reference | LR-6 | Must identify asset and decode depth, not store descriptor as depth. |
| Exposure | `/settings/tone/exposure` candidate | LR-2 | Old-PV semantics and precedence versus Exposure2012 unverified. |
| FillLight | `/settings/tone/shadows` / curves candidate | LR-2 | No exact legacy fill-light type/operator. |
| GrainSeed | Missing `/settings/effects/grain` seed field | LR-7 | Grain has amount,size,roughness only; preserving exact random realization requires explicit seed semantics. |
| GrayMixerRed, GrayMixerOrange, GrayMixerYellow, GrayMixerGreen, GrayMixerAqua, GrayMixerBlue, GrayMixerPurple, GrayMixerMagenta | Missing B&W mixer type; HSL luminance is not an established equivalent | LR-2 | Eight independent unsupported properties; needs grayscale pipeline semantics. |
| HighlightRecovery | `/settings/tone/highlights` or linearize reconstruction candidate | LR-2 | No exact legacy recovery type; cannot claim slider renaming is equivalent. |
| IncrementalTemperature, IncrementalTint | `/settings/white_balance/{temperature,tint}` candidate | LR-2 | Need relative-delta baseline, units and raw/rendered-input semantics. |
| LensProfileIsEmbedded | `/settings/lens/profile` candidate | LR-7 | Existing profile source enum supports selection; flag precedence with explicit profile and asset availability need definition. |
| OverrideLookVignette | Missing look/vignette override field | LR-7 | Existing look and vignette settings do not express override relationship. |
| RangeMaskMapInfo | `/settings/locals/adjustments/*/components` candidate; asset metadata/reference missing | LR-4 | Descriptor format unverified; may require LR-5 storage support. |
| RedEyeInfo | Missing red-eye retouch kind | LR-3 | Existing Heal/Clone/Remove/Skin are not red-eye correction. |
| SDRBlend, SDRBrightness, SDRClarity, SDRContrast, SDRHighlights, SDRShadows, SDRWhites | Missing separate SDR rendition settings | LR-7 | Seven properties; global tone cannot represent simultaneous HDR/SDR treatments without changing HDR pixels. |
| Shadows | `/settings/tone/blacks` / curves candidate | LR-2 | Legacy Shadows meaning must be verified; do not map solely by matching modern field name. |
| ToggleStyleAmount | `/settings/camera_profile/look/amount` candidate | LR-7 | Relationship to Look Amount and toggling semantics unverified. |
| ToggleStyleDigest | provenance/style identity candidate | LR-7 | No exact digest/toggle representation. |
| ToneCurveName2012 | provenance or `/settings/tone/curves/rgb` by documented preset | LR-2 | Named curve alone may require preset lookup; explicit curve should have defined precedence. |
| UprightCenterMode, UprightCenterNormX, UprightCenterNormY | Missing Upright calibration center fields | LR-7 | Existing Upright stores only mode/guides; no center calibration. |
| UprightDependentDigest | provenance | LR-7 | Dependency/cache identity, not transform matrix. |
| UprightFocalLength35mm, UprightFocalMode | Missing Upright focal calibration fields | LR-7 | Cannot preserve in mode/guides alone. |
| UprightPreview | provenance/cache metadata candidate | LR-7 | Payload/role unverified. |
| UprightTransform_0, UprightTransform_1, UprightTransform_2, UprightTransform_3, UprightTransform_4, UprightTransform_5 | Missing stored homography in recipe; renderer has `lens::Homography` | LR-7 | Six unsupported names; mode-selection, coordinate convention, matrix direction/order and number layout unknown. |
| UprightVersion | provenance | LR-7 | Versioning metadata; preserve with transform interpretation. |
| Version | provenance | LR-7 | Application/version metadata; no pixel operation if confirmed. |

Type evidence: `engine-api/src/recipe/settings.rs:361-383` lens; `435-504` look/profile; `548-554` WB; `639-750` tone/curves; `863-875` color; `939-969` grain/blur; `1080-1135` transform/upright; `1166-1174` output. Retouch kinds: `recipe/mask.rs:330-371`.

## B. Explicit brief families absent from KEY_MAP and CrsKey

These names are unknown at the Lua boundary; equivalent XMP names would be unsupported. Source search does not prove Adobe grammar.

| Adobe key/family | Recipe destination | Lane | Current gap |
|---|---|---|---|
| ExtendedToneCurvePV2012, ExtendedToneCurvePV2012Red, ExtendedToneCurvePV2012Green, ExtendedToneCurvePV2012Blue | `/settings/tone/curves/{rgb,red,green,blue}` candidates | LR-2 | Need domain/range and extended-vs-standard precedence; current adapter restricts curve coords to 0..255 and codec divides by 255 (`xmp.rs:204-238`; `develop.rs:277-300`). Blind aliases may clip HDR/extended points. |
| UprightFourSegments* | `/settings/geometry/upright/guides` candidate | LR-7 | Exact suffix inventory not present in source/brief; parse documented segment grammar then normalized GuideLine. |
| Upright*Count | provenance or collection cardinality validation | LR-7 | Mentioned only by coordination note; exact names/meaning not in code. Do not invent count expansion. |
| UprightTransform* other than _0.._5 | Missing stored transform type | LR-7 | Unknown family suffixes remain diagnostic; six known spellings are in A. |
| EnableDistractionRemoval | Missing cloud-feature result/request type | LR-7 | Explicit explained unsupported diagnostic required by lane brief; boolean is not completed removal pixels. Clarify contradiction with overarching “everything renders” goal. |
| Preset, SyntheticFutureKey; arbitrary unmatched keys | `/unknown/lrcat_develop_lua` today; explained diagnostic | LR-7 | Preserve fail-safe diagnostics; no inferential mapping. |

## C. Recognized keys with translation/render gaps

| Adobe key or nested field | Existing path | Lane | Source-supported status |
|---|---|---|---|
| PointColors | `/settings/color/point_colors` | LR-1 | Codec requires Tessera typed `ts:` data; Adobe string grammar unimplemented (`structures.rs:3-9,233-237`). PointColor has source_lch, hue_shift, saturation_shift, luminance_shift, one range; NO separate feather or per-axis ranges (`settings.rs:837-847`). CPU `color()` rejects any points (`color_detail.rs:11-16`); full render supported-settings also omits points. Native roundtrip is not Adobe import or rendering. |
| RetouchAreas | `/settings/locals/retouch` | LR-3 | Only ts:id/kind/target/enabled with CRS Opacity/Feather; Adobe geometry/operation grammar absent (`structures.rs:238-252`). Existing radial/brush targets and normalized source offsets can represent many spots/strokes. |
| RetouchInfo | same | LR-3 | Adobe string grammar absent; requires native ts fields. Empty alias protection exists but both nonempty aliases need precedence/dedup policy (`structures.rs:219-227`). |
| LensBlur Active, BlurAmount | `/settings/effects/lens_blur/{amount,...}` | LR-6 | Active false -> None; amount supported. Adobe FocalRange or BokehShape explicitly rejects whole structure (`structures.rs:183-216`). Native focus_range/bokeh/depth_model survive ts fields only. |
| LensBlur FocalRange, BokehShape, depth descriptor/source | same plus missing depth asset ref | LR-6 | Four-value focal range vs native two-value range, numeric shape vocabulary, and depth source unresolved (`structures.rs:7-9`). CPU blur operator exists with runtime depth; model is provenance and never invokes inference (`lens_blur.rs:34-41`). |
| ChromaticAberrationR, ChromaticAberrationB | no recipe_path (Legacy) | LR-7 | Table says Legacy (`crs.rs:311-312`); codec immediately rejects absent recipe_path (`develop.rs:214-216`). Contrary to table prose, no best-effort conversion in inspected decode. Manual channel-CA representation missing. |
| PerspectiveUpright; PerspectiveVertical, Horizontal, Rotate, Aspect, Scale, X, Y | `/settings/geometry/upright/mode`; `/settings/geometry/transform/{vertical,horizontal,rotate,aspect,scale,offset_x,offset_y}` | LR-7 | Existing enum/scalar mapping and CPU geometry/Upright analysis. Does NOT preserve Adobe stored homography or imply matching estimates. `crs.rs:321-328`, `develop.rs:243-245`, `pipeline-cpu/src/upright.rs:43-81`. |
| Look | `/settings/camera_profile/look` | LR-7 | Codec maps Name/Amount only (`develop.rs:260-270`); referenced style rendering/other Adobe look payload not established. Full raw renderer excludes camera_profile.look. |
| HDREditMode, HDRMaxValue | `/settings/output/{hdr,hdr_headroom_stops}` | LR-7 | Typed mapping exists but source itself labels HDR on and units/range assumptions (`crs.rs:369-372`); flag this fidelity uncertainty. |

Eight Enhance properties are already informational and stored in provenance (`crs.rs:287-294`; `develop.rs:93-100`): EnhanceDenoiseAlreadyApplied, EnhanceDenoiseVersion, EnhanceDenoiseLumaAmount, EnhanceDetailsAlreadyApplied, EnhanceDetailsVersion, EnhanceSuperResolutionAlreadyApplied, EnhanceSuperResolutionVersion, EnhanceSuperResolutionScale. Do not regenerate already-baked enhancements. Their aux namespace is intentional.

## D. MaskGroupBasedCorrections detailed inventory (LR-4 + LR-5)

Root destination `/settings/locals/adjustments`. The entire property decode is atomic: one unsupported component can reject all its groups (`sidecar/src/masks.rs:334-408`; adapter test `xmp.rs:485-489`). Even successful import always retains a source fidelity warning (`xmp.rs:180-186`). Counts alone cannot establish full mask translation.

| Payload | Recipe field | Lane | Gap/status |
|---|---|---|---|
| CorrectionName, CorrectionActive, CorrectionAmount | name, enabled, amount (*100) | LR-4 | Existing mapping. Native LocalId and CompositeInverted only in ts fields. |
| LocalExposure2012, LocalContrast2012, LocalHighlights2012, LocalShadows2012, LocalWhites2012, LocalBlacks2012, LocalTemperature, LocalTint, LocalHue, LocalSaturation, LocalTexture, LocalClarity2012, LocalDehaze, LocalSharpness, LocalLuminanceNoise, LocalMoire, LocalDefringe | params.{exposure,contrast,highlights,shadows,whites,blacks,temperature,tint,hue,saturation,texture,clarity,dehaze,sharpness,noise,moire,defringe} | LR-4 | Existing codec PARAMS (`masks.rs:14-32`); local defringe CPU rejects nonzero (`locals.rs:75-80`). Adobe numeric scaling/operator equivalence still needs swatches. |
| LocalToningHue, LocalToningSaturation | params.color_overlay | LR-4 | Codec exists; CPU rejects any overlay (`locals.rs:75-80`). |
| Mask/Gradient; FullX/Y, ZeroX/Y; legacy StartX/Y, EndX/Y | components.kind=linear; start,end | LR-4 | Existing decode/rasterize; adapter normalizes legacy spellings (`xmp.rs:252-290`). |
| Mask/CircularGradient; Left,Right,Top,Bottom,Angle,Feather | radial; center,radii,angle,feather | LR-4 | Existing mapping; presence of Flipped explicitly rejects (`masks.rs:433-470`). Need Adobe Flipped semantics. |
| MaskActive=false | Missing per-component enabled bit | LR-4 | Explicitly rejects; group enabled exists but is different. |
| MaskBlendMode; MaskInverted | combine add/subtract/intersect, invert | LR-4 | Existing 0/Add,1/Subtract,2/Intersect; any other mode rejected. Need check group nesting/order semantics. |
| nested Masks | Missing nested composite type (flat component list only) | LR-4 | Explicit rejection; flattening arbitrary nesting can change subtract/intersect semantics. |
| group-level or nested CorrectionRangeMask | luminance_range/color_range/depth components candidate | LR-4 | Explicit rejection outside native range kind. Adobe range type codes/LumRange/sample encoding unknown. |
| LumMin, LumMax, LumFeather; DepthMin, DepthMax, DepthFeather; ColorAmount | range,smoothness/feather,amount | LR-4 | Recognized only under native ts:kind; Adobe descriptor selection absent. |
| Adobe brush/dab masks | brush.strokes (points,radius,feather,flow,erase) | LR-4 | Only native ts:strokes parse; no Adobe dab decoder. CPU brush rasterization exists. Exact radius, pressure and coordinate conventions need synthetic proof. |
| Adobe color-range samples | color_range.samples (OkLab) | LR-4 | Only ts:samples; Adobe sample color space unknown. Color smoothness is runtime MaskOptions, not serialized MaskKind. |
| Mask/Sky, Mask/Subject, Mask/Background | sky/subject/background optional model | LR-5 | Recognized as semantic selection request; no Adobe raster import. CPU procedural rasterizer rejects AI variants (`pipeline-cpu/src/masks.rs:169`). |
| Mask/Image; people/object/landscape Adobe encodings | existing person/object/landscape variants; missing raster ref kind | LR-5 | Mask/Image unsupported. Person/object/landscape only selected via ts:kind with required private metadata. No import-lrcat mask helper-table/store reader found. |
| Saved AI raster/helper .lrdata / Masks tables | mask-store::MaskRaster/MaskStore exists outside recipe | LR-5 | Need verified catalog/helper schema, raster decode, asset identity and recipe/runtime binding. Source offers no Adobe storage contract. |
| depth range masks | depth kind range/feather/model | LR-4 (decode), LR-6 (depth supply) | CPU works only with runtime depth plane; model field does not itself generate a plane. |

Mask type evidence `engine-api/src/recipe/mask.rs:61-178,215-236,254-310`. It lacks raster-reference, nested-group, per-component enabled and per-component opacity fields. Do not casually drop these semantics.

## E. Rendering and sequencing blockers

1. LR-1 must implement CPU Point Color, not just decode. Full raw `render.rs:441-458` supported-settings check must admit implemented points only after pipeline integration.
2. LR-3 must connect recipe retouch to rendering. Brush clone/heal operators exist (`crates/brush/src/engine.rs:568-589`), but full raw renderer admits only locals.adjustments, not locals.retouch (`crates/pipeline-cpu/src/render.rs:451`). Existing brush UI operators are not evidence of recipe CPU retouch.
3. LR-4 must cover local defringe/color overlay and nested/disabled semantics to claim all locals render. CPU procedural masks support gradient/radial/brush/luminance/color/depth; semantic AI needs integration through mask-ai/ml-segment and persistent store.
4. LR-5 has a schema/integration decision: mask-store raster API (`mask-store/src/lib.rs:10-101`) exists; MaskKind has no asset reference and importer has no helper asset reader. Regenerated categories cannot reproduce Adobe pixels; diagnostics must state regenerated and record category/model, with unknown category remaining blocked rather than guessed. Source-only synthetic work can test storage plumbing but cannot infer actual Adobe binary grammar.
5. LR-6 CPU blur/runtime depth already exists (`pipeline-cpu/src/render.rs:342-345`); four Adobe focal breakpoints cannot automatically collapse to native two-value range. Need representation decision only after grammar/semantics evidence.
6. LR-7 stored Upright matrices cannot fit mode/guides losslessly. Renderer already handles lens::Homography internally (`lens_plan.rs:141,287`), so prefer a narrowly reviewed recipe addition or mathematically proven decomposition over automatic re-estimation. Extended legacy CA, B&W, SDR, grain seed, red-eye, cloud removal need explicit lane scope decisions.
7. Current adapter warnings additionally cover malformed values (range/choice/boolean/curve), duplicates, process-version disagreement, legacy PV1/2 best-effort. These are validation diagnostics, not missing-key implementations; do not remove them to make unsupported counts zero (`xmp.rs:82-140,191-196`).
8. Lua serialization itself can reject mapped value shapes: mixed keyed/positional tables, numeric table keys, nested list values, nil list items, invalid XML keys/control chars, odd curve coordinate counts (`lua_develop.rs:730-860`). Real supported encodings may require dedicated shape translation, not merely KEY_MAP expansion. Top-level nil is skipped intentionally. Unknown numeric/positional keys retain source diagnostic.

Recommended review decisions: accept LR-0 finite coverage boundary; approve owner assignments for the 56 passthrough keys and additional renderer gaps; require evidence for each Adobe grammar/scale before implementation; agree which missing recipe fields are necessary; retain explicit blocked/unsupported outcomes for undocumented/cloud-only formats. LR-1/2/3/7 can prepare synthetic RED cases; LR-4 schema decisions precede LR-5, and LR-6 shares depth supply with LR-4/5. Do not claim end-to-end fidelity from successful JSON field population.
