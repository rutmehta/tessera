# LR-6b — approximate Lens Blur and depth translation

Machine B, branch `wp/LR-6-lens-blur`, local only. Commits are descendants of
`dcf07355`; no rebase. Original RED tests: `a258521f`. The Machine A ruling
supersedes the original full-fidelity blocker: an unverified Adobe convention
must be **approximate**, with translated fields, exact per-key source in
`lrcat_develop_source`, info-level `approximate: <reason>`, and zero user-facing
warnings. Both matrix rows use this status; the guard checks all four conditions
and negative tests remove each condition independently.

The old RED assertion requiring source deletion has therefore been replaced
with exact-source retention. Original RED evidence remains in `evidence/red.log`.
There is no Adobe-rendered synthetic chart or public DNG+XMP parity evidence.
No real catalog or user image has been opened. All fixtures are invented.

## Per-field interpretation and uncertainty

Paths below are relative to `/settings/effects/lens_blur`. All Adobe conventions
in this table are **unverified**. Types/names follow the public
[ExifTool XMP LensBlur and DepthMapInfo tables](https://www.exiftool.org/TagNames/XMP.html#LensBlur),
checked 2026-10-01; those tables do not specify rendering semantics. Lua numbers
for documented string fields (e.g. Version or DepthSource) are converted to their
text spellings by the existing Lua→XMP adapter. The exact Lua/XMP spelling is
always retained separately.

| Adobe field (type) | Recipe field / interpretation | Specific uncertainty |
| --- | --- | --- |
| Active (boolean) | False disables blur (`None`, or amount 0 when depth metadata needs a container); true enables it. `adobe.active` is also recorded with extended optics. | Enable behavior is defensible; Adobe internal state transitions are not reproduced. |
| BlurAmount (real) | Existing `amount`, percent of the CPU maximum radius (default 16 input pixels). | Adobe's amount-to-radius/image-size scaling is unknown. |
| FocalRange (string) | `adobe.focal_range`: four ordered numbers divided by 100 and clamped to [0,1]. Inner two also populate existing `focus_range`. Outer→inner intervals ramp linearly from full blur to sharp; beyond outer endpoints blur saturates. | Four-point order, percentage units, near→far direction, and linear falloff are assumed. Collapsed shoulders are steps. Malformed/unordered/nonfinite inputs retain the existing decode error behavior. |
| BokehShape (real) | Original number in `adobe.bokeh_shape`; existing `bokeh`: 0 circle, 1 bubble, 2 five-blade, 3 ring, 4 cat-eye; unknown/fractional values fall back to circle. | Enum ordering and exact aperture profiles are unverified. |
| BokehShapeDetail (real) | `adobe.bokeh_shape_detail`; clamp to 0..100, divide by 100, increase radial pupil weight by detail × normalized radius. | A radial detail approximation, not Adobe's undocumented shape morphing. |
| BokehAspect (real) | `adobe.bokeh_aspect`; signed -100..100 log2 stretch; x stretch is 2^(value/100), y is reciprocal. | Sign, scale and area-preserving convention are assumed. |
| BokehRotation (real) | `adobe.bokeh_rotation`; degrees in the image coordinate plane. | Adobe origin, sign and angle units are unverified. |
| HighlightsBoost (real) | `adobe.highlights_boost`; clamp 0..100, percentage highlight gain in the CPU operator. | Adobe gain/tonemapping curve is unknown. |
| HighlightsThreshold (real) | `adobe.highlights_threshold`; clamp 0..100 and divide by 100 as scene-linear Rec.2020 luminance threshold. | Adobe may use a different transfer function/domain. |
| CatEyeAmount (real) | `adobe.cat_eye_amount`; percentage radial pupil clipping strength. | Native reference clipping is not calibrated to Adobe. |
| CatEyeScale (real) | `adobe.cat_eye_scale`; multiplier /100 on CatEyeAmount; combined strength clamped [0,1]. | Scale's meaning is inferred. The existing cat-eye preset keeps its minimum clipping. |
| SphericalAberration (real) | `adobe.spherical_aberration`; clamp -100..100, signed radial pupil weighting: 1 + value/100 × (2r−1), combined with shape detail; minimum weight 0.01. | This is an intensity-profile approximation, not a wave-optics model. |
| Version (string) | `adobe.version`, provenance only. No recipe version bump. | Does not select an Adobe rendering implementation. |
| FocalRangeSource (real) | `adobe.focal_range_source`, selection provenance only. | Source enum undocumented; no automatic re-selection. |
| SampledArea (string) | `adobe.sampled_area`, selection provenance; exact string. | Coordinate/brush encoding unknown; does not invent a region from it. |
| SampledRange (string) | `adobe.sampled_range`, selection provenance; exact string. | Encoding/relationship to final focus unknown; explicit FocalRange is authoritative. |
| SubjectRange (string) | `adobe.subject_range`, selection provenance; exact string. | Subject selection encoding unknown; explicit FocalRange is authoritative. |
| DepthSource (string) | `depth.depth_source`, opaque provenance. | Does not infer an enum or model identity. |
| BaseRawDepthTable (string) | `depth.base_raw_depth_table`, opaque caller-resolved resource ID. | Never interpreted as a filesystem path or invented helper table schema. |
| BaseRawDepthInputDigest (string) | `depth.base_raw_depth_input_digest`, association metadata. | Digest algorithm unknown; caller must associate resource with the image. |
| BaseRawDepthVersion (string) | `depth.base_raw_depth_version`, provenance. | No unverified binary decoder selected by version. |
| BaseLayeredDepthTable (string) | `depth.base_layered_depth_table`; preferred over raw resource when decodable. | Layered encoding unknown; only independent grayscale containers are accepted. |
| BaseLayeredDepthInputDigest (string) | `depth.base_layered_depth_input_digest`, association metadata. | Digest algorithm unknown. |
| BaseLayeredDepthVersion (string) | `depth.base_layered_depth_version`, provenance. | Version's interpretation unknown. |
| BaseHighlightGuideTable (string) | `depth.base_highlight_guide_table`, guide resource ID only; never mistaken for depth. | Guide encoding/application unknown; HighlightsThreshold uses the CPU approximation above. |
| BaseHighlightGuideInputDigest (string) | `depth.base_highlight_guide_input_digest`, association metadata. | Digest algorithm unknown. |
| BaseHighlightGuideVersion (string) | `depth.base_highlight_guide_version`, provenance. | Guide version semantics unknown. |

`DepthBasedCorrections` is a separate LR-4 matrix row and is unchanged. This lane
maps the LensBlur refinements listed above, not LR-4 local correction masks.

The same interpretations and uncertainty are persisted in
`unknown.lrcat_translation_diagnostics` as `{level:"info", key, message}`.
They never enter the import warning vector. `adobe` and `depth` are optional,
serde-defaulted, skipped when absent. No schema bump and no inline raster data.
Native XMP round-trips use individually typed Tessera fields for the extensions;
these are not presented as Adobe-compatible payload encodings.

## Depth resource and regeneration lifecycle

The pure develop decoder cannot access catalog resources. It records the opaque
identities and `depth.regenerate = true`, with **"regenerated depth: pending"**.
This marker does not claim inference has run. A DepthMapInfo-only record creates
an amount-zero container; it cannot turn blur on by itself.

The host supplies resource bytes through
`image_core::depth::DepthProvider::prepare_lens_blur_depth(recipe, image, store, resolver)`.
The resolver owns image/resource association. Layered then raw IDs are attempted.
Only grayscale 8/16-bit PNG or TIFF of the pre-geometry image extent is accepted.
These are independently decodable containers, **not a claimed Adobe helper
format**. White is approximately interpreted as near, with normalized inverse
depth preserved without min/max stretching. Proprietary, missing, corrupt,
colored, or wrong-size resources fall back to this provider's `estimate` seam.

Successful imported or regenerated maps are stored through ml-depth's existing
mask-store API. `depth.mask_key` contains only a content key. Store read-back is
verified before committing the recipe reference or clearing `regenerate`;
failed writes leave the recipe untouched. History is updated through `Recipe::edit`.
A valid stored key is reused before invoking the resource resolver. On eviction
or corruption, resolution/regeneration is retried. Completed regeneration adds
**"regenerated depth: complete"**; diagnostics retain the earlier pending event.
The returned DepthMap can be installed with `DepthProvider::from_map` for the
existing renderer. This is a host seam, not a new catalog-helper discovery engine.

Tests inject synthetic maps. `from_support` remains the explicit model-backed
host path; no model download or inference runs in the LR-6 gate. The ml-depth
cached-model test is now explicitly ignored unless opted into.

## Validation and compatibility

Run `tools/orchestrate/wp/LR-6/gate.sh`. It pins the requested environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-6-lens-blur"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

The gate also limits Rust test threads to 3. Packages: import-lrcat, engine-api,
image-core, mask-store, sidecar, pipeline-cpu, ml-depth. It runs tests, the paired
import→CPU render test, clippy `--all-targets -- -D warnings`, and fmt check.
Explicit skip names in the script exclude real RAW fixture readers and cached
or trained model inference. Synthetic RAW metadata/cache bookkeeping remain in.
No Swift gate, app build/run, remote push, Cargo.lock, dependencies or board changes.

Coverage includes Lua and XMP source retention, every documented field, all
optional fields' serialization and render effect (or intentional provenance-only
identity), native XMP extension round-trip, decoder clamps/shape fallbacks,
resource import/cache reuse, injected regeneration and failed-store atomicity.
The five untouched-input recipe/warning byte baselines were generated by compiling
`dcf07355` in a temporary detached checkout (removed afterwards). The synthetic
catalog golden was re-pinned only because its LensBlur/DepthMapInfo rows now carry
translated metadata and info. Native LensBlur serialization without extensions
is byte-identical to its pre-extension representation.

The end-to-end test consumes the exact recipe bytes emitted by import-lrcat
through a temporary file (no new test dependency). With an injected two-plane
map: focused pixels must be bit-identical; far checkerboard contrast must fall
below 0.35 from 0.7 (at least 50% attenuation); CPU results after save/reload must
agree within 1e-6 per channel. These are CPU approximation invariants, not Adobe
render equivalence tolerances.

## Gate results (2026-10-01)

| Gate | Result | Evidence |
| --- | --- | --- |
| Seven-package `cargo test --locked ... --no-fail-fast` with named exclusions | exit 0; 517 top-level passed, 0 failed, 4 ignored, 13 filtered | `evidence/lr6b-test.log` |
| Paired synthetic import → CPU render | exit 0; 1 passed | `evidence/lr6b-e2e.log` |
| Additional XMP depth-attribute/exact-fragment test | exit 0; 1 passed | `evidence/lr6b-xmp-attributes.log` |
| Seven-package clippy, all targets, `-D warnings` | exit 0 | `evidence/lr6b-clippy.log` |
| `cargo fmt --all -- --check` | exit 0 | `evidence/lr6b-fmt.log` (empty) |

The broad log also contains 3 successful helper-subprocess test runs (520 passes
if all nested harness summaries are summed). The 4 top-level ignored cases are
the paired E2E (subsequently run successfully), release-only import timing, a
real legacy codec asset test, and a real RAW codec measurement. Named filters
exclude actual model inference and real RAW readers. The added XMP attribute
case was compiled and run separately after the broad test binaries were built.
No semantic production edits followed the broad gate; clippy and E2E compiled
the final source. The historical bundled LibRaw C deprecation messages are
build-script output; Rust clippy completed with warnings denied.

## Local commits

- Feature: `fbaef2944ccd9e3a143a5a10c72cdf021cb5b76c`.
- Tests/matrix/gate: `d34c9a4d91970b0c347b592f477178b0c1424367`.
- Documentation/evidence: the commit containing this handoff; obtain with
  `git log -1 --format=%H -- tools/orchestrate/wp/LR-6/HANDOFF.md`.

All commits carry the requested Claude Opus 5.5 co-author footer. They descend
from `dcf07355` without rebasing and have not been pushed. Temporary baseline
checkout was removed. No Cargo.lock, manifest/dependency, board, Swift or app
changes.
