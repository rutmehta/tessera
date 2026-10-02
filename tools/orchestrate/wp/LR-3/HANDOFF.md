# LR-3e: diagnostics conversion and retouch review fixes

Local branch `wp/LR-3-retouch`, Machine B. Rebased all 17 existing lane commits
without squashing from `f55672b8` onto `38684d9b` (`origin/wp/LR-DIAG`), which
includes LR-SCHEMA `02ae8196`. `origin/main` did not contain LR-DIAG at fetch.
The two rebase conflicts were in `translation_matrix.rs`: LR-DIAG's complete
shared guard wins; the lane-local approximate guard is gone. All 107 base matrix
rows remain; only `RetouchAreas` and `RetouchInfo` change.

## Review items

1. **B2:** the import regression now includes `Exposure2012=0.75` and each retouch
   alias, asserts the translated exposure and exactly one replay-valid history
   entry, and pins decoder provenance `Import XMP` / `Author::Import { source:
   "xmp" }`. The importer uses that provenance. It remains one consolidated edit.
2. **Shared diagnostics:** removed the private TODO shim and its literal-key
   writer. `retouch::translate` calls `crate::diagnostics::push_approximate` with
   field `/settings/locals/retouch`, matching both matrix rows. Readers use
   `diagnostics::entries`. No lane-local matrix guard remains. Source stays
   retained and approximate translations produce no warnings.
3. **N1:** only the retouch solve is downsampled. Its nonzero pixel deltas are
   lifted into the original scene-linear image's matching box cells, before
   Detail/Tone. Original crop, depth plane, and output scale are retained; every
   other stage follows the no-spot sampling order. Unchanged cells are not
   rewritten. Tests cover odd image edges, scales 2 and 4, real heal and clone,
   default Detail plus contrast/clarity/texture/vibrance, and bit identity outside
   the spot plus the stated margin: radius 0.0625 of the short image edge, 32
   input pixels for feather/downstream filter support, and two sampling cells.
   A separate injected 3x3 solve test pins target resolution and exterior bits.
   This is a locality test for those bounded operators; global dehaze statistics
   can legitimately respond to an edited input and are not claimed local.
   The continuation's full gate found a remaining MCP scale switch: spot recipes
   selected the requested size while empty recipes selected a different size.
   `48a00d45` adds RED exterior-bit tests for both MCP source paths;
   `c967a490` makes requested-size selection independent of spots. Both paths
   now pass exterior identity on textured 512x384 input at 128x96, with a
   radius 0.02 plus 32 input pixels and two output-cell margin. Graph comparisons
   use equivalent cold caches because warm WB tiles are intentionally f16.
   The requested-resolution test uses each path's own sampling-order reference:
   graph stages run at the requested pyramid level; direct RGB stages retain
   their input resolution and only the spot solve is reduced.
4. **N2:** one lazily cached CPU fallback renderer per session retains its RGB
   memo and Upright analysis across frame snapshots. Current process/profile,
   depth, and caller-owned capability state come from the current request.
   Replacing the retouch capability resets its fallback; the cached template has
   a separate empty OnceLock to avoid a self-referential Arc. The internal
   repeated-frame test proves one L0 solve; the real Metal-selected session test
   allows initial admission/fallback analyses but requires no subsequent solves.
5. **N3:** equality tests assert the expected written version before normalizing
   schema_version. The single retouch v4 predicate checks both current settings
   and `history.base`; bumped-only-when-present tests cover each. The base schema
   constant remains 3. No Cargo manifest or lockfile changes.

## Interactive timing: one clone + Upright Level

Same new test `lr3e_gpu_spot_upright_frames`, actual `GpuStageOp`, CPU brush bridge,
768x512 synthetic slanted-line RGB, L2 preview, five manual rotations 0–4 degrees.
Unoptimized test profile, jobs=3, Rayon=3. Baseline used the pre-fix
`image-core/src/render.rs`; the fixed source was restored immediately afterward.
These are shared-machine samples, not an isolated release performance claim.

| Frame | Before (ms) | After (ms) |
| --- | ---: | ---: |
| Cold 0 | 2595.03 | 5796.35 |
| 1 | 1077.96 | 315.53 |
| 2 | 1246.04 | 271.86 |
| 3 | 1323.13 | 245.19 |
| 4 | 1099.29 | 240.29 |

Steady-frame median: 1172.66 → 258.52 ms (about 4.54x faster in these samples).
L0 solves: `[2,3,4,5,6]` → `[2,2,2,2,2]`; the two cold analyses belong to admission
and the CPU fallback. The small 192x160 direct `run_m2` test likewise changed
`[1,2,3,4,5]` → `[1,1,1,1,1]`, with steady frames ~201–277 → ~81–82 ms. No timing
threshold was added or changed. The cold after sample was slower; do not claim a
cold-start improvement.

## Coordinator merge rules

- **LR-7d owns the first-merged-lane schema checklist.** At the initial rebase its
  fetched remote did not yet contain the LR-SCHEMA checklist files. This
  standalone LR-DIAG-based branch retains the necessary import schema expectation/golden and strengthened
  equality checks so it can be tested before that landing. At rebase onto the
  LR-7d landing, drop these lane copies and keep LR-7d's shared tests/pins; retain
  only this lane's retouch predicate and bumped-only-when-present feature tests.
  Recompute the combined import golden after integrating all translators.
- **LR-2:** keep `validate_settings_with_retouch` alongside `validate_domain`;
  taking only LR-2's validation rejects every spot recipe. Apply retouch to the
  white-balanced buffer, then LR-2's pre/post-curve bindings. Detail hash includes
  retouch; Tone hash chains monochrome. The rgb_render monochrome block must use
  CPU ops. Lua ordering: `parse_inner(…, false)`, then `lr2::lua`, then
  `geometry::apply`, and **`retouch::translate` last**.
- **LR-6:** recompute `golden.rs`; do not choose either lane's hash. The standalone
  LR-3e digest is `87d28d71460e64ad1034fd0a5dc408a20a0452b7d37ccfd6f2a00ada8db3c0d5`,
  changed because retouch rows now use decoder provenance plus shared diagnostics.
- Keep LR-DIAG's shared guard and all base matrix rows at every merge. This lane
  makes no edits in another lane's worktree and does not integrate their code.

## Verification

Final clean-build verification on source `c967a490`: the complete 12-crate gate
passed in 4304.505 s (1,563 top-level passed, zero failed, 60 existing ignores;
two additional child-process test executions passed). Workspace/all-target
clippy `-D warnings` passed in 51.123 s; fmt passed in 2.026 s.
`export_batch_does_not_starve_slider_drag` passed in the broad run and serially:
120/120 L2 frames, 4.7 ms render p90, all five exports succeeded.

**Remaining performance limit:** Liquify's 20MP test passed in the final broad
run but failed the required serial rerun: p95 266.6 ms against the unchanged
250 ms limit. Earlier broad/serial samples were 467.2/434.6 ms. No threshold was
changed. The initial gate also exposed the MCP issue fixed in `48a00d45` /
`c967a490`; the entire clean-build gate was rerun after that fix, and both new
MCP exterior-bit tests passed. This is not an all-gates-green handoff because
of the serial Liquify result.

Exact commands, counts, elapsed times, initial failures, and final serial
measurements are in [LR-3e-EVIDENCE.md](LR-3e-EVIDENCE.md). The historic sections
below describe older lane states; this LR-3e section supersedes their shim,
early-scale, and gate-skip notes. Repo RAW fixtures are authorized for LR-3e;
the full broad run uses no `--skip` filters. No personal Lightroom catalog, Swift gate, app launch, push, or
board.json write.

---

# LR-3d: retouch review follow-up

Local-only lane `wp/LR-3-retouch`. Do not force-push from Machine B.

The authorized rebase replayed all ten lane commits without conflicts onto
`c7254291` (including LR-SCHEMA `02ae8196`). No commits were squashed.
The detached timing baseline is `87536669`, the `origin/main` observed when the
comparison worktree was created; its intervening merge is B5-42/42b accessibility.
The two base commits have identical Rust crates and Cargo manifests/lockfile.

## Current commits and checks

- `36a30ea4`, `bb4d3353`, `a9845aed`, `6b54a3ee`: RED regressions.
- `6693781e`: stage order, CPU host equality, union masks, reduced-resolution
  solves, source/history/diagnostics and conditional-schema fixes.
- `72be4015`: preserve inactive-spot identity in reduced-resolution previews.
- The final `docs(LR-3d):` commit records verification below and in
  [LR-3d-EVIDENCE.md](LR-3d-EVIDENCE.md).

Focused import, schema/journal, geometry, MCP and real-brush integration tests
pass. The final twelve-package functional gate passed: 1,514 passed, zero
failures; clippy with warnings denied and fmt passed. RAW-fixture tests and the
two isolated timing tests were filtered from that broad gate. Serial Liquify
latency failed on both base (p95 344.2 ms) and tip (375.4 ms), against 250 ms.
The RAW-dependent export/slider comparison remains blocked by the synthetic-only
constraint; no exception was received. Blocker 4 is therefore only partially
resolved. Full commands, exclusions and timings are in the evidence file.
Earlier LR-3/LR-3b/LR-3c gate reports are historical, not evidence for this tip.

## Rendering contract

The caller-owned `pipeline_cpu::RetouchRenderer` trait remains the dependency
boundary. Engine assembly passes `Arc::new(brush::render_retouch)`; no dependency
edge, manifest or lockfile changed. Missing renderers and unsupported enabled
operations fail explicitly, including on preview cache hits.

All three Develop paths apply retouch to scene-linear pixels **before Detail,
Tone, colour and local adjustments**. Retouch participates in the Detail cache
key; Locals hashes only local adjustments. Native GPU-selected sessions use a
CPU f32 chain for retouch, including downstream operators, and tests compare
all output bits to the CPU-selected session. Adobe process selection and DCP
wrappers survive that host fallback. This proves backend routing equality, not
Adobe render equivalence.

One spot uses one union mask and one immutable source/destination snapshot.
Separate paths cannot compound spot opacity or clone pixels modified by an
earlier path in that same spot. Heal solves the union once using the existing
Poisson kernel. Separate spots still execute in recipe order.

## Approximate import and schema

`RetouchAreas` and `RetouchInfo` are **approximate**, not translated: recipe
fields are populated, exact Lua/XMP source remains in `lrcat_develop_source`,
and each nonempty successfully mapped key gets an info diagnostic with an
`approximate: ...` reason and zero importer warnings for that key. Unsupported
keys keep their existing diagnostics. Empty aliases do not get approximate
entries. Import produces one history entry with `Author::Import`, including
both the decoder's settings and retouch; replay validation passes.

The matrix guard checks field presence, source retention, an info/approximate
entry for that key, and no warnings. Four separate negative tests remove each
required condition. All inherited main rows remain in the matrix.

LR-DIAG was absent from main when this lane worked. The sole temporary writer is
private `retouch::push_approximate(recipe, adobe_key, field, lane, reason)`, marked
with the exact LR-DIAG TODO. It appends and deduplicates entries without replacing
another key's list or the diagnostics object. At the LR-DIAG rebase, replace the
private shim with `use crate::diagnostics::push_approximate;` and use
`diagnostics::entries(recipe)` in the matrix reader. Do not invent another
channel. LR-DIAG/Machine A owns B5-46's separate “Approximate translations” report
group; no Swift/report UI change or gate is part of this lane.

This was the first real `V4_FEATURE_PREDICATES` entry on main:
`("retouch", |r| !r.settings.locals.retouch.is_empty())`. The base
`RECIPE_SCHEMA_VERSION` remains 3. Retouch recipes serialize as 4; other imports
stay 3. The predicate harness and FFI journal test cover the bump. Import schema
expectations and the affected synthetic golden were updated, and sidecar/merge
round-trip equality ignores only the projected schema version. The schema 4
bump remains sticky on re-save, as LR-SCHEMA specifies.

Final 2,000-image synthetic golden:
`7022e432ed77c0c42227de06f331090e8a43d4e06ca4749959f136a26b659763`.
Its 200 retouch rows now retain source, carry info diagnostics, have one import
history entry, and write schema 4. Recompute on LR-6 merge; do not take a side.

## Coordinate and encoding boundary

Coordinates are normalized to the **current Develop input frame**, before the
common lens-distortion/output geometry map and crop. For CFA RAW this is the
unrotated active image (masked sensor margins removed); file EXIF orientation
is applied by presentation/export afterward. Rendered RGB may already be upright
because its decoder consumed EXIF orientation before Develop. Radius is relative
to input width; source offset is explicit source minus the first destination.

This convention, Adobe feather/flow behavior and the healing solver are not
verified against an Adobe-rendered synthetic chart or public DNG+XMP pair.
They remain approximate with exact source retained. No user catalog is evidence.
Tests pin supported crop rotation plus a synthetic lens profile and EXIF 5–8
export behavior. Recipe-level `geometry.orientation` remains explicitly
unsupported; this lane does not silently implement it as file orientation.

The LR-3c allow-list rejected LrC 11+ fields including `Seed`, `CenterValue`,
`MaskDigest` and `Mask/Circle`; those spots stayed retained. LR-3d now accepts
`Seed`/`MaskDigest` as provenance (preserved exactly in source), and plain
`Mask/Circle` geometry with `CenterX`, `CenterY`, and `Radius`, as approximate.
`CenterValue`, other unknown mask fields, unsupported circle encodings,
variable-radius/pressure commands, inverted/subtractive masks, unresolved
sources, OffsetY-only sources, unknown methods, cloud/generative/remove modes,
and conflicting nonempty aliases remain retained atomically. This is not a
claim to accept every LrC 11+ spot.

## Performance and remaining PERF work

CPU scaled renders reduce the scene-linear buffer before active retouch.
Disabled/zero-opacity spots keep the no-retouch sampling order and remain an
identity; a renderer is still required for every nonempty list. Image-core M2
already supplies target-level pixels; MCP now honors the requested retouch
preview edge instead of solving its fixed larger preview. JPEG fidelity already
builds its target-sized input. Scaled export and library previews inherit the
CPU reduction. Full-resolution export still intentionally renders full detail.
Depth planes are reduced consistently when supplied to the scaled CPU path.

The CPU bridge transfers ownership of planar storage instead of cloning the full
frame. Brush output reuses one tile buffer rather than reading the same raster
tile per pixel; spot application touches only its dirty rectangle. Raster
snapshots share tile storage, and one union heal avoids per-stroke solves.

**PERF follow-up remains:** the detail/loupe path retains the full sensor to keep
remote clone sources available and can re-develop/re-solve the full RAW frame on
each request. Add source-aware windows and a bounded per-level prefix/retouch
memo. Further reduce raster/working-frame storage and Poisson scratch allocation,
and add cancellation during long solves. No low-latency loupe or Adobe solver
parity claim is made by this lane.

## Explicit errors and construction-site limits

These sites still reject spots rather than render; they are acceptable deferred
integration work, not silent drops:

- `apps/tessera-cli/src/media.rs` thumbnail and scene-linear paths, and CLI export
  when no caller-owned renderer is supplied.
- `crates/filters/src/camera_raw.rs` camera-raw smart filter.
- Public `export::render_pixels` without a retouch context; callers that have a
  renderer use `render_pixels_with_retouch`.
- Camera-linear smart previews, including smart-preview thumbnails: the prefix
  admission guard requires the original **before** renderer registration.
- Retouch file export combined with AI masks, raw denoise or depth hooks, and the
  standalone managed GPU export wrapper. Ordinary registered file/print/HDR
  exports route through the CPU retouch context.

The LR-3c construction-site audit remains useful, but its smart-preview
registration must not be read as successful spot rendering from a proxy.

## Coordinator merge notes

- `lua_develop.rs`: keep `parse_without_retouch`; preserve LR-4's
  `masks_translated` hook after it when merging that lane.
- LR-DIAG: replace the marked shim/reader with the shared helper/entries API.
- LR-6 `golden.rs`: regenerate combined output; do not choose either digest.
- Local changes only. No board.json, Swift gate, app launch, push, or personal
  catalog access. All new fixtures are synthetic. Every LR-3d commit ends with
  `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>`.
