# LR-4b follow-up — Machine B, local only

**Partial completion.** Four-bound luminance and radial inversion are implemented;
range subtype dispatch covers luminance and scalar depth. Adobe brush and color
models remain blocked by the full-fidelity requirement; no guessed decoder or
approximate operator was promoted. The original LR-4 handoff follows below.

Base: `a88440a4`, branch `wp/LR-4-parametric-masks`, no rebase. The supplied
`LR-RULINGS-FROM-A.md` remains untracked and untouched. No push/mailbox/board,
Cargo.lock, dependency, Swift, app or real-catalog changes/access.

## LR-4b requested items

| Item | Outcome |
| --- | --- |
| 1. Brush `Dabs` | **Blocked/source-retained.** `d/r/f/h` grammar evidence exists, but the current linear-flow/smoothstep brush is not Adobe's measured operator. No unverified mapping was shipped as full fidelity. |
| 2. Color sampled/point/area models | **Blocked/source-retained.** Neither Adobe sample color space nor the area-model/amount selection function is established as Tessera's OkLab-distance operator. |
| 3. Four-bound `LumRange` | **Implemented** with optional `MaskComponent.luminance_bounds`, default absent/omitted, CPU independent smoothstep shoulders, JSON/native-XMP round trip. |
| 4. Subtype-coded ranges | **Partial**: Type 2 luminance and Type 3 scalar depth translate; Type 1 color stays blocked. Unknown codes remain unchanged. |
| 5. Radial `Flipped` | **Implemented** as the complement of `MaskInverted`. Matching paired flags apply once; conflicts retain source. |

Synthetic Lua and XMP import-to-CPU exposure fixtures cover the implemented forms.
They prove the stated engine semantics, **not Adobe pixel parity**. Depth fixtures
supply an invented same-level depth plane. `Mask/Image` remains LR-5.

## LR-4b representation and consumers

- `luminance_bounds: Option<[f32;4]>` is the only new recipe field. No format or
  contract version bump. Old builds parse it away and use the two-bound fallback;
  they cannot render the new shoulders faithfully.
- CPU validates active ordered bounds and handles collapsed shoulders. Disabled
  ancestors suppress inactive geometry validation while structural limits remain.
- FFI normalization and external/AI composition retain the additive field.
- GPU metadata explicitly rejects four-bound masks. Camera Raw's capability
  predicate declines them before dispatch, selecting the existing CPU fallback.
- A complete-parent structural audit gates new foreign decoding. A failed audit
  or renderability check uses prior mask-decoder behavior, preserving untranslated
  recipes as well as exact source envelopes. Native typed fields still round-trip.
- All new fixtures are invented. The 44-group fixture is untouched; eleven
  unsupported Lua/XMP recipes are pinned against a detached `a88440a4` baseline.
  The temporary baseline worktree was removed after collecting its digests.

See [LR-4b contract, evidence and blockers](../../../../docs/coordination/LR-4B-PARAMETRIC-MASKS.md)
and [matrix rows](../../../../docs/coordination/LR-TRANSLATION-MATRIX.md). The matrix
file was absent on the base commit; the new file contains LR-4b rows only and must
be merged with the coordinator's full matrix rather than replacing other lanes.

## LR-4b commit/evidence sequence

- `1f4ca976`: RED importer 3 failures, schema 1, CPU 2, GPU 1, native-XMP 1,
  import/render 1. See `red-lr4b-{forms,schema,cpu,gpu,sidecar,e2e}.log`.
- `6ca578f7`: mixed/ambiguous four-bound payload RED; `red-lr4b-ambiguous.log`.
- `b49f1900`: external composition, GPU capability fallback and disabled-tree RED;
  baseline-pinned byte tests and extended subtype pixel cases.
- `0da696a9`: RED unknown sibling/parent fields preserve old atomic behavior;
  `red-lr4b-untranslated.log`.
- `760b984b`: implementation; final validation evidence accompanies the docs commit.

Later consumer/source-audit RED checks ran against the then-uncommitted
implementation of the earlier tests. Some later test commits reference the new
field before its implementation commit, intentionally following the tests-first
sequence. No passing test is claimed for those intermediate committed trees.

Every commit has the requested Claude Opus 5.5 co-author trailer. All remain local.

## LR-4b gates

- Completed synthetic `cargo test --locked --no-fail-fast` across nine crates:
  **1202 passed, 1 failed, 30 ignored, 141 filtered**, 208 harness/doc-test result
  summaries; 2926.52 seconds. Exit 101, **not a green gate**.
  `gate-lr4b-synthetic.log` contains the command, exclusions and full results.
- Sole broad-run failure: existing FFI
  `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`, p95 **388.8 ms**
  against **<250 ms**. Exact compiled test rerun alone with test threads=1 and
  Rayon threads=3 also failed: p95 **318.2 ms**, 52.92 seconds, exit 101.
  See `gate-lr4b-latency-rerun.log`. Shared host load was present, but the rerun
  does not establish a passing result or prove load was the sole cause. No
  Liquify implementation or threshold changed in this lane.
- All LR-4b mapping, source-promotion/retention, schema round-trip, native-XMP,
  CPU render, external composition, disabled-tree and GPU fallback tests passed.
  Eight invented Lua/XMP exposure scenes measured maximum absolute channel error
  **0**, tolerance **1e-6** (`green-lr4b-pixels.log`). This is engine-semantics
  validation, not an Adobe pixel-parity result.
- The unchanged 44-group golden passed (81,809 bytes; SHA-256
  `aec3a2eb9f1a31a1596d063b27ba785ae1d55219ba1931745c79a7cf9d8043cb`).
  All eleven baseline-pinned unsupported Lua/XMP recipes passed byte comparison.
- Clippy across the same nine crates, all targets, `--locked --no-deps -- -D warnings`:
  **passed**, 41.84 seconds (`gate-lr4b-clippy.log`).
- `cargo fmt --all --check`: **passed** (`gate-lr4b-fmt.log` is empty on success).
  `git diff --check`: **passed**.

The initial focused and two preliminary broad runs were interrupted for consumer
and source-compatibility fixes. They are **not completed green gates**. Their logs
are retained separately. One scalar-luminance pixel fixture initially sampled
exactly at a hard bound; pre-existing f32 luma rounding selected the other side.
The final scalar fixture uses interior samples; the four-bound test still covers
endpoints and fractional shoulders. No production luma calculation was changed.
`red-lr4b-pixels-boundary.log` records that fixture failure.

Run the final synthetic gate with:

```sh
python3 tools/orchestrate/wp/LR-4/run-lr4b-synthetic-gate.py
```

It selects the nine touched crates: import-lrcat, engine-api, sidecar,
pipeline-cpu, pipeline-gpu, tessera-ffi, ml-depth, mask-ai and filters. Test files
that can reach RAW fixtures are excluded by printed name filters; optional RAW
fixture environment paths point to an absent lane-local path. Some synthetic
checks in those mixed files are consequently excluded too. This is not an
unrestricted workspace/RAW gate. Commands use `--locked`, the prescribed target,
3 build jobs, 3 Rayon threads, debug info=0, incremental=0, test threads=1.

---

# LR-4 parametric masks — local handoff (partial coverage)

Status: partial Adobe-format coverage. The additive group/enabled representation
and CPU/downstream consumers are implemented; opaque Adobe brush/color payloads
remain source-retained. See [representation and limits](../../../../docs/coordination/LR-4-PARAMETRIC-MASKS.md).

Branch `wp/LR-4-parametric-masks`; starting HEAD
`87ff1ff173e4d6d2053a534b7cfd9343aef906e1` (parent `e6c3e5da`).
No push, mailbox, board, lockfile, dependencies, Swift gates or app launch.
The coordinator-supplied `LR-RULINGS-FROM-A.md` remains untracked and untouched.

## Commit sequence

| Stage | Exact hash | RED evidence |
| --- | --- | --- |
| Disabled/tree tests | `5e60a86a0d5169b07d600be0b914a3ef03aba06f` | `red-import.log`: 2 failures; `red-render.log`: 2 failures |
| Explicit ranges | `b3cdbefc3563fa640f32ccf32b1cf5ec4fb443ac` | `red-ranges.log`: range and group constraint failures |
| Inversion/tree bounds | `6d089abe99f785173eaafafa1252e9e03d469f60` | `red-inversion.log`, `red-recursion.log`: 1 failure each |
| Consumers/retention | `c300678ef81d8f9d820aa3112359acc315d237d4` | `red-consumers.log`: 5 failures; cache/external compose: 1 each; retention/native round trip failures |
| Adobe spellings/audit | `d9697be2477b18f9a5abf60027cf819f03d75154` | `red-adobe-shapes.log`: 3 failures |
| Invalid geometry | `460b4f96d10e7bc182c2b1d4c3a65c7ca392eff3` | `red-invalid-geometry.log`: degenerate gradient wrongly source-promoted |
| Mixed range audit | `b7ac91ca8a31565bbb7e119c975074b56c6a313e` | `red-mixed-range.log`: unused family fields wrongly dropped |
| Implementation | `e97876bfb3b5561618045e20e43e435b6069c965` | Final gate results below |

RED commits preceded the corresponding implementation; later RED checks ran
against then-uncommitted implementation where needed. Some consumer RED commits
intentionally reference the new fields before they exist at committed HEAD.
The final duplicate-Lua test asserts parser rejection: duplicate keys never
produce a recipe. The earlier RED retention case also exposed misplaced fields.

## Delivered

- Default-omitted `MaskComponent.enabled` and optional ordered `group`; no enum
  or version bump. Old readers parse the fields away but cannot reproduce their
  pixels. Imported group fallback is an empty brush; native XMP preserves any
  fallback kind using `ts:GroupFallback`.
- Disabled subtrees, recursive add/subtract/intersect and wrapper inversion;
  64-level/65,536-node limits. Refinement runs once at the outer CPU composite.
- `Mask/Group` and `Mask/Aggregate`, component/group range constraints, scalar
  luminance/depth bounds, range/component inversion XOR, and enabled flags.
- Recursive RGB cache dependencies, external-raster composition, export/FFI AI
  discovery and FFI window coordinates. GPU metadata rejects trees for existing
  CPU fallback; disabled components do not seed GPU masks.
- Conservative source promotion requires one decoded property, a complete
  structural audit and CPU-compatible active geometry/amounts. Legacy flat
  inputs retain their old envelope. Lua/XMP decoder edits remain additive.

## Compatibility (B5-29c)

Default fields are omitted exactly. A new invented 44-group gradient/radial
fixture was serialized on the untouched starting commit: 81,809 bytes and
Digest::derive("LR-4 legacy recipe", bytes) =
`aec3a2eb9f1a31a1596d063b27ba785ae1d55219ba1931745c79a7cf9d8043cb`.
`baseline-legacy44.log` captures that baseline; the test pins it without
repinning. The existing 2,000-row retained-source golden is unchanged.
This is synthetic baseline coverage, not inspection of any real catalog's
44 groups. Nil, unknown/misplaced fields and unsupported foreign payloads retain
source; duplicate Lua keys continue to be rejected by the existing parser.
Raw XMP packets remain separately preserved even after per-key promotion.

## Gates and measurements

- Final importer: **76 passed, 0 failed, 1 ignored**, 70.66 s. This includes the
  last source-audit change and the 44-group baseline digest.
- End-to-end Lua/CPU: five 4x1 invented scenes, 3 channels each; maximum absolute
  channel error **0** in all five, against tolerance **1e-6**. Cases are gradient,
  radial, luminance band, supplied depth plane, and nested disabled/intersect/
  subtract composition with local exposure.
- Clippy: all 11 touched crates, all targets, no dependencies, `-D warnings`:
  **passed**, 10.83 s final invocation (earlier full invocation 130.20 s).
- `cargo fmt --all --check` and `git diff --check`: **passed**.
- Focused LR-4: **33 passed, 0 failed**, 357.20 s including lock wait and
  launching/filtering all package test binaries. The additional mixed-range
  regression is covered by the final importer gate above.
- Broad synthetic gate: **1,328 passed, 1 failed, 26 ignored, 155 filtered**,
  248 completed test/doc-test suites, **2711.45 s** (45 min 11.45 s), exit 101.
  Only failure: unchanged export `script_timeout_cancellation_empty_and_spawn_failure`
  could not read its marker after the one-second script deadline. Separate runs
  of the identical workflow binary passed **3/3 twice** (before and after the
  broad gate). Root cause is not established; this broad gate is **not green**.
  The final mixed-range audit was added after this gate compiled; the complete
  importer rerun and final clippy cover that change.

All cargo commands use `--locked`, the lane's
prescribed target directory, build jobs=3 and Rayon threads=3. Shared disk
pressure required debug info=0 and incremental=0 for dev/test profiles; test
threads=1. No package dependencies or lockfile changes.

Reproduce the synthetic gate with:

```sh
python3 tools/orchestrate/wp/LR-4/run-synthetic-gate.py
```

The helper runs cargo test across all 11 touched crates: `import-lrcat`,
`engine-api`, `sidecar`, `pipeline-cpu`, `mask-ai`, `image-core`, `export`,
`pipeline-gpu`, `tessera-ffi`, `ml-depth`, `tessera-mcp`. It excludes every test
in integration files that can reach external/repository RAW fixtures and points
optional RAW fixture variables at an absent lane-local path. Exact excluded
files/name filters and the expanded command are printed in `gate-test.log`.
These exclusions mean this is not an unrestricted whole-workspace gate.
An initial unrestricted run was stopped after an existing image-core test loaded
repository RAW fixtures; it is not claimed as a synthetic or completed gate.
No real catalog was accessed; all added LR-4 fixtures are invented.
The initial run also hit an unchanged export script timeout test (marker absent);
the identical binary rerun passed 3/3 in 1.05 s. The failure recurred under
the broad synthetic gate, so that gate is not reported as clean. See
`gate-export-workflow-rerun.log` and `gate-export-workflow-final-rerun.log`;
no export workflow code was changed.

## Blocked / unrepresentable

- Adobe opaque brush `Dabs` and color `AreaModels`/`PointModels` lack verified
  encoding/coordinate/color-space semantics. Existing native brush/color RDF
  tests are not foreign Adobe decoding. These remain atomically source-retained.
- Four-bound `LumRange` needs asymmetric falloffs; Type-coded ranges and radial
  `Flipped` are unresolved. Nonzero scalar feather can decode best-effort but
  stays source-retained because Adobe falloff parity is unverified.
- `Mask/Image` and its resource reconstruction remain LR-5. New groups can host
  future external leaves; LR-5 must add/classify/request the actual raster kind.
- Scalar depth requires a runtime normalized plane; depth resource lookup,
  corrected-depth edits and `RangeMaskMapInfo` remain outside this lane.
- Local curves/PointColors/grain, defringe and color overlay are not added.
  Source promotion refuses unrenderable defringe/color overlay and bad geometry.
- Existing blend-code and slider conventions are unchanged, not newly validated
  against Adobe renders. Synthetic expected pixels prove engine semantics only.
- No recursive GPU shader, Swift bridge generation, UI nested-child editor or
  live app verification. Machine A should review the additive consumer changes
  before integration and keep source-retained cases visibly incomplete.

All implementation is committed locally; no push was performed. The docs commit
containing this handoff follows the implementation hash above. Machine A should
review the unresolved coverage and non-green broad gate before deciding to merge.

Research links and the precise recipe/render contract are in the linked design
note. No public source rows were copied into fixtures.
