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
