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
