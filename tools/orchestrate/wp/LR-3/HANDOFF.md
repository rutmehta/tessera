# LR-3: retouch translation — renderer integration blocked

**Status: PARTIAL; full Rust gate has a Liquify latency failure. Do not merge as
app-visible retouch support.** Translation and an explicit CPU-operator render test are
implemented. Full Develop integration is blocked by the no-new-dependency rule.
The CPU reference renderer currently rejects nonempty retouch recipes.

## Commits and ownership

- Branch: `wp/LR-3-retouch`; local only, never pushed.
- Starting HEAD: `87ff1ff173e4d6d2053a534b7cfd9343aef906e1`, the brief-only
  successor of the requested base `e6c3e5da`.
- RED: `68a042a976daed9b350a8835a27dc363b73759f9`.
- Additional regression tests: `1b2b067f9653c9abb3f9e7e42638a814fcb4fadd`.
- Implementation: `31562ff18e38100489f5f79f3fd96c393b665004`.
- The documentation commit follows the implementation; its hash is in the final
  agent summary and branch HEAD.

All commit messages carry the requested Claude Fable 5.1 coauthor trailer.
No dependency manifest, Cargo.lock, board, mailbox, Swift source or gate, or app
launch was involved. No personal catalog was accessed; new fixtures are entirely
synthetic. The externally supplied `LR-RULINGS-FROM-A.md` was left untouched.

## Implemented

`crates/import-lrcat/src/retouch.rs` consumes the existing retained-source
contract. Supported Lua tables, legacy comma-separated RetouchInfo strings, and
XMP RDF resources become existing heal/clone operations in
`settings.locals.retouch`. Explicit normalized source coordinates become source
minus destination offsets. Circles use a one-point brush target; simple paint
paths preserve ordered dabs. Radius is relative to image width. Adobe 0–1
opacity/feather/flow become recipe percentages. Feather is applied on the target,
with no second operation-level feather.

A complete supported property is removed from `lrcat_develop_source.properties`
and its stale CRS diagnostic is removed. Unsupported properties retain their
source. Empty-only values preserve existing output; matching nonempty aliases
apply once, and conflicting aliases remain retained. A proper import-history
edit keeps settings and history replay consistent. No recipe schema changed.

Shared-file changes are small: one module registration; Lua uses an untranslated
XMP decode before rebuilding exact Lua retention, then calls the translator;
XMP has an additive wrapper calling the same translator after source retention.
The ordinary sidecar decoder and the existing CPU brush operators are unchanged.

## RED / GREEN evidence

At `68a042a9`, four mapping tests failed because zero operations were imported;
the malformed-source retention test passed. The synthetic catalog/CPU test also
failed at the expected zero-versus-one operation assertion. The two later edge
regressions failed against the initial translator draft (unknown method was
accepted; inherited XML prefixes were not resolved) before implementation fixes.
All seven mapping/retention tests now pass.

The end-to-end test writes a synthetic catalog, replaces one develop row, imports
it, then explicitly adapts its decoded target to the existing `brush::Stroke`
CPU clone operator on a synthetic 128×80 raster. It does not use the application's
Develop dispatch. Destination is (32, 40), source is (96, 40), radius is 8 px,
opacity is 50%, and feather is 50%.

Measured:

- Affected-region centroid error: **0.00000000 px**, limit ≤ 1 px.
- Center channel value: **0.50000000**, absolute tolerance 1e-5.
- Feather shoulder: **0.17323937**, absolute tolerance 1e-5 against the independent
  reference `0.5 * smoothstep((8.5 - sqrt(6.5² + 0.5²)) / 5)`.
- Pixel outside the footprint remains exactly zero.
- Retouch keys are absent from the fixture's retained-source map.

No RED assertion was weakened. The feather assertion was tightened from an
interval to the independent numerical reference. Clippy-only assertion spelling
changes preserve behavior. The recipe-history invariant failure discovered by
the catalog test was fixed by using `Recipe::edit`.

## Gates

Environment for every Cargo command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-3-retouch
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

- PASS: `cargo clippy --locked -p import-lrcat -p engine-api -p tessera-ffi --all-targets -- -D warnings`
- PASS: `cargo fmt --all --check`
- PASS: `git diff --check`
- FAIL: `cargo test --locked -p import-lrcat -p engine-api -p tessera-ffi -- --test-threads=3`
  stopped at the unmodified `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`.
  Initial run: median 289.8 ms, p95 **583.7 ms**, threshold **< 250 ms**.
  Isolated retry (`--exact --nocapture --test-threads=1`) also failed: median
  212.4 ms, p95 **266.1 ms**. No threshold or renderer code was changed. This is
  a measured failure on this shared Mac, not a proven LR-3 regression or a claim
  that contention is the sole cause.
- PASS: all 32 remaining FFI test binaries, using `--no-fail-fast` to finish
  coverage after the original Cargo invocation stopped: **204 passed, 16 ignored**.
- PASS: `cargo test --locked --doc -p import-lrcat -p engine-api -p tessera-ffi`
  (these three packages currently have zero doc tests).

The initial gate recorded **523 passed, 1 failed, 15 ignored**. Together with the
continuation, **727 tests passed, 31 were ignored, and one unique test failed**;
the isolated retry reproduced that same failure. The full gate is NOT green.
All seven LR-3 mapping tests and the synthetic CPU import/render test pass.
Compact evidence is in [EVIDENCE.md](EVIDENCE.md); full raw logs are archived on
Machine B under `$CARGO_TARGET_DIR/lr3-evidence/`.

`tessera-ffi` is included because it already depends on both importer and brush;
only a new test file is added there. This avoids adding even a test dependency.
The existing LibRaw C++ build emits `sprintf` deprecation messages; Rust clippy
with warnings denied passes. Swift gates and live app verification are deferred
to the coordinator, as instructed.

## B5-29c compatibility

A comparison against starting HEAD generated the same 20-image synthetic catalog
shape, normalized only each recipe's catalog-path-derived image ID, and compared
the serialized JSON byte slices per image. **18 of 20 images were byte-identical.**
Only synthetic IDs 1003 and 1013 changed; both contain now-translated RetouchInfo.
Their changed paths are limited to:

- `/recipe/settings/locals/retouch`
- `/recipe/history/entries` and `/recipe/history/head`
- `/recipe/crs:RetouchInfo`
- `/recipe/lrcat_develop_source/properties/RetouchInfo`

The 2,000-image golden includes 200 such structure rows. It was re-pinned after
this comparison; streaming/import/PlanJson byte equality remains tested.

- Previous digest: `d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`
- New digest: `8dd7443a4a62f86c4133d2ee8bbb2036c1790912ed21070d799d1a8dd87c16c4`

Other lanes merging this change must regenerate their combined synthetic golden
if their own translations affect these rows. Do not revert this digest change
without also accounting for the newly translated retouch records. Retention
policy for other keys is untouched.

## Blocked and unrepresentable

**Merge blocker:** `image-core/src/rgb_render.rs` dispatches local adjustments,
not retouch. `pipeline_cpu::validate_settings` rejects nonempty retouch, so the
newly imported operations can make a CPU-reference Develop request refuse the
recipe. The explicit CPU brush test is not proof of working application render.

Neither image-core nor pipeline-cpu depends on brush. Adding pipeline-cpu→brush
would create the cycle brush→compositor→merge→pipeline-cpu. A clean image-core
integration needs a new dependency or an agreed dispatch/extraction design.
Stopped at this boundary under the user's dependency instruction; Cargo.lock
and manifests were not touched.

Conservatively retained variants include missing/unresolved source coordinates,
OffsetY-only source encodings (semantics not established), variable-radius or
pressure dab commands, center-weight and other unknown mask fields, inverted or
subtractive masks, seed/heal-version metadata, unknown methods, generative/remove
modes, and conflicting nonempty aliases. Empty-only retouch lists retain their
previous output. This is not complete coverage of every Adobe retouch encoding.

Heal kind and offsets are mapped, but Adobe healing solver parity is unverified;
Tessera's existing operator uses Poisson blending. The pixel test covers clone,
not Adobe-vs-Tessera healing equivalence. Real Adobe renders, rotated/cropped
coordinate conventions, and undocumented encodings were not used as fixtures.
