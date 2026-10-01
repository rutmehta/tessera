# LR-6c — review corrections for Lens Blur and depth translation

This handoff describes LR-6c on top of `caee61c2` (no rebase, local only).
The LR-6b gate results and commit list at the end are historical. LR-6c results
are recorded in the review section below.


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
| Active (boolean) | False disables blur (`None`); true enables it. Depth metadata never creates a Lens Blur container. `adobe.active` is also recorded with extended optics. | Enable behavior is defensible; Adobe internal state transitions are not reproduced. |
| BlurAmount (real) | Existing `amount`, percent of the CPU maximum radius (default 16 input pixels). | Adobe's amount-to-radius/image-size scaling is unknown. |
| FocalRange (string) | `focus_range` is the sole focus-position authority. Inner endpoints populate it; outer-to-inner differences populate optional native `focus_falloff` widths. All four ordered numbers divided by 100 remain unclamped in provenance-only `adobe.focal_range`. Outer→inner intervals ramp linearly from full blur to sharp; beyond outer endpoints blur saturates. For `-48 32 64 144`, widths are 0.8 on both sides, not 0.32/0.36. | Four-point order, percentage units, near→far direction, and linear falloff are assumed. Collapsed shoulders are steps. Malformed/unordered/nonfinite inputs retain the existing decode error behavior. |
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
`unknown.lrcat_translation_diagnostics` as one `{level:"info", key, field, message}` record per translated source field. Missing fields and inactive Lens Blur produce no approximation records. DepthMapInfo without active blur produces only a retained-source info record.
They never enter the import warning vector. `adobe` and `depth` are optional,
serde-defaulted, skipped when absent. No schema bump and no inline raster data.
Native XMP round-trips use individually typed Tessera fields for the extensions;
these are not presented as Adobe-compatible payload encodings.

## Depth resource and regeneration lifecycle

The pure develop decoder cannot access catalog resources. It records the opaque
identities and `depth.regenerate = true`, with **"regenerated depth: pending"**.
This marker does not claim inference has run. A DepthMapInfo-only or Active=false record never creates a Lens Blur container.
Depth metadata stays exact retained source plus a retained-source info record.

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
failed writes leave the recipe untouched. Resource preparation adds no history entry. `Recipe::set_lens_blur_depth` enriches
only the current state and its existing history delta (or base), preserving
entry ids, labels, authors, and replay validity. Import retains one Import-authored
entry; no default User entry is added.
A valid stored key is reused before invoking the resource resolver. On eviction
or corruption, resolution/regeneration is retried. Completed regeneration adds
**"regenerated depth: complete"**; diagnostics retain the earlier pending event.
The renderer now reads the stored mask key through `prepare_lens_blur_depth`
before estimating. `from_support` connects the provider to the host depth store;
`with_store` supports an explicitly supplied store with an injected source.
Renderer settings are immutable, so automatic preparation uses a transient
recipe; hosts call explicit preparation when persisting a newly resolved key
and completed regeneration diagnostics. Existing stored keys are consumed
directly after reload. This does not add catalog-helper discovery or path handling.

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
image-core, mask-store, sidecar, pipeline-cpu, ml-depth, tessera-ffi. It runs tests, the paired
import→CPU render test, clippy `--all-targets -- -D warnings`, and fmt check.
Explicit skip names in the script exclude real RAW fixture readers and cached
or trained model inference. Synthetic RAW metadata/cache bookkeeping remain in.
No Swift gate, app build/run, remote push, Cargo.lock, dependencies or board changes.

Coverage includes Lua and XMP source retention, every documented field, all
optional fields' serialization and render effect (or intentional provenance-only
identity), native XMP extension round-trip, decoder clamps/shape fallbacks,
resource import/cache reuse, injected regeneration and failed-store atomicity.
The five untouched-input recipe/warning byte baselines were generated by compiling
`dcf07355` in a temporary detached checkout (removed afterwards). LR-6b re-pinned the synthetic catalog golden; LR-6c restores the original
`d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`
without changing the digest function. See the LR-6c result below for the remaining
retained-diagnostic byte compatibility issue. Native LensBlur serialization without extensions
is byte-identical to its pre-extension representation.

The end-to-end test consumes the exact recipe bytes emitted by import-lrcat
through a temporary file (no new test dependency). With an injected two-plane
map: focused pixels must be bit-identical; far checkerboard contrast must fall
below 0.35 from 0.7 (at least 50% attenuation); CPU results after saving the recipe file, dropping renderer/store handles, and
reopening the mask-store must agree within 1e-6 per channel. The fallback map has
the wrong dimensions, so any bypass of the persisted key fails without inference.
A post-import focus edit must change the rendered pixels. These are CPU approximation invariants, not Adobe
render equivalence tolerances.

## Historical LR-6b gate results (2026-10-01)

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

## Historical LR-6b local commits

- Feature: `fbaef2944ccd9e3a143a5a10c72cdf021cb5b76c`.
- Tests/matrix/gate: `d34c9a4d91970b0c347b592f477178b0c1424367`.
- Documentation/evidence: the commit containing this handoff; obtain with
  `git log -1 --format=%H -- tools/orchestrate/wp/LR-6/HANDOFF.md`.

All commits carry the requested Claude Opus 5.5 co-author footer. They descend
from `dcf07355` without rebasing and have not been pushed. Temporary baseline
checkout was removed. No Cargo.lock, manifest/dependency, board, Swift or app
changes.


## LR-6c review corrections and validation (2026-10-01)

- B1: inactive Lens Blur and standalone DepthMapInfo leave `lens_blur = None`.
  The original golden digest is restored, never re-pinned.
- B2: one Import-authored history step; preparation preserves history length and
  metadata and keeps recipe replay valid. Failed store writes leave it untouched.
- M1: render consumes `depth.mask_key` from the provider's mask-store before
  attempting regeneration. Resource decoding still accepts only grayscale PNG/TIFF
  at the pre-geometry extent, through opaque resolver IDs. No Lightroom path access.
  Cache read-back precedes committing the 32-byte key. Synthetic E2E saves/reloads
  the actual recipe file and reopens the store, then renders without model access.
- M2: imported focus enters native `focus_range` and optional `focus_falloff`.
  `adobe.focal_range` is provenance only; native edits and subject-focus updates
  are no longer shadowed.
- M3: each translated field has its own reason; inactive Lens Blur emits none.
  Matrix guard checks each supplied field's target and info record, with negative
  tests for missing field, missing reason, source loss, wrong level, and warnings.
  The DepthMapInfo matrix fixture includes an active Lens Blur companion.
- M4: unclamped outer endpoints preserve shoulder widths and linear extrapolation.
- README updated to distinguish approximate supported controls from opaque helpers.

| LR-6c gate | Result | Evidence |
| --- | --- | --- |
| Eight-package broad tests, requested environment and exclusions | exit 101; initial failures: restored golden, intermediate sidecar binary, 20 MP liquify latency | `evidence/lr6c-test.log` |
| Final targeted regression suite | exit 0; 30 passed, 1 paired test ignored here | `evidence/lr6c-targeted-final.log` |
| Full sidecar suite rebuilt from committed source | exit 0; resolves the intermediate null-field failure | `evidence/lr6c-sidecar-final.log` |
| Paired import → file save/reload → mask-store → CPU render | exit 0; 1 passed | `evidence/lr6c-e2e.log`, `evidence/lr6c-e2e-final.log` |
| Eight-package clippy, all targets, `-D warnings` | exit 0 | `evidence/lr6c-clippy.log` |
| `cargo fmt --all -- --check` | exit 0 | `evidence/lr6c-fmt.log` (empty) |
| Serial liquify latency rerun | exit 101; p95 321.3 ms exceeds 250 ms (initial concurrent p95 818.2 ms) | `evidence/lr6c-liquify-serial.log` |

The broad harness summaries (including nested probes) report 1,072 passed,
3 failed, 28 ignored, and 55 filtered. The broad script's final status is
`test=101 e2e=0 clippy=0 fmt=0`, recorded in
`evidence/lr6c-gate-status.log`. It does not hide or normalize test failures.
The liquify timing case was rerun alone with `--test-threads=1`, keeping
`CARGO_BUILD_JOBS=3` and `RAYON_NUM_THREADS=3`. It still failed at p95 321.3 ms
against 250 ms; the threshold and test remain unchanged. This result is not
claimed as a resolved flake. The final outstanding gate failures are that timing
assertion and the restored byte golden below. All LR-6c targeted behavior tests,
full rebuilt sidecar suite, paired E2E, clippy, and fmt pass.

RAW-fixture readers and actual cached/trained model inference are excluded by
name; ignored real-catalog and benchmark tests remain ignored. No Swift/app gate
was run. The historical bundled LibRaw C warnings are build-script output;
Rust clippy completed with warnings denied.

The original full-byte golden currently fails: observed
`be459483cec045b19e6fae5ec597baddc53ae2b04f3611b9a131570dcd13cc00`, expected
`d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`.
The inactive-blur regression itself passes. The mandated retained-source info
record and removal of the old depth warning change the serialized diagnostic
bytes. The digest function remains unchanged; no normalization, skip, or re-pin
hides this failure. Reconciling this byte baseline with the new info contract
remains a coordinator decision. See `evidence/lr6c-golden.log`.

Targeted final checks: 30 passed, 0 failed, 1 paired test ignored in that run;
paired E2E then passed separately. Native XMP round-trip includes focus falloff.
The impulse-radius regression verifies the unclamped ramp numerically.
A focused clippy run found `unnecessary_get_then_check` in a regression test;
`d8d73f65` corrects it, without behavior changes. The broad clippy result below
supersedes that earlier diagnostic.

The broad test process compiled a sidecar binary before the final absent-field
encoding correction and later reported `codec_round_trips_every_structure_independently`
with an unwanted `focus_falloff: null`. The committed implementation omits that
optional tag when absent. Both the final targeted run and a fresh full sidecar
suite pass; `evidence/lr6c-sidecar-final.log` supersedes that intermediate failure.

RED evidence: `evidence/lr6c-red.log` and `evidence/lr6c-red-matrix.log`.
Tests were committed before production edits as `1180073e`.
Implementation commit: `4cf04e8f`; lint-only follow-up: `d8d73f65`.
The final docs/evidence commit is the commit containing this section. Every
LR-6c commit has the requested Claude Opus 5.5 co-author footer, and all descend
from `caee61c2` without rebasing.

Coordinator: `crates/import-lrcat/tests/golden.rs` conflicts with LR-3 at merge.
Recompute the combined golden after merging both implementations; do not take
one lane's digest side. This lane does not authorize a re-pin of its own.

No Cargo.lock, manifests/dependencies, board.json, Swift gate, app, remote push,
real catalog, or user-image changes. RAW-fixture and model tests are explicitly
excluded by `gate.sh`; synthetic tests remain enabled.
