# LR-6 — Lens Blur and depth translation

**Current: LR-6f integration, in the final appendix.** Earlier sections and gate
results below are historical; the LR-6f appendix supersedes their diagnostics,
schema, history, cache, and golden statements.

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


## LR-6d — shared diagnostics and conditional schema conversion (2026-10-01)

Rebased all nine original commits, without squashing, onto `38684d9b`
(`origin/wp/LR-DIAG`, including LR-SCHEMA `02ae8196`). Conflicts were limited to
translation-matrix documentation/guards. Kept LR-DIAG's complete guard and all
base matrix rows, then restored LR-6's additive field checks using `entries()`.
No remote push. The coordinator owns publication of the rebased branch.

### Review status and changed contracts

- **C1 resolved:** the unsupported DepthMapInfo exemption, warning removal, and
  top-level source removal apply only when an active Lens Blur was decoded.
  Inactive Lens Blur and standalone DepthMapInfo emit no info entries. Three
  baseline rows (inactive, standalone depth, inactive plus depth) were captured
  from an isolated `38684d9b` checkout, then added to the existing untouched-input
  fixture without changing its previous rows. The temporary checkout was removed.
  The original full-byte golden remains
  `d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`.
- **C2 resolved:** removed `Recipe::record_translation_info`, its image-core
  caller, and the XMP top-level-array writer. Only
  `import_lrcat::diagnostics::push_approximate` writes translation information.
  Entries are keyed by Adobe key, info/approximate, lane LR-6, and the exact
  matrix recipe path (`/settings/effects/lens_blur` or its `/depth` child).
  The reason begins `approximate: <Adobe subfield>:`. Readers use `entries()`;
  the shared guard retains all four negative conditions plus field-path equality.
  The LR-6 extension additionally checks each supplied field and reason, and
  FocalRange requires both `focus_range` and `focus_falloff`.
- **Active is translated, exact boolean:** it never receives an approximate
  entry. The aggregate LensBlur row remains approximate because its optics/focus
  mappings remain unverified. No Adobe equivalence claim or real-catalog evidence
  was introduced. Nothing translated means no info diagnostic.
- **User history resolved:** depth attachment only folds into an existing
  Import-authored head. A user-authored head or absent head is left unchanged,
  with no extra history entry and no newly persisted key. Preparation still
  returns the usable raster. Import metadata and replay validity are preserved.
- **Eviction resolved:** imported grayscale PNG/TIFF depth uses
  `DepthMap::store_pinned` / `MaskStore::put_pinned`. Files live in the store's
  `pinned/` class, outside normal cache accounting and eviction. Reads prefer
  that class, validate its checksum, and work after reopening the store. Ordinary
  inferred maps keep the cache budget. A synthetic pressure/reopen test proves
  imported depth survives eviction. No regeneration-completion diagnostic is
  written by image-core: this is an import-translation channel, not a render log.
- **Schema resolved:** one `lens_blur` predicate covers the whole optional
  feature, including focus falloff, Adobe controls/provenance, and imported depth.
  `RECIPE_SCHEMA_VERSION` stays 3; active blur serializes as 4. The predicate test
  exercises each nested feature and the no-feature case. LR-6 owns the first-lane
  checklist here: import schema tests, sidecar/merge roundtrip version handling,
  and a local journal save/reopen test asserting envelope schema 4. Existing
  catalog fixtures have no active blur and remain schema 3, so no golden re-pin
  is warranted. Older golden and streaming byte-equality tests remain intact.
- **C3 gate coverage corrected:** `gate.sh` now runs all ordinary tests in all
  nine packages, including merge and the FULL tessera-ffi suite, with no
  command-line `--skip`. Upstream `#[ignore]` tests keep their stated reasons;
  the paired synthetic import-to-render E2E is explicitly run separately.

### Commit order and verification

RED commit: `cd8f17af` (`test(LR-6d):`), with evidence in
`evidence/lr6d-red.log`: six import regressions failed on the old channel/inactive
behavior; the schema predicate test failed because no feature was registered.
Implementation commit: `944fb7d8` (`fix(LR-6d):`). A clippy-only nested-if
collapse followed the initial clean build. The initial gate was interrupted during
FFI develop tests, with no final exit status. Its completed harness results are
preserved in `lr6d-test.log`. The continuation cleaned import-lrcat and reran its
full suite plus the full unfiltered FFI suite. No semantic production edit followed
the clean nine-package gate build; the continuation corrected two stale test comments.

All new LR-6d fixtures are synthetic. The requested unfiltered suite also runs
its existing repository camera fixtures (the CC0 set listed in
`fixtures/fetch.sh`), including the formerly excluded FFI history tests. No real
Lightroom catalog or user photo collection was opened. The real-catalog test
remains opt-in and is listed with its reason in `evidence/lr6d-ignored-tests.md`.
No GUI, app/Swift changes, dependency/manifest/Cargo.lock changes, or board edits.
The Swift gates are therefore not applicable to this lane's changes.


### Final LR-6d gates (continuation)

The nine-package clean removed 95,011 files / 25.6 GiB before the original gate.
That run completed all eight non-FFI package harnesses without failures, then was
interrupted during FFI develop tests. It has **no final exit code** and is not
reported as a completed gate. The continuation cleaned import-lrcat again
(3,613 files / 521.9 MiB), then completed the full import-lrcat and FFI suites,
without command-line skips. Remaining doctests were run separately.

| Gate | Result | Evidence |
| --- | --- | --- |
| Initial clean non-FFI harnesses | 617 top-level passed, 0 failed, 9 ignored; includes import-lrcat subsequently rerun | `evidence/lr6d-test.log` |
| Clean import-lrcat rerun | 95 passed, 0 failed, 1 ignored; original golden and every matrix guard pass | `evidence/lr6d-resumed-test.log` |
| FULL tessera-ffi suite | 566 passed, 1 failed, 30 ignored, 0 filtered; sole failure is Liquify latency | `evidence/lr6d-resumed-test.log` |
| Paired synthetic import → recipe save/reload → reopened depth store → CPU render | export exit 0; render exit 0 | `evidence/lr6d-e2e.log` |
| Remaining seven-package doctests | exit 0 | `evidence/lr6d-doctests.log` |
| Liquify serial rerun (`--test-threads=1 --nocapture`) | exit 101; p95 506.8 ms against 250 ms | `evidence/lr6d-liquify-serial.log` |
| Serial frame-starvation rerun (`--test-threads=1 --nocapture`) | exit 0; 2 passed | `evidence/lr6d-frames-serial.log` |
| Nine-package clippy, all targets, `-D warnings` | exit 0 | `evidence/lr6d-clippy.log` |
| `cargo fmt --all -- --check` | exit 0 | `evidence/lr6d-fmt.log` |

**Remaining blocker: Liquify p95.** The full run measured 340.2 ms p95
(median 245.9 ms); the serial rerun measured 506.8 ms p95 (median 319.4 ms).
Both exceed the unchanged 250 ms threshold. Other lanes were active on this
shared host; serial here means one Rust test thread, not an exclusive host.
These results do not establish that host load is the only cause. C3's missing
coverage is corrected, but its performance gate remains red. No timing threshold
or production Liquify implementation was changed.

All ordinary frame-delivery tests passed in the full run, including
`export_batch_does_not_starve_slider_drag`, `slow_interactive_frames_are_not_starved`,
and `edits_do_not_wait_for_frames_in_flight`. The formerly filtered
`process_version_is_undoable_and_persisted` and
`session_renders_into_surfaces_and_persists_undoable_edits` also passed.
The serial rerun passed both starvation tests: 120 frames during export; render
p90 3.0 ms and setting-to-frame p90 4.4 ms. The interactive burst delivered
39 frames in 284.1 ms. See `evidence/lr6d-frames-serial.log`; its 10 filtered
cases are selection for this additional rerun, not exclusions from the full suite.

There are 39 distinct ignored tests across the broad runs. Every one has a
reason in `evidence/lr6d-ignored-tests.md`; the paired synthetic E2E was explicitly
run afterward. The initial log includes three nested helper-subprocess passes
and 21 internal filter counts, excluded from the 617 top-level count above;
these are subprocess probes, not command-line exclusions from the lane gate.
Existing LibRaw C deprecation warnings remain build-script output, not Rust
clippy failures. Swift/app gates are not applicable because this lane changes
no app or Swift files.

The requirement-by-requirement audit is `evidence/lr6d-review.md`. Final docs and
evidence are in the `docs(LR-6d):` commit containing this appendix; resolve its
hash with `git log -1 --format=%H -- tools/orchestrate/wp/LR-6/HANDOFF.md`.

## LR-6e — Machine A review and LR-7 integration

Rebased the twelve unsquashed LR-6..6d commits onto `52eda533`
(`origin/wp/LR-7-upright`, main `427ab116` plus LR-7..7e). LR-7 owns the
first-lane schema checklist: its schema module docs, sidecar/merge roundtrip
changes, and journal-envelope test were kept. LR-6's duplicate copies were
removed. All 109 base matrix rows remain; only LensBlur and DepthMapInfo rows
changed. LR-6 retains its predicate and bumped-only-when-present tests.

- **D1:** the v4 predicate checks only `focus_falloff`, `adobe`, or `depth`.
  A plain native LensBlur stays schema 3, including on serialization. The new
  regression was observed failing against the old broad predicate.
- **D2:** active imported LensBlur without resolved depth records exactly one
  info-only approximate reason: `regenerated depth: no Adobe depth resource;
  Tessera estimates depth at render`. Its key is DepthMapInfo when present,
  otherwise LensBlur; its field is `/settings/effects/lens_blur/depth` and lane
  is LR-6. `depth.regenerate` remains true. A successful resource attachment
  removes that pending reason before publication. Rendering writes no history
  or translation diagnostics and emits no completion assertion.
- **D3:** the field checker is shared between the matrix and per-subfield tests,
  and runs inside every LensBlur/DepthMapInfo field loop. It checks the target
  and exactly one `approximate: <field>:` reason. The duplicate-reason negative
  control proves an extra reason fails. Diagnostic readers use `as_deref()`.
- **D4:** removed the separate `prepare_lens_blur_depth` API. The production
  `LrcatImport::apply_with_depth_resolver` applies resolved bytes through
  `image_core::depth::import_lens_blur_depth` before writing the sidecar. The
  existing caller-owned opaque-resource association seam is exposed as the
  `LrcatDepthResolver` callback; no path guessing or proprietary decoding was
  added. Ordinary `apply` supplies no resolver and leaves regeneration pending.
  Source rasters are read-only; destinations are checked by Sidecar's
  Lightroom-owned-path guard. Depth is attached to the existing Import entry;
  user edits then retain its key without being rewritten.
- **Bound and lifetime:** a stable image-ID digest owns one durable raster
  under Tessera support `previews/depth-cache/pinned`. Maximum per slot is
  256 MiB (including its 48-byte header/checksum), so N imported image slots
  occupy at most N × 256 MiB at rest, plus at most one 256 MiB atomic-write
  temporary per concurrently committing importer. Re-import replaces that slot,
  or removes it if no
  usable depth is supplied. Failed sidecar publication restores the prior slot.
  `Engine::forget_missing` removes it with the image record; an existing image
  retains it. The independent inference cache retains its 256 MiB eviction cap.
- **Preview consumption:** rendering now reads full-size imported maps at
  matching pyramid levels using in-memory downsampling. The explicit preview
  regression failed before the change (it fell back to an incompatible estimator)
  and passed afterward, with byte-identical recipe and unchanged stored raster.
  No derived depth file or completion diagnostic is written.
- **Rebase interaction:** LR-7 records history after parsing. Depth metadata is
  now populated before that entry is recorded, rather than silently discarded
  by the old import-head-only setter. The setter is used only at apply time.
- **Minors:** unsupported-depth filtering and approximation share the final
  normalized recipe's active predicate. A duplicate-XMP regression covers both
  final active states. Tests use `diagnostics::KEY`. Resumed reports read the
  saved recipe instead of reporting an obsolete pending-depth spool copy.

All inputs are generated fixtures or repository test data. No app, Swift files,
real catalog, dependency manifest, lockfile, board, or remote branch is changed.
Original catalog golden remains `d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`.

The initial broad release run had one failure in the new rollback probe: it
expected a skip report, but the deliberately invalid XMP destination also caused
the final catalog scan to propagate an I/O error. The corrected probe checks the
error and the restored pin; its targeted release rerun passed. Initial logs are
preserved as `evidence/lr6e-initial-*.log`. During that run, the preview audit
added a failing regression and the in-memory downsampling fix. A fresh clean
release gate verifies the final source rather than relying on the earlier binaries.

Final source/test commit: `a086b6b8` (preview implementation: `d5b0b2b3`).
### Final clean release gate

The final gate exited **0**: `test=0 e2e=0 clippy=0 fmt=0`.
`gate.sh` uses the requested external target, three Cargo jobs and three Rayon
threads, plus three Rust test threads. It cleans the touched packages in release
mode before testing. No command-line skips were passed.

| Gate | Final result | Evidence |
| --- | --- | --- |
| Release tests: import-lrcat, engine-api, image-core, mask-store, pipeline-cpu, sidecar, merge, ml-depth, tessera-ffi | Pass; zero failures across all targets and doctests | `evidence/lr6e-test.log` |
| Full tessera-ffi release suite | **582 passed, 0 failed, 30 upstream opt-in ignored** | `evidence/lr6e-test-summary.json` |
| Paired synthetic import → saved recipe → CPU render | Pass; explicitly runs the opt-in synthetic test | `evidence/lr6e-e2e.log` |
| Original catalog golden | **d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8**, unchanged | `evidence/lr6e-test.log` |
| Liquify 20 MP brush + preview | Median **11.4 ms**, **p95 26.7 ms**, max 61.1 ms; threshold 250 ms | `evidence/lr6e-test.log` |
| Clippy, release, all targets, `-D warnings` | Pass | `evidence/lr6e-clippy.log` |
| `cargo fmt --all -- --check` | Pass | `evidence/lr6e-fmt.log` |
| Clean before final gate | 2,278 files / 3.8 GiB removed | `evidence/lr6e-clean.log` |

The broad run reports 38 upstream unconditional ignores; the paired synthetic
E2E test is subsequently run explicitly. Every retained opt-in exclusion is
listed with its reason in `evidence/lr6e-ignored-tests.md` (isolated performance
benchmarks, prerequisite model/RAW qualification, or the prohibited real-catalog
acceptance). No new ignores were introduced. Debug-only streaming ignores are
disabled by release mode: both 20k-image memory/time gates passed. Final FFI
inspection took 52.102 s, with 63,846,296 bytes tracked peak heap; open/apply
peaked at 64,983,240 bytes.

The machine-readable summary aggregates harness output, including nested test
subprocesses. Its filtered counts belong to ml-depth/sidecar internal exact-test
subprocesses, not command-line filters on the broad gate. The FFI count has zero
filtered tests. No app/Swift gate was needed because apps/mac was untouched.


## LR-6f — final-stack rebase and combined integration (2026-10-02)

Local-only rebase from `75085381` onto `origin/wp/LR-3-retouch`
`85af5278b1fe297c23993c7147096dbd011d7022` (main `486d069f` plus LR-2,
LR-1, LR-4, LR-5 and LR-3). All 20 LR-6..6e commits remain unsquashed.
No new feature semantics, Cargo delta, board edits, GUI launch, or real catalog
access. The temporary isolated main checkout used only generated test catalogs
and has been removed.

### Every conflict and its resolution

Commit IDs in this list are the original replayed IDs.

- `82c6cd98`: golden.rs retained predecessor digest provisionally pending merged-tree recomputation; matrix retained LR-4/LR-5 prose and every base row, applied LR-6 rows/prose.
- `832673ff`: sidecar/UNMAPPED.md kept LR-1 PointColors and LR-6 LensBlur descriptions.
- `c63d2cda`: golden.rs kept predecessor digest provisionally; historical restoration must not erase LR-3 changes. Recompute after full rebase.
- `d04bab01`: recipe/mod.rs retained both set_imported_masks and set_lens_blur_depth; independent attachments fold into the same Import history.
- `97ea265e`: golden provisional predecessor value; matrix kept LR-4/LR-5 prose and updated LR-6c prose only.
- `5650976d`: schema_version.rs retained LR-2 tests plus LR-6 active/inactive/depth-only tests. schema.rs auto-merged union retained.
- `8eed0461`: recipe/mod.rs kept mask setter plus updated depth contract; schema.rs union of all predicates; mask-store kept predecessor bounded pin reads (MAX_PINNED_BYTES), not unbounded historical depth pins.
- `8a9096e5`: golden remains provisional; matrix kept predecessor LR-4/LR-5 prose and adopted LR-6d prose.
- `6b2df335`: lrcat.rs includes both mask and depth test modules.
- `aaf46a64`: recipe/mod.rs kept both setters and final depth docs; schema.rs retained all predicates with narrowed LR-6 extension predicate; mask-store unified duplicate bounded put/remove methods and retained pinned_revision, added removal lock; changes.rs removes both lanes pins on forget; lrcat.rs retained both callback traits and entry points, added combined apply_with_resolvers (no preexisting options struct), depth preparation then transactional mask apply/publication with depth rollback, shared resumed diagnostics.
- `75085381`: matrix retained predecessor LR-4/LR-5 prose and final LR-6e prose; all base rows preserved.

The final matrix audit compares all 118 predecessor row keys and complete row
text: only LensBlur and DepthMapInfo differ. The first-lane schema checklist and
shared approximate guard remain the main/LR-7 implementations. LR-6 adds its
extension predicate and field checks; all other lanes' predicates/tests remain.
LR-7's shared finish creates the one Import entry; both resource setters enrich
that entry without creating another.

There was no shared resolver-options struct. Both public single-resolver entry
points remain, delegating to `apply_with_resolvers(mask_resolver, depth_resolver)`.
The combined implementation prepares depth, then performs LR-5's transactional
mask apply and publication; a mask/publication failure restores the previous
depth pin too. Image forgetting removes both classes. Mask slots remain bounded
at 256 slots and 256 MiB total per image; depth remains one slot, at most 256 MiB
including its header/checksum. The existing pin revision/read bounds are retained.

### Golden justification

Recomputed the original 2,000-image golden from the merged source; it is
`87d28d71460e64ad1034fd0a5dc408a20a0452b7d37ccfd6f2a00ada8db3c0d5`.
It equals LR-3's value because the original fixture has **no active Lens Blur**.
This is a measured result, not a choice of conflict side. Main is `486d069f`.
A temporary identical probe compiled on main and on the merged tree serialized
every ImportedImage with only recipe.image_id normalized, as in the golden.
Raw JSON substring comparisons found exactly these changed original rows:
`catalog_id = 1003 + 10*k`, for every integer `k` from 0 through 199 inclusive
(1003, 1013, ..., 2993; the complete explicit list is in
`evidence/lr6f-golden-diff.txt`). Every such row uses structures.lua. LR-3
translates its explicit-source heal spot, adds the retouch history delta and
shared approximate diagnostics, consumes the top-level crs:RetouchInfo retained
fallback while retaining exact lrcat_develop_source, and writes schema 4.
Their inactive LensBlur and DepthMapInfo behavior is unchanged. All remaining
1,800 complete image rows are byte-identical to main.

A second golden preserves the original and adds active coverage by replacing
only the develop text of four synthetic rows (history source cells stay as
originally generated):

- 1004: active LensBlur with BlurAmount=37 and FocalRange='10 20 60 80'. LR-6
  adds translated focus/optics, pending depth state, per-field approximation and
  regeneration diagnostics, schema 4 and the shared Import history entry.
- 1005: active LensBlur with BlurAmount=37 and BaseRawDepthTable='synthetic-depth'.
  LR-6 attaches opaque depth metadata and pending regeneration to the existing
  Import entry, replaces top-level retained fallback with exact source plus
  shared info diagnostics, removes the metadata-only warning, and writes schema 4.
- 1006: standalone DepthMapInfo with DepthSource=1. Entire serialized image is
  byte-identical to main, including source, schema and history. The develop warning remains main's
  LR-2 metadata-only wording.
- 1007: inactive LensBlur plus DepthMapInfo with DepthSource=1. Entire serialized
  image is byte-identical to main. Both exact main byte strings are checked in
  `crates/import-lrcat/tests/data/lr6f-main-inactive.json` and asserted without reserialization.

Thus the augmented fixture changes exactly the 200 LR-3 IDs above plus 1004 and
1005 relative to the same augmented fixture on main. No other row changes.
The augmented digest, recomputed after the baseline cleanup, is
`141018bf3d1b53071354c60090993dc0799d7f7fa35c0c8685cbbb55349ef6e0`.
The digest function and its normalization are unchanged.

One existing LR-1 compatibility pin also necessarily changes: only `pending`,
whose literal is `s = { LensBlur = { Active = true }, RetouchInfo = { 'opaque' } }`.
Its opaque retouch stays untouched; active blur gains LR-6 depth pending state,
shared regeneration info and schema 4 (11,717 → 12,169 bytes; digest
`2678608afc35568412c6c26bce7acae5815954ded18022c41a587689ac81d444`).
All nine other LR-1 compatibility rows retain their predecessor pins, including
the LR-3-adjusted `structures` row. The failed-then-recomputed values are in
`evidence/lr6f-golden-recompute-clean.log`.

The first post-baseline targeted run reused artifacts from the isolated main
build in the shared target: it reported main's original golden. Those digest
results are superseded. The accompanying depth warning was subsequently confirmed
as a separate semantic merge interaction (documented below), not dismissed as a
cache issue. The isolated checkout was removed, then
all touched release packages were cleaned again (469 files, 2.9 GiB); the clean
rebuild restored the measured merged-tree digest and exposed only the intended
new active golden/pending compatibility pin changes.

### Combined test

`lrcat::combined_tests::lr6f_all_lanes_one_apply_both_resources_and_both_absent`
uses generated fixture photographs, an invented develop row covering all seven
lanes, and caller-supplied grayscale PNG masks/depth. It checks schema 4, one
Import-authored entry, replay, diagnostics via entries(), both pin contents and
bounds, the CPU graph with real brush retouch and pinned-mask composition, and
stored-depth reuse with a deliberately incompatible fallback (no model inference).
It also covers joint publication rollback and reimport with both resolvers absent,
checking both regenerated diagnostics and removal of both obsolete pin classes.

Gate results and final commit references follow after validation.


Additional semantic merge conflict found by the full tests: LR-2's metadata
hook re-added its generic DepthMapInfo warning after LR-6 had translated active
depth. Its fallback now defers only when the final normalized LensBlur is active;
inactive/standalone fallback bytes remain exactly main's behavior. This is a
resolution between existing lane contracts, not a new translation feature.
The older LR-6 standalone/inactive warning baselines (two rows) and the duplicate
XMP expectation now use main's existing LR-2 wording; their recipe-byte pins
remain unchanged. All 14 LR-6 import regressions passed after the correction.
The augmented golden was recomputed again after row 1005 correctly lost the
metadata-only report warning; its digest is unchanged because the established
digest covers ImportedImage, not the enclosing report warnings. No serialized
image row changed in this correction. The direct develop tests cover warnings.


### Gate commands and environment

All commands run locally; there are no command-level test exclusions. Existing
model/benchmark/real-catalog ignores remain opt-in; no model inference was enabled.
Repository RAW fixtures use the existing `fixtures/raw` link. The new combined
test is an ordinary, non-ignored test. Generated Swift/CTesseraFFI bindings are
included with the test commit; no app feature or dependency change is introduced.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-4-parametric-masks"
export CARGO_BUILD_JOBS=4
export RAYON_NUM_THREADS=4
export RUST_TEST_THREADS=4
export MACOSX_DEPLOYMENT_TARGET=15.0
cargo clean --release -p engine-api -p image-core -p import-lrcat -p mask-store \
  -p ml-depth -p pipeline-cpu -p sidecar -p tessera-ffi -p blake3
cargo test --release --no-fail-fast -p import-lrcat -p engine-api -p image-core \
  -p mask-store -p mask-ai -p pipeline-cpu -p pipeline-gpu -p brush -p sidecar \
  -p merge -p export -p previews -p tessera-mcp -p tessera-ffi
cargo clippy --release --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
(cd apps/mac && ./build-ffi.sh)
tools/orchestrate/swift-gate.sh
(cd apps/mac && swift build -c release --product Tessera \
  -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors)
```

The initial broad run had seven failed assertions across three targets
(lr6_lens_blur, translation_matrix, tessera-ffi's combined test), all due to the
LR-2 metadata warning interaction and stale pre-main warning expectations.
The subsequent focused LR-6 import suite and the complete new combined test pass.
Initial fmt also flagged the newly substituted golden digest's line wrapping;
`cargo fmt --all` corrected it. Initial workspace clippy and Swift gate pass.
Those initial logs are retained, and the final clean run below supersedes them.


The semantic LR-2/LR-6 resolution and conflict formatting were folded into the
replayed `aaf46a64` integration commit (`209f5f6e`) by editing that rebase step;
no original commits were squashed. All pending source/test/evidence file hashes
were verified unchanged across the amendment and restoration. The rebase retains
exactly 20 original lane commits, followed by one LR-6f test commit and one docs
commit. The initial strict Swift release build passed, but its linker reported
one cached Blake3 NEON object built for macOS 26.2 while linking macOS 15. The
final clean also rebuilds blake3 with `MACOSX_DEPLOYMENT_TARGET=15.0` (the same
setting used by build-ffi.sh), to avoid carrying that incompatible cache artifact.


### Final source and completed Rust/Swift test gates

Rebased lane tip: `3cc5e644` (20 individual commits over `85af5278`).
Single new test/bindings commit:
`0b3b8074f75601e4511db1816495e362458c4419`.
`evidence/lr6f-history.txt` maps every original commit to its rebased commit and
verifies each requested co-author footer. Source/test/bindings remain unchanged
through the final gates. Final clean removed 3,091 files / 7.2 GiB.

| Final gate | Result | Evidence |
| --- | --- | --- |
| All 14 requested packages, release, no command-level exclusions | **Pass: 1,844 passed, 0 failed, 60 existing opt-in ignored, 0 filtered** | `evidence/lr6f-test.log`, `evidence/lr6f-test-summary.json` |
| Combined all-seven-lane apply → pins → CPU render → rollback → absent resolvers | **Pass**, included in the full FFI suite | `evidence/lr6f-test.log` |
| Original and augmented goldens; main inactive-depth bytes; LR-1 compatibility; full matrix | **Pass**, included in full import-lrcat suite | `evidence/lr6f-test.log` |
| Workspace clippy, release, all targets, `-D warnings` | **Pass** | `evidence/lr6f-clippy.log` |
| `cargo fmt --all -- --check` | **Pass** | `evidence/lr6f-fmt.log` |
| `apps/mac/build-ffi.sh` | **Pass**, regenerated arm64 bridge/archive | `evidence/lr6f-build-ffi.log` |
| Swift gate | **SWIFT GATE OK**; 920 XCTest tests, 3 skipped, 0 failures; 5 Swift Testing tests pass | `evidence/lr6f-swift-gate.log`, `evidence/lr6f-swift-test-detail.log` |
| Strict release product build, complete concurrency, warnings as errors | **Pass**; no warnings, including no deployment-version linker warning | `evidence/lr6f-swift-release.log` |

The Rust summary counts the last (top-level) harness result for each Cargo target,
so helper subprocesses are not double-counted. Every retained ignore appears in
`evidence/lr6f-ignored-tests.txt`; none was introduced by LR-6f. Both release
streaming memory/time tests ran and passed. Repository RAW fixtures ran; real
catalog acceptance and actual model inference remain opt-in. No GUI was opened.
Cargo manifests and Cargo.lock have zero delta against both `85af5278` and the
original `75085381` tip. No board.json changes or remote push occurred.


All six final statuses are zero in `evidence/lr6f-gate-status.log`:
`test=0 clippy=0 fmt=0 build_ffi=0 swift_gate=0 swift_release=0`.
The final strict release product build completed in 167.35 s with no warnings.
The rebuilt macOS 15 Blake3 object removed the initial deployment-version linker
warning. No source or generated-binding changes followed the final gate.
The final docs/evidence commit is the commit containing this appendix; resolve
its full hash with `git log -1 --format=%H -- tools/orchestrate/wp/LR-6/HANDOFF.md`.

## Restack without LR-5

Binding A-ROUND2 RESTACK ruling: approved LR-3 and LR-6 are replayed without
rejected LR-5. All 22 original LR-6 commits remain in order; this is the one
extra docs commit. The approved RED-before-fix sequence is retained. No new
behavior beyond the required removal of LR-5 is introduced. Earlier sections
describe the approved pre-restack tree; this section supersedes their stack,
resource-availability, and final-gate statements.

### Item-by-item review disposition

- DONE — LR-3 is based on approved LR-4 5e0633e1; LR-6 is based on the new
  LR-3 restack tip (including its one handoff commit). Rejected 04217312 is
  not an ancestor. Old branch tips remain unchanged.
- DONE — keep LR-3 behavior: export/print retouch callbacks remain; only
  LR-5 mask-support plumbing is absent. Covered by
  pixel_and_file_export_use_registered_retouch_or_error and
  lr3f_combined_import_preserves_history_diagnostics_and_spot_exterior.
- DONE — keep LR-6's shared pin functions: mask-store's final source,
  image-core depth source, and ml-depth source match approved 43a508e3
  byte-for-byte. ecfbee51's replay carries the pin support formerly inherited
  from LR-5. Covered by imported_depth_survives_cache_eviction_and_user_edits,
  lr6e_reimport_replaces_one_pin_per_image_and_user_edits,
  lr6e_unresolved_reimport_removes_only_the_images_previous_pin, and
  lr6e_preview_reads_full_resolution_imported_depth_without_inference_or_writes.
- DONE — remove AI-mask imports: no adobe_ai, lrcat_masks, LrcatMaskResolver,
  mask_support, or combined mask resolver references remain in Rust sources.
  The depth-only public resolver retains its original signature. Depth apply,
  rollback, reimport, and prune are covered by
  lr6e_apply_pins_depth_before_user_edit_reimports_and_deletes_with_image.
- DONE — drop only LR-5 portions of the combined test: synthetic AI-mask row,
  resolver/hook, AI pin and rollback checks, and LR-5 diagnostics removed;
  LR-1/2/3/4/6/7 plus resolved/pending depth and rollback retained in
  lr6f_all_lanes_one_apply_depth_resource_and_absent.
- DONE — regenerate Swift bindings from the restacked Rust library. The final
  build-ffi run must leave tracked bindings unchanged.
- DONE — original commit order and subjects preserved; no squash. Each new
  replay commit ends with the required Opus trailer. LR-3's first four retain
  their original Fable credit with the required Opus credit appended.
- DONE — Cargo.lock and board.json unchanged from 5e0633e1. All newly edited
  tests use synthetic data. Build logs remain outside Git; no private pixels,
  names, or paths added. No foreground GUI launch or system-settings changes.
- DEFERRED BY A — LR-12 cleanup: full regeneration reason equality and exact
  rollback error kind remain the explicit post-merge cleanup lane. LR-3's
  six cleanup notes remain there too; RESTACK preserves approved behavior.
- OUT OF SCOPE — LR-5b B1/B2/B3 and M1–M9 belong to the rejected lane's later
  replacement. Other A-ROUND2 lane findings (LR-8/9/11, B5, ENG) are not
  changes authorized by this RESTACK assignment.

### Range-diff verdicts

Thirteen LR-6 patches are equal. Nine differ:
- fcaf3fb6: LR-5 matrix context absent; LR-6 changes identical.
- 3d22622a: insertion context changes because set_imported_masks is absent.
- a72e4c09: LR-5 matrix context absent; LR-6 changes identical.
- ecfbee51: retain shared pin functions/get support formerly inherited from LR-5;
  history-method context changes because set_imported_masks is absent.
- 751acbef: LR-5 matrix context absent; LR-6 changes identical.
- 4e7a0c06: depth-test module insertion context no longer includes LR-5 tests.
- 209f5f6e: depth-only apply/prune replaces LR-5 combined plumbing; preserve
  mutable import recipe, schema/depth behavior, and resolver signature.
- 3cc5e644: LR-5 matrix context absent; LR-6 changes identical.
- 0b3b8074: remove only LR-5 combined-test sections and regenerate bindings.

### Final gate evidence

LR-3 tip: b75dd20f1d4775044800340c5950f71f0d9f3424. LR-6 tested implementation tip: e06124abfbe71d68b2b4d8a81195f9684e2151cd.
This final documentation-only commit follows that tested implementation.

Environment: PATH includes the Rust toolchain; CARGO_BUILD_JOBS=4,
RAYON_NUM_THREADS=4, RUST_TEST_THREADS=1. CARGO_TARGET_DIR uses the fresh
external RESTACK directory. Explicit release cleaning of affected packages
removed 428 files / 1.2 GiB before the final gates.

| Gate | Result |
| --- | --- |
| LR-3 import-lrcat + engine-api release tests | 314 passed, 0 failed, 0 ignored |
| LR-6 full requested release test command, retry | 1,912 top-level passed, 0 failed, 67 ignored; two additional restart subprocess probes passed |
| Workspace Clippy, all targets, -D warnings | PASS |
| cargo fmt --check | PASS |
| apps/mac/build-ffi.sh | PASS; regenerated bindings have no tracked drift |
| tools/orchestrate/swift-gate.sh | SWIFT GATE OK; 917 XCTest passed, 3 skipped, 0 failures, plus 5 Swift Testing passed |
| Strict release Tessera build | PASS; strict-concurrency=complete and warnings-as-errors |
| Import goldens | Approved golden source/data byte-identical to 43a508e3; all golden checks passed |

Rust command (no command-level exclusions, same command for both attempts):

```sh
cargo test --locked --release -p import-lrcat -p sidecar -p engine-api \
  -p image-core -p pipeline-cpu -p pipeline-gpu -p filters -p previews \
  -p export -p tessera-ffi -p tessera-mcp
```

The raw log sums to 1,914 passing reports because the two sidecar restart tests
launch their own exact-test subprocesses; each child reports one pass and nine
internally filtered tests. The parent suite also counts those same probes.
No tests were filtered or excluded by the top-level gate command. Existing
source-level ignored tests remain ignored. Swift's three skips are the opt-in
20k-file measurement and two smart-preview checks requiring an explicit RAW
fixture setting.

First Rust attempt: 1,111 passed, 1 failed, 37 ignored before Cargo stopped.
Only previews::tests::raw_without_jpeg_is_rendered failed: 3.181422375 seconds
against its unchanged 3-second local timing assertion; image-content assertions
passed. The entire requested command was rerun after the other gates, without
source changes, CI bypass, exclusions, or relaxed bounds, and passed. This
transient timing failure remains part of the evidence, not an omitted run.

Goldens explicitly passed: synthetic_catalog_output_including_retained_source,
streaming_matches_import_and_plan_json_is_byte_identical, and
lr6f_active_blur_and_inactive_depth_catalog_golden. No golden was re-pinned.

Range-diff summary before this one documentation-only commit:

```text
 1:  8ad5dce8 =  1:  fa82141a test(LR-6): expose missing Adobe lens blur and depth bookkeeping
 2:  f339a7fd =  2:  3c3bb084 docs(LR-6): hand off RED evidence and fidelity blockers
 3:  aee04699 =  3:  86d60944 feat(LR-6b): approximate Adobe lens blur controls and depth resources
 4:  fcaf3fb6 !  4:  493dff80 test(LR-6b): enforce approximate mappings and synthetic depth rendering
 5:  9e709b68 =  5:  06b2e52d docs(LR-6b): record per-field approximation contract and green gates
 6:  247aa1a6 =  6:  17fc6323 test(LR-6c): expose depth activation, history, rendering and field diagnostics regressions
 7:  3d22622a !  7:  24b96b69 fix(LR-6c): honor active blur, stored depth and native focus without extra history
 8:  ced4ae52 =  8:  0ef4992a fix(LR-6c): satisfy diagnostics regression clippy check
 9:  a72e4c09 !  9:  433059a8 docs(LR-6c): record review fixes, passing behavior gates and remaining failures
10:  052a900f = 10:  db153a93 test(LR-6d): pin diagnostics conversion, untouched depth, schema and resource history
11:  ecfbee51 ! 11:  ff2f238e fix(LR-6d): adopt shared diagnostics, preserve inactive bytes and pin imported depth
12:  751acbef ! 12:  63126bd6 docs(LR-6d): record conversion audit and full gate results
13:  c2dd9b9f = 13:  66d2fbd0 test(LR-6e): cover native schema, explicit regeneration and depth ownership
14:  4e7a0c06 ! 14:  a4b93234 test(LR-6e): exercise import apply and bounded depth lifetime
15:  09457c20 = 15:  583242ef test(LR-6e): check normalized XMP and failed publication rollback
16:  209f5f6e ! 16:  0e5a483e fix(LR-6e): attach bounded depth during import and preserve native schema
17:  ce3fa9e0 = 17:  6e593028 test(LR-6e): require preview reuse of imported full-size depth
18:  fee0df10 = 18:  da2f15b0 fix(LR-6e): reuse pinned depth at preview pyramid levels
19:  d4913121 = 19:  bdc41cc4 test(LR-6e): accept propagated I/O failure in rollback probe
20:  3cc5e644 ! 20:  dabca97a docs(LR-6e): record review fixes, bounded depth lifecycle and release gates
21:  0b3b8074 ! 21:  470a07f7 test(LR-6f): verify all-lane import with mask and depth resolvers
22:  43a508e3 = 22:  e06124ab docs(LR-6f): record stack resolutions, golden evidence and final release gates
```

Remaining work: no RESTACK implementation or gate work remains. Machine A owns
merge review and the explicitly deferred cleanup/replacement lanes. The two new
branches are ready for the authorized non-force push; old branches are unchanged.
