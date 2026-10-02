# LR-3f: rebase-only pre-stack integration

This section is current; the LR-3e and earlier sections below are historical.

Local Machine B branch `wp/LR-3-retouch`; original tip `244ad145`, rebased
without squashing all 25 lane commits onto `origin/wp/LR-5-ai-masks`
`04217312` (main + LR-2..2f + LR-1..1c + LR-4..4f + LR-5..5b).
No features or Cargo dependency changes. No push, board.json write, GUI launch,
or personal catalog access. Tests use generated catalogs and authorized repo RAWs.

## Conflicts and resolutions

The following records every stopped replay and each conflicted file (original
commit IDs identify the replay, not the rewritten hashes):

- `65f69eb1`, `import-lrcat/src/lua_develop.rs` and `src/xmp.rs`: retain
  `parse_without_retouch`, delegating to `parse_inner(..., false)` for Lua.
  Preserve the base XMP mask-audit decoder. Lua applies LR-2, then geometry,
  then retouch last, then the shared `finish` and validation. Direct XMP likewise
  translates retouch before `finish`. The fetched base has no `masks_translated`
  symbol; its successor `decode_with_mask_audit` path remains intact.
- `9bdffcad`, `export/src/lib.rs`: preserve both caller-owned mask support and
  retouch fields/defaults. Preserve both public wrappers through a common
  resource-aware pixel renderer. FFI print passes both capabilities. Retouch's
  existing explicit unsupported combinations remain unchanged.
- `9bdffcad`, `image-core/src/render.rs`: retouch-aware validation alongside
  LR-2 `validate_domain`; retain monochrome stage condition and both stage
  operations pending the later LR-3d ordering commit.
- `9bdffcad`, `image-core/src/resident_render.rs`: retain both Point Color and
  retouch predicates selecting the nonresident path.
- `9bdffcad`, `image-core/src/rgb_render.rs`: retain LR-2 post-curve color and
  LR-3 retouch routing, subsequently moved before Detail by LR-3d.
- `9bdffcad`, `tessera-ffi/src/export.rs`: keep mask support and brush retouch
  registration together, including print's common resource-aware call.
- `02cf00d0`, `tessera-ffi/src/smart_preview.rs`: base LR-7 first-lane schema
  journal test wins; drop the duplicate LR-3 schema checklist test.
- `bc293446`, `engine-api/src/recipe/schema.rs`: retain every predecessor's
  predicate and recursive mask scan, adding only retouch's predicate.
- `bc293446`, `engine-api/src/recipe/settings.rs`: Detail hashes retouch;
  Tone keeps LR-2 monochrome / LR-1 Point Color chaining.
- `bc293446`, `image-core/src/render.rs`: apply retouch to white-balanced
  pixels before pre/post-curve bindings and Detail; retain monochrome condition
  and remove the obsolete late retouch operation.
- `bc293446`, `image-core/src/rgb_render.rs`: retouch before Detail, keep
  pre/post-curve color order, use selected CPU ops in the monochrome block,
  remove late retouch from Locals.
- `bc293446`, `import-lrcat/tests/schema_version.rs`: keep the shared harness;
  final fixture expectation accounts for the newly translated retouch rows.
- `bc293446`, `merge/tests/recipe.rs` and `sidecar/tests/roundtrip.rs`: base
  assert-expected-version-then-normalise pins win; no lane-local copies.
- `bc293446`, `tessera-ffi/src/smart_preview.rs`: retain LR-7's shared test.
- `3334718f`, `docs/coordination/LR-TRANSLATION-MATRIX.md`: preserve every base
  row and all base notes; change only RetouchAreas/RetouchInfo rows. Drop the
  lane-local generic approximation definition and matrix prose additions.
- `721c8a42`, `image-core/src/render_review_tests.rs`: retain both saved-Upright
  skip-analysis and retouch session-reuse tests.
- `721c8a42`, `merge/tests/recipe.rs` and `sidecar/tests/roundtrip.rs`: again
  retain base explicit version pins and normalization.
- `3f745b70`, `engine-api/src/recipe/schema.rs`: preserve all base predicates;
  retouch scans settings and history.base, with only its feature-presence tests.

Semantic integration audit after replay: LR-3's translator now only assigns
retouch settings; LR-7's shared `finish` alone records `Import XMP` / `xmp`.
Diagnostics readers use `entries()` and optional field `as_deref()`. The base
shared matrix guard remains unchanged. RGB Tone memo hashing uses the shared
stage hash so monochrome/Point Color changes invalidate the correct prefix.
Retouch precedes Detail/Tone; Point Color precedes B&W; masks precede Upright.
The shared Point Color compatibility fixture's `structures` row contains a heal
and necessarily changes under LR-3: only that row's byte length/hash is repinned
(18101 -> 20442 bytes); all nine unaffected rows retain their exact pins. LR-7's separate fingerprint
for the same structures row is likewise repinned; its other three rows remain
byte-identical. The
2,000-image integration golden was recomputed by the test and still matches
`87d28d71460e64ad1034fd0a5dc408a20a0452b7d37ccfd6f2a00ada8db3c0d5`.

## Combined regression and gates

Test commit: `c58e617f` (`test(LR-3f): verify combined import and exact spot
exterior pixels`). Semantic post-replay corrections are folded into the replayed
LR-3e integration commit `c206de07`; no original lane commits were squashed.
All requested gates passed on this exact source. The final documentation commit
changes only this handoff and restores base matrix prose; its two retouch rows
are identical to those exercised by the gates. The preliminary full run
stopped at LR-1's structures pin; the importer follow-up exposed LR-7's pin for
the same row. Both were updated after inspecting the retouch-only changes.
The focused combined regression passed before the clean final run.

Environment and final commands (no test filters or command-level exclusions):

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-4-parametric-masks
export CARGO_BUILD_JOBS=4
export RAYON_NUM_THREADS=4
cargo clean --release -p brush -p engine-api -p export -p image-core \
  -p import-lrcat -p pipeline-cpu -p previews -p tessera-ffi -p tessera-mcp
cargo test --release -p import-lrcat -p engine-api -p pipeline-cpu \
  -p pipeline-gpu -p brush -p image-core -p export -p previews \
  -p tessera-mcp -p sidecar -p merge -p mask-store -p tessera-ffi
cargo clippy --release --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
(cd apps/mac && ./build-ffi.sh)
tools/orchestrate/swift-gate.sh
(cd apps/mac && swift build -c release --product Tessera \
  -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors)
```

Release cleaning removed 3,548 files / 8.8 GiB; the clean compilation completed
in 6m 33s. Final gates:

| Gate | Result | Wall time |
| --- | --- | --- |
| Unfiltered 13-package release tests | PASS, exit 0 | 935 s |
| Release workspace/all-target clippy, `-D warnings` | PASS, exit 0 | 33 s |
| `cargo fmt --all -- --check` | PASS, exit 0 | 2 s |
| `apps/mac/build-ffi.sh` | PASS, arm64 archive and generated bindings | 143 s |
| `tools/orchestrate/swift-gate.sh` | PASS, `SWIFT GATE OK` | 434 s |
| Strict-concurrency / warnings-as-errors Tessera release build | PASS, exit 0 | 138 s |

Cargo test summaries report 1,810 passed, zero failed and 59 existing ignores
(including child-process executions in the reported pass total). No command
filters, skip flags, threshold changes or test exclusions were used. Repo RAW
fixtures were exercised. The new LR-3f combined test passed in the clean gate.
Swift gate: 920 XCTest tests, 3 skipped, zero failures; an additional 5 Swift
Testing tests passed. FFI generation produced no tracked binding delta.
The strict release product build completed in 137.86 s. It emitted one nonfatal
linker warning: cached `blake3_neon.o` was built for macOS 26.2 while linking for
15.0. Swift compilation with complete strict concurrency and warnings-as-errors
passed; no deployment compatibility claim beyond these requested gates is made.

Final audits: zero Cargo.toml / Cargo.lock delta against `04217312`; all 118 base
matrix rows remain and only RetouchAreas/RetouchInfo differ. Base shared schema
journal, sidecar/merge round-trip pins and translation-matrix guard are unchanged.
The pre-existing untracked `LR-RULINGS-FROM-A.md` is untouched.
Raw command logs are `/tmp/LR-3f-{release-final,clippy,fmt,ffi,swift-gate,swift-release}.log`;
detailed Swift test output is `/tmp/LR-3f-swift-tests-detail.log`.


 The LR-3f test uses a generated catalog row combining
LR-1 Point Color, LR-2 monochrome/mixer, LR-3 heal plus clone, LR-4 nested display
luminance range and LR-7 saved Upright. It verifies one Import edit, schema 4,
shared per-lane diagnostics, CPU render, both spots changing pixels and exact
exterior bits against a separately parsed row without spots. Identity Upright
and disabled spatial Detail controls isolate literal spot footprints while all
imported color and mask operators remain active.

---

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
