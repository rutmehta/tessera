# LR-7 — Upright geometry / cloud diagnostics

Machine B, branch `wp/LR-7-upright`. Local commits only; coordinator owns merge.
Ready for review with the RAW-fixture gate exclusion described below.

## Exact commits

- Actual starting HEAD: `87ff1ff173e4d6d2053a534b7cfd9343aef906e1` (brief ownership documentation immediately after requested `e6c3e5da`; no rebase performed).
- RED: `68915c5ef7227c0fec56ebe8f3345b733030bf35`.
- Implementation: `0669664ddd67d7b92ffc580f169d8aa9f8bd22cc`.
- This handoff, README and evidence are in the subsequent `docs(LR-7):` commit; final lane HEAD identifies that documentation commit.

Followed the lane brief and Machine B overrides, including the coordinator's
`LR-RULINGS-FROM-A.md` ruling permitting an optional geometry homography and
requiring the cloud diagnostic wording. That coordinator-owned untracked file
is not included in this lane's commits.

## What landed

- Existing PerspectiveUpright mode mapping and all seven manual controls remain
  intact: Vertical, Horizontal, Rotate, Scale, Aspect, X, Y.
- The active `UprightTransform_1..5` comma-separated 3x3 matrix becomes optional
  `/settings/geometry/upright/homography`. Existing mode numbering: Off=0,
  Auto=1, Full=2, Level=3, Vertical=4, Guided=5.
- The matrix is a source-to-output map in [0,1] image coordinates. CPU geometry
  conjugates to its [-1,1] coordinate system and inverts once, bypassing image
  analysis. Manual transforms and crop keep their existing composition order.
  The resident lens plan uses the same saved inverse. No new dependency.
- Guided mode translates complete sets of 2–4 `UprightFourSegments_0..3`
  endpoint strings, with count validation, bounded finite coordinates and
  nonzero segments. Translation is atomic for the guide set. A saved matrix
  takes precedence; guide-only imports report recomputation/parity limitations.
- Enabled `EnableDistractionRemoval`, `GenerativeRemove`, `GenerativeFill`
  produce an import report explanation containing “requires Adobe cloud; not
  translatable”, explain the missing rendered pixels, and suggest a rendered
  TIFF export. Source remains retained.
- Selected matrices and complete guide sets leave pending source only after
  successful translation. Singular/nonfinite/pole-crossing matrices remain
  exact source and get diagnostics. Inactive matrices remain preserved.
- Recipe JSON round-trip and history replay validate with the new field.
  Absent homography is omitted from serialization; no version bump.

The actual Develop geometry implementation is in `pipeline-cpu`; `transform`
is the document-layer transform crate and needed no change. The integration
test lives in `tessera-ffi/tests` solely because that crate already has both
import-lrcat and pipeline-cpu dependencies. No FFI production or Swift changes.

## RED / GREEN evidence and measured numbers

- `red.log`: four importer mapping/report tests failed for missing fields,
  absent guides and generic-only cloud diagnostics.
- `red-cpu.log`: both CPU tests failed (saved matrix ignored; singular saved
  matrix not rejected).
- `red-render.log`: synthetic SQLite -> import -> CPU test failed because the
  explanatory cloud diagnostic was absent from the import report.
- `green-focused.log`: initial focused GREEN: 5 importer + 1 byte-compatibility
  + 2 CPU tests passed. Subsequent broad gates cover the final implementation.
- One RED test indexed a source map that correctly disappears entirely on
  success. Its assertion was strengthened to require absence of the whole
  pending-source bucket; no behavior expectation or tolerance was relaxed.
- `green-render.log`: synthetic catalog import -> imported recipe -> CPU
  geometry passed. 128x128 coordinate ramp, four interior rectangle corners at
  (32,32), (96,32), (96,96), (32,96). Forward matrix is
  `[[1,0,0],[0,1,0],[0.2,0,1]]`; expected inverse is calculated independently.
  Maximum coordinate error **0.019978 pixels**, tolerance **0.08 pixels**.
  The same test verifies the explanatory warning appears in `ImportPlan.report`.
- `compat-before.log` and `upright_lr7_compat.rs`: pre-change serialized-byte
  fingerprints remain identical for four synthetic untranslated inputs:

| Input | Bytes | FNV-1a fingerprint |
| --- | ---: | --- |
| scalar + unknown structured key | 11420 | 35b9bf7bfe270b28 |
| cloud flag + opaque metadata + inactive matrix | 11199 | 01729e71086f5d4d |
| existing synthetic global.lua | 15228 | 14af73e31df40bc7 |
| existing synthetic structures.lua | 18101 | beb156c96528413e |

These are correctness measurements, not a throughput benchmark or Adobe visual
parity measurement. Build/test times in the logs reflect shared-machine load.

## Gates

Environment for all Cargo commands:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-7-upright"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
# Used on the final synthetic suite to bound test concurrency too:
export RUST_TEST_THREADS=3
```

1. Full `cargo test --locked -p import-lrcat -p engine-api -p pipeline-cpu`
   was started. On discovering that `raw_fixture_goldens` follows the external
   `fixtures/raw` symlink, that test was terminated (SIGTERM). The symlink target
   was not inspected. The unfiltered gate is **not claimed green**.
   Evidence: `green-gates.log` (initial run, interrupted).
2. Synthetic-only gate passed, **327 passed, 0 failed, 3 pre-existing ignored,
   4 filtered out**, in `green-synthetic-gates.log`:

```sh
cargo test --locked -p import-lrcat -p engine-api -p pipeline-cpu -- \
  --skip raw_fixture_goldens \
  --skip real_opcode_fixtures_when_available \
  --skip fixture_as_shot_roundtrip_and_slider_directions \
  --skip five_actual_raws_auto_lens_and_upright_are_finite
```

3. `cargo test --locked -p tessera-ffi --test upright_lr7 -- --nocapture`:
   **1 passed**, `green-render.log`. The unrelated full FFI suite was not run.
4. `cargo clippy --locked -p import-lrcat -p engine-api -p pipeline-cpu --all-targets -- -D warnings`:
   **passed**, `clippy.log`.
5. `cargo clippy --locked -p tessera-ffi --test upright_lr7 -- -D warnings`:
   **passed**, `clippy-render.log`.
6. `cargo fmt --all --check`: **passed**, `fmt.log`; `git diff --check`: passed.

The build scripts emit existing libraw C `sprintf` deprecation warnings; Rust
clippy with `-D warnings` succeeds. No Swift gates or application launch.

## Limitations / review boundaries

- No Adobe-rendered reference comparison; synthetic geometry accuracy does not
  establish Adobe solver, lens coordinate convention or manual-slider parity.
- Solver cache metadata (`UprightVersion`, center/focal/digest/preview/count
  metadata), inactive solutions and unknown future Upright keys remain retained.
  They are not claimed translated. Only the selected matrix is materialized.
- Guide input supports scalar CSV endpoint strings; alternate table/resource
  shapes remain unsupported and retained. Invalid/incomplete guides do not
  partially populate the recipe. Degenerate guide configurations remain the
  existing guided solver's responsibility.
- Cloud-generated pixels cannot be reconstructed from an enable flag; import
  explains that explicitly. This lane does not implement cloud execution.
- The optional matrix wins over line detection/guides while mode is active.
  Clients that re-solve/re-author Upright must clear or replace the saved
  homography; manual perspective slider edits compose with it. Swift editing
  flows and GPU runtime execution were not exercised by this lane.
- Four external-RAW-dependent tests are deferred for a coordinator environment
  with authorized fixtures. No catalog-derived fixture was created or committed.

## 29c integration notes

- `lua_develop.rs`: one additive post-retention hook (16 lines).
- `xmp.rs`: one additive post-retention hook (8 lines).
- All new translation is isolated in `import-lrcat/src/geometry.rs`.
- No changes to retention heuristics, source spellings, parser budgets, import
  streaming or source externalization. Exact literals/fragments survive for
  untranslated keys. Warning explanations may improve without recipe changes.
- The new hook filters to geometry/cloud keys and returns immediately when
  none occur. No throughput claim; byte compatibility is measured above.
- Only successful translations remove their keys and corresponding stale
  unsupported warnings; unrelated unknown buckets and source remain untouched.
- `Cargo.lock`, `board.json`, mailbox branches and remote branches untouched.

Reference check: [ExifTool's primary XMP CRS tag table](https://www.exiftool.org/TagNames/XMP.html)
confirms the `UprightTransform_0..5` and `UprightFourSegments_0..3` spellings.
This is tag-schema evidence only, not a published Adobe rendering specification.


## LR-7b addendum — ruling 4 legacy independent CA (2026-10-01)

Local-only continuation on `wp/LR-7-upright`, directly on `9fb0f3db`; no rebase.
The untracked coordinator-owned `LR-RULINGS-FROM-A.md` remains untouched.

- RED: `11660342664c6ed0d20ddb86b2df0e984984d450`.
- GREEN implementation: `e335a8a86e131cfef5ee84c921a44dc179170969`.
- The subsequent `docs(LR-7b):` commit contains this append and evidence;
  its hash is the final lane HEAD reported to the coordinator.
- All three commits end with the requested Claude Opus 5.5 co-author trailer.

### Changes and compatibility

- Added optional `Option<f32>` lens fields `legacy_ca_red` and `legacy_ca_blue`,
  with serde defaults and omission when absent. No recipe FORMAT, contract,
  process or smart-preview version bump. The pre-change serde reader ignored
  these keys (the RED round-trip failure demonstrates this); older recipe
  readers can ignore the additions. None adds no correction; explicit zero is
  identity. Existing transient `LensContext.manual_ca` remains the fallback
  per absent channel. Present recipe values override it, avoiding double apply.
- The catalog geometry extension now also imports `ChromaticAberrationR/B`
  from Lua and namespaced XMP, accepting finite values in [-100,100], recording
  an ordinary recipe history edit and consuming only successfully mapped source
  diagnostics. Malformed/out-of-range values remain retained and diagnosed.
  Shared sidecar CRS Legacy targets remain unchanged; no sidecar export claim.
  The existing PV1/2 general fidelity warning remains appropriate and is retained.
- Resolved CPU lens state feeds the existing independent manual-CA pass. It
  operates regardless of profile and automatic-CA toggles, before colour mixing,
  after demosaic for RAW. Finite-field validation is included. Persistent preview
  parsing accepts either optional field without making it mandatory; a synthetic
  proxy round-trip verifies both persistence and rejection of stale-prefix edits.
- Matrix and its guard were copied from local `main` because neither existed on
  this branch. Only the two CA rows were promoted, with concrete recipe pointers
  and Lua values 35/-25. They are the only legacy CA rows assigned to LR-7 there.
  Other rows retain main's statuses. The guard now checks 10 translated rows.
- Ruling 9 required no production change: enabled `EnableDistractionRemoval`,
  `GenerativeRemove`, and `GenerativeFill` already report the exact substring
  **"requires Adobe cloud; not translatable"**. A regression test covers all three.
- The four pre-existing untranslated-input serialized byte fingerprints in
  `upright_lr7_compat.rs` remain identical. No dependencies, Cargo.lock, board.json,
  remote branches, mailbox branches, Swift gates, GUI/app launch or real catalog
  access. All database and pixel fixtures here are synthetic.

### Formula, source and assumptions

For output pixel p=(x,y), active crop [x0,y0,w,h] and channel coefficient a:

```text
c = (x0 + w/2 - 0.5, y0 + h/2 - 0.5)
k_R = 1 + clamp(legacy_ca_red,  -100, 100) / 10000
k_B = 1 + clamp(legacy_ca_blue, -100, 100) / 10000
q_R = c + k_R * (p - c)
q_B = c + k_B * (p - c)
out_R(p) = bilinear(in_R, q_R)
out_B(p) = bilinear(in_B, q_B)
out_G(p) = in_G(p)
```

Coordinates clamp to the source image at boundaries. Positive a samples farther
from the center (shrinks the displayed channel); negative a expands it. ±100
means ±1% source-radius displacement. This is Tessera's existing
`optics::manual_ca` / `ManualCaSettings` convention, now reachable from recipes.

[Adobe's CRS schema](https://github.com/adobe/xmp-docs/blob/master/XMPNamespaces/crs.md)
documents both key names and the -100..100 range.
[Adobe's Camera Raw workflow whitepaper, page 5](https://www.adobe.com/digitalimag/pdfs/ps_workflow_sec2.pdf)
describes separate red/blue channel-size adjustments and calls the adjustment
nonlinear, without specifying the transfer function. **The constant radial scale,
1/10000 factor, sign and crop-center choice are explicit approximation assumptions,
not a recovered Adobe equation or measured Adobe pixel parity.** Translation status
means settings are represented and rendered; it does not claim proprietary parity.

### RED / GREEN and measured tolerances

`LR-7b-evidence.log` records test output and gate summaries.

- RED round-trip: each new field was discarded (Null instead of -100).
- RED importer: coefficient absent with legacy unsupported diagnostic.
- RED matrix: ChromaticAberrationB still retained its unsupported diagnostic.
- RED CPU: radial error 0.01060665 exceeded fixed tolerance 0.0005.
- RED generated SQLite -> recipe -> CPU: no translated coefficient found.
- GREEN per-field recipe round-trips cover -100, -25, 0, 35, 100, independently;
  Lua and XMP imports cover each, with full Recipe JSON replay round-trips.
- GREEN radial target: 129x129 radius ramp, center (64,64), tested interior
  [16,112]^2 excluding radius <8 to avoid the radial cusp. Independent analytic
  expected radius r*k/64. Pairs (100,-100), (-50,75), (0,0) yield maximum errors
  **0.00005652**, **0.00003661**, **0.00000005**, each < **0.0005** normalized units
  (equivalent to 0.032 radius pixels). Green plane is bit-identical; zero/zero
  leaves all input planes bit-identical.
- GREEN synthetic catalog -> full CPU renderer: analytic radial reference runs
  through the same downstream colour/tone pipeline with CA removed. Maximum
  absolute linear RGB difference **0.00002826**, tolerance **0.001**, same interior.
- Initial broad run caught three failures after optional fields were mistakenly
  made mandatory in the strict preview schema. Corrected before final gates;
  both old snapshots and the new optional-prefix round-trip now pass. No RED
  expectation or pixel tolerance was weakened.

### Final gates

Environment: PATH prepended with `$HOME/.cargo/bin`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-7-upright`,
`CARGO_BUILD_JOBS=3`, `RAYON_NUM_THREADS=3`, `RUST_TEST_THREADS=3`.

```sh
cargo test --locked -p import-lrcat -p engine-api -p pipeline-cpu -- \
  --skip raw_fixture_goldens \
  --skip real_opcode_fixtures_when_available \
  --skip fixture_as_shot_roundtrip_and_slider_directions \
  --skip five_actual_raws_auto_lens_and_upright_are_finite
# 336 passed, 0 failed, 3 pre-existing ignored, 4 excluded external-RAW tests
cargo test --locked -p tessera-ffi --test legacy_ca_lr7b -- --nocapture
# 1 passed; test-only FFI integration, no production FFI edits or full FFI suite
cargo clippy --locked -p import-lrcat -p engine-api -p pipeline-cpu --all-targets -- -D warnings
cargo clippy --locked -p tessera-ffi --test legacy_ca_lr7b -- -D warnings
cargo fmt --all --check
git diff --check
```

All listed final gates passed. Existing libraw C compiler `sprintf` deprecation
warnings remain; Rust clippy passes with `-D warnings`. External RAW qualification,
Adobe-rendered reference parity, GPU execution and app/Swift gates are not claimed.

## LR-7c — Machine A review corrections (2026-10-01)

This section supersedes LR-7/LR-7b claims that a saved Adobe matrix or legacy CA
is fully translated, that translated source is deleted, or that an imported
matrix may continue overriding mode/guide edits. Local-only work; Machine A owns
the force-push and integration. No real catalog or GUI was opened.

### Rebase and commits

- Rebased onto `origin/main` at `44db24b251522a477375d36f3bc34244ea015836`.
- Resolved the expected add/add translation-matrix conflict by preserving every
  main row and applying the lane's CA row changes. The guard file was identical
  to main at rebase. No extra rebase commit, dependency/lockfile or board changes.
- RED: `759e013d` (`test(LR-7c): cover review regressions for saved geometry and legacy CA`).
- Implementation: `bb87f061725c4a1929da6e94be57586d74ac26dd` (`fix(LR-7c): preserve approximate imports and honor Upright edits`).
- Final lane HEAD is the subsequent documentation commit. Every new commit has
  the requested Claude Opus 5.5 co-author trailer.

### Blockers and majors

1. **Approximation/source contract:** the matrix and guard support `approximate`.
   A concrete fixture must populate its recipe path, retain its exact literal in
   `unknown.lrcat_develop_source.properties`, and carry an info-level per-key
   `unknown.translation_diagnostics` message beginning `approximate: `, with zero
   user-facing warnings for that mapping. This applies to CA, selected matrices,
   center/focal framing and complete guide sets. Lua literals/ordered entries and
   XMP fragments stay retained. LR-1/LR-2 notes explicitly inherit the same rule.
2. **Frame assumption:** row-major source-to-output matrices are still an
   approximation. With center/focal metadata, assume
   `q = (u-cx, v-cy)/(f35/35)` on both normalized image axes, defaulting missing
   centers to `(0.5,0.5)` and missing focal length to `35`. Center/focal mode flags
   signal a saved frame; their Adobe enum meanings are not claimed verified.
   Conjugate into the recipe's unit frame at import. Invalid/nonfinite frame
   values are rejected and retained. Non-square 160x80/80x160 rotated ramp tests,
   center-offset rotation and focal-scaled translation tests cover this explicit
   assumption. Poles are checked after frame conversion, in the actual image
   domain; unknown future frame-family keys remain opaque and cannot select a
   frame. Both boundaries have separate observed RED/GREEN regressions. These
   tests are not Adobe reference evidence.
3. **Stale solutions:** optional `homography_mode` tags the solved mode. A mismatch
   cannot bypass the solver. Recipe edits and live FFI edits invalidate a saved
   matrix on mode/guide changes; Swift patches explicitly clear matrix and tag.
   Rust mode/guide regressions and Swift mode/endpoint-edit regressions cover it.
   Absent tags on older recipes remain compatible; absent fields stay omitted.
4. **Resident CA admission:** legacy coefficients participate in export admission,
   M2 activation and the resident-prefix support check. Resolution reaches the
   lens-plan path; its existing manual-CA restriction selects the CPU reference
   prefix rather than silently omitting CA. The resident-backend-versus-CPU test
   uses profile None, automatic CA disabled and nonzero red/blue coefficients,
   with an offset non-square default crop. It explicitly permits/validates the
   required fallback; it does not claim a new GPU CA kernel.
5. **PV/zero handling:** only Adobe PV1/2 materializes legacy CA. Zero values are
   absent; stale PV2012+ values remain source-only, including when XMP claims an
   older version than the authoritative catalog version. No zero-only history
   edit. Sign, units and constant radial-scale approximation remain unverified.
6. **History:** geometry and CA fold into the same import transaction as ordinary
   develop settings, with `Author::Import`; at most one entry, none for a no-op.
7. **Sidecar:** one shared decoder now serves catalog and standalone XMP paths.
   Export writes canonical CRS values for changed settings and preserves
   unchanged source fragments. `ts:GeometryLens`, bound to the existing CRS
   ExportHash, round-trips optional native geometry/CA fields and the mode tag.
   External CRS changes invalidate that companion. `sidecar/UNMAPPED.md` updated.
8. **Validation/analysis:** `Recipe::validate` rejects invalid/singular/pole-crossing
   matrices. A usable saved solution skips interactive L0 develop/line analysis;
   the regression asserts zero stage invocations.
9. **Inventory/cloud:** selected transform, center/focal and four-segment rows have
   current statuses; inactive Off matrices and transform count remain retained.
   GenerativeRemove/GenerativeFill rows use the required wording:
   “requires Adobe cloud; not translatable”. Cloud source remains retained.

No FORMAT bump. CA placement remains after demosaic in linear camera RGB, before
colour matrix, Upright and crop, using the default-crop centre. Non-Off Upright's
GPU geometry fallback is unchanged. All added import fixtures are synthetic.

### Verification

Final evidence: [LR-7c-evidence.log](LR-7c-evidence.log).

- Seven-crate `cargo test --locked --no-fail-fast`: **1243 passed, 1 failed,
  49 ignored** across 193 target summaries; exit 101. The only failure was
  `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`: p95 **562.5 ms**
  against `<250 ms`. The requested isolated serial retry also failed, p95
  **486.4 ms** (0 passed, 1 failed, 14 filtered). This remains an unresolved
  latency gate failure, not a proven flake. No threshold or Liquify test changes.
- Every other target passed, including full image-core, pipeline-gpu, all LR-7c
  regressions, translation matrix, sidecar and remaining tessera-ffi tests.
- Required `five_actual_raws_auto_lens_and_upright_are_finite` is opt-in/ignored
  in the normal suite; explicitly executed with `--release --ignored --nocapture`:
  **1 passed**, 205.65s, finite Canon/Sony/Nikon/Fuji/DNG outputs from repo fixtures.
- Seven-crate all-target clippy `-D warnings`, format and diff checks: **PASS**.
- Rebuilt FFI archive/bindings, then Swift gate: **SWIFT GATE OK**, 907 XCTest
  tests (3 skipped, 0 failures), plus 5 Swift Testing tests passed.
- Strict-concurrency, warnings-as-errors release `Tessera` product: **PASS**,
  283.13s. No GUI launched.

The initial RED run failed all three engine regressions (mode, guides, invalid
matrix) and all three initial importer regressions (approximation/history,
PV/zero, center/focal/tag). The first broad build found a test-only accidental
`serde_json` reference in image-core; it was replaced by direct settings
construction without adding a dependency. A subsequent broad run was stopped
when the final frame-domain/future-key regression fixes changed the decoder;
its partial successes are not used as the final gate. The seven-crate gate and
both Swift checks were restarted against the corrected source.

### Integration limits

No Adobe-rendered synthetic chart or public DNG+XMP pair verifies these Adobe
conventions. The status intentionally remains `approximate`, without warnings
for valid mapped values. Exact source is available for a later verified decoder.
`upright_lr7_compat.rs` fingerprints will need re-baselining after LR-2 lands;
this lane does not pre-empt that change. Source-only unsupported properties and
malformed inputs can still have their own diagnostics.

## LR-7d — diagnostics/schema integration and review conditions (2026-10-01)

This addendum supersedes LR-7c's private diagnostics bucket, history-reset behavior,
and unconditional schema-3 serialization. The Adobe coordinate and CA conventions
remain unverified; no Adobe pixel-parity claim has been added.

### Base and commit sequence

- Started at `4eacf725225712f302863ae2908eddcf8b75bf55`.
- Authorized rebase: `git fetch origin && git rebase origin/wp/LR-DIAG`, onto
  `38684d9bdb34f6f6b9178f8589a839f68cb23832`. The first fetch encountered a
  concurrent remote-ref update; retry succeeded. All nine original lane commits
  were replayed without squashing.
- LR-SCHEMA merge `02ae8196` is an ancestor. LR-DIAG's final helper, field-matching
  matrix guard, and report channel are present. The two conflicts in
  `translation_matrix.rs` retained LR-DIAG's guard; LR-7 context was then
  restored without weakening it. Every base matrix row is preserved.
- RED: `cb05baa9f952c309296581593ccbcfd09c7f62de`.
- Implementation: `2abae0bab9d17900ae8c17b300fe8f697509bf29`.
- The following `docs(LR-7d):` commit appends this handoff and evidence. It
  cannot contain its own hash; the final lane summary reports that hash.
- `origin/main` was `e558c5df` initially and remote main advanced to
  `ad93e333` (batch 39) during the gates. A final read-only fetch verified that
  LR-2c still is not on main. This lane stays on the coordinator's explicitly
  authorized diagnostics base; its remote head remains `38684d9b`.
  LR-2c (`c2df1958`) exists on another branch but is not on origin/main or this
  base. The `global.lua` fingerprint remains unchanged and its LR-2c rebaseline
  is pending coordinator integration.

### Review conditions closed

1. **C1 / default standalone XMP:** the exact-source bucket is inserted only
   after a geometry/CA approximation was actually decoded, not merely because
   ACR's default center/focal keys were present. Synthetic Upright-Off ACR XMP
   has no catalog-source bucket and a fixed BLAKE3 serialized-recipe fingerprint:
   `bd82c6ac0009c1f11342a8d837117c3c6f598591f43feb7096b1217d8cca0dbb`.
   The original XMP remains available for source-preserving export.
2. **C2 / immutable history:** the public geometry decoder rejects a recipe
   with existing history before changing the recipe or warnings. It never
   clears or rewrites history and no longer chooses a label. Standalone XMP
   records once after geometry and the native companion; catalog XMP and Lua
   use an uncommitted shared-codec decode, run the hooks, then record once.
   The existing caller label/source (`Import XMP` / `xmp`) is preserved.
   Tests pin one replayable entry for composed tone, CA, and Upright imports,
   as well as a byte-for-byte unchanged user-edited recipe on rejection.
   LR-2c's additional hooks still require integration on the coordinator's tree.
3. **LR-DIAG conversion:** sidecar returns
   `Vec<ApproximateEntry { adobe_key, field, reason }>`; it writes no diagnostics
   key. The catalog adapter alone calls
   `import_lrcat::diagnostics::push_approximate` for each entry. Tests read through
   `diagnostics::entries`. No shim or ad-hoc channel remains. Valid approximations
   retain exact source, use the matrix's exact recipe path, and produce no warnings.
   The shared helper deduplicates repeated Lua/XMP decoding.
4. **Stale CA:** nonzero Adobe PV2012+ legacy coefficients remain inactive and
   retained, and now emit info with `ignored (PV2012+)` in the shared channel,
   as specifically requested by review. Zero coefficients emit no approximation
   entry and create no history edit.
5. **Renderer/FFI:** the resident fallback test now also requires a nonzero
   pixel difference from the same render with both CA fields absent. The new
   synthetic PNG FFI test first verifies a saved matrix and mode tag are present,
   changes mode through `DevelopSession::set_settings`, then verifies both are
   cleared and the requested mode is active.

### LR-SCHEMA first-lane checklist

- Registered four predicates: `upright_homography`,
  `upright_homography_mode`, `legacy_ca_red`, `legacy_ca_blue`, each with its own
  `assert_bumped_only_when_present` test. `RECIPE_SCHEMA_VERSION` stays **3**;
  feature-bearing recipes write **4**, lifting the writable maximum to 4.
- The import fixture + 300-image schema test now independently checks the four
  feature fields and expects 3 or 4 accordingly. Three synthetic feature imports
  separately prove schema 4. Existing bundled/common golden fixtures use none of
  these features; the existing catalog digest and all four compatibility
  fingerprints pass unchanged, so no unrelated digest was re-pinned.
- Sidecar and merge round-trip tests now exercise a real CA feature, assert the
  loaded version is 4, and compare content with the in-memory version normalized.
- Added the FFI journal test proving `save_local_recipe` persists schema 4 in
  the envelope for a feature-bearing recipe. The existing no-feature legacy
  envelope test remains the counterpart.
- Sticky bumps, pinned-RAW ceiling, write refusal above max, and the documented
  pre-v4 FFI read limitation retain LR-SCHEMA's behavior.

### RED/GREEN audit

- The initial focused RED run observed **eight failures**: four absent predicate
  registrations, missing PV2012+ info, default-XMP bucket pollution, user-history
  rejection, and standalone import author preservation. The FFI RED run separately
  observed the schema-4 journal failure.
- The hook-order regression already passed at RED. The FFI mode-change test
  also passed before this implementation; it covers LR-7c's existing behavior.
  The CA/no-CA assertion is additional coverage of existing rendering, not a
  claimed newly failing test.
- GREEN added first-lane round-trip/schema coverage, pinned the default-XMP
  fingerprint, and strengthened the FFI test's setup/presence assertions.
  These additions were not all separately observed failing.
- One intermediate broad run used the initial uncommitted decode before its
  premature `Recipe::validate` call was removed. It failed with
  `settings do not match history head`, was stopped, and is not final evidence.
  The corrected decoder validates after the caller records the transaction.
- **LR-7c process correction requested by Machine A:** GREEN adjusted the RED test
  setup and added four tests that were never seen failing. Earlier RED/GREEN
  wording must not be read as claiming that every LR-7c regression was observed
  failing before its implementation.


### Final gates and measured timing

Environment: PATH includes `$HOME/.cargo/bin`;
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-7-upright`,
`CARGO_BUILD_JOBS=3`, `RAYON_NUM_THREADS=3`. Rust gate test concurrency is 3.

Before the final build, ran `cargo clean -p engine-api -p import-lrcat -p sidecar
-p merge -p pipeline-cpu -p image-core -p tessera-ffi -p pipeline-gpu`:
112,028 files / 34.9 GiB removed. The first unfiltered debug attempt passed the
engine-api and image-core unit targets but was interrupted during the long
image-core fixture-extremes test. It is **not** a completed gate. The same full
eight-crate gate was then completed in release, with no command-line skips.
This also enabled the release-only 20,000-image import scale test, which passed.
No test threshold was changed.

| Gate | Result |
| --- | --- |
| Full eight-crate `cargo test --locked --release ... --no-fail-fast -- --test-threads=3`, including all tessera-ffi targets | **PASS**, exit 0; **1340 passed, 0 failed, 48 ignored**, 210 target summaries |
| Eight-crate `cargo clippy --locked ... --all-targets -- -D warnings` | **PASS**, exit 0 |
| `cargo fmt --all -- --check` and `git diff --check` | **PASS** |
| `apps/mac/build-ffi.sh` | **PASS**, arm64 archive; regenerated bindings unchanged |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**; 912 XCTest, 3 skipped, 0 failures; 5 Swift Testing tests passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | **PASS**, exit 0, 279.92 s |

The eight crates are engine-api, import-lrcat, sidecar, merge, pipeline-cpu,
image-core, pipeline-gpu, and tessera-ffi. The full gate includes the shared
diagnostics writer scan, all matrix negative controls, report aggregation and
resume tests, the FFI mode-change regression, and schema-4 journal serialization.

**Timing repeats:** used the exact test binaries produced by the clean release
gate, first with 3 test threads and then serially (1), with output enabled.
This avoids rebuilding a different artifact between timing measurements.

| Check | 3-thread repeat | Serial repeat |
| --- | --- | --- |
| Liquify 20 MP brush+preview p95, unchanged limit <250 ms | **15.5 ms**, all 15 module tests passed | **23.2 ms**, target passed |
| Interactive drag frame delivery | **39 frames / 222.8915 ms** burst, final L1 | **39 frames / 209.207625 ms** burst, final L1 |
| Slider while export is active | **120 frames / 2.424813 s**, set-to-frame p90 **5.6 ms** | **120 frames / 2.193971 s**, set-to-frame p90 **4.3 ms** |

All three timing tests also passed in the full gate. Its successful test output
was captured by Rust's harness, so the numeric values above are from the explicit
measured repeats, not invented full-gate measurements. A first frame-repeat
selector matched sidecar's same-named `develop` binary and selected zero tests;
that attempt is excluded. The corrected repeats ran both tessera-ffi starvation
tests and passed. No latency or frame-delivery threshold was relaxed. These are
release measurements; they do not retroactively turn LR-7c's debug p95 failures
into passes.

**Coverage limits:** the 48 Rust ignores are unchanged opt-in tests, not added
filters. Each name/reason is listed in [LR-7d-evidence.md](LR-7d-evidence.md).
They include large/manual performance runs, fixture/weight/exclusive-lane
qualifications, the explicit real-catalog acceptance test (not run), and the
pre-existing signed-luminance reference-bound follow-up. The three Swift skips
are the opt-in 20k-file library measurement and two Smart Preview acceptance
tests requiring `TESSERA_SMART_PREVIEW_RAW`; both opt-in environment variables
were unset. No real catalog was opened, no GUI app was launched, and every new
fixture/test input is synthetic. Existing standard-suite fixture tests ran.

The final branch-wide whitespace check also trimmed inherited blank EOF lines
from four older evidence logs; their substantive output is unchanged.
No dependency, Cargo.lock or board.json change. No push or remote write.
The coordinator-owned untracked `LR-RULINGS-FROM-A.md` is left untouched.
The remaining integration action is LR-2c's `global.lua` rebaseline and merged
hook-order qualification after LR-2c actually lands.

## LR-7e — ignored diagnostics and main fingerprint qualification (2026-10-01)

Local-only continuation on `wp/LR-7-upright`, directly on `c7cac521`; no rebase.
Tests: `d01167ab`; implementation: `41e65318`. The following `docs(LR-7e):`
commit is the final lane HEAD. All three use the requested co-author trailer.

- Shared `push_ignored(recipe, adobe_key, lane, reason)` stores an info-level
  `ignored` entry with no `field` member. It shares the approximate writer's
  append/dedupe/no-clobber implementation. The reader accepts absent fields;
  existing approximate entries still serialize with their populated field.
- Sidecar returns a typed `GeometryEntryKind::Ignored` for stale PV2012+ legacy
  CA. The catalog adapter dispatches it to `push_ignored`; it no longer claims
  the empty legacy CA field was populated. The matrix guard filters to
  `status == "approximate"` and tests ignored notes in both row statuses.
- The known schema fixtures contain zero LR-7 features: the test explicitly
  expects version 3 for both required and serialized versions. Separate
  feature-bearing synthetic imports still explicitly require version 4.
- `crates/sidecar/UNMAPPED.md` now states that standalone XMP imports carry no
  translation notes. Catalog imports alone persist the decoder's info data.
- **Lua calls `apply` twice**: first through `xmp::parse_unrecorded` on the
  generated packet, then after Lua source retention. This is harmless: the
  assignments are idempotent, shared diagnostics dedupe identical entries,
  and both calls precede the single `finish` history transaction. The existing
  one-entry replay test passes. This pass documents rather than changes that
  ordering.

### Tests-first evidence

Before production edits, the stale-CA regression failed with `approximate`
instead of `ignored`, and the translated-row guard regression rejected an
ignored entry. The ignored-only approximate-row negative control already passed
(the approximate branch already checked status). The helper unit tests failed
compilation because `push_ignored` did not yet exist. An initial test-only
BTreeMap mutable-index error was corrected before observing behavioral failures.
The literal schema assertion and both compatibility packets are additional
coverage that already passed, not claimed newly failing regressions.
After implementation, all 55 targeted diagnostics/import/matrix/schema tests
passed, including ignored-entry dedupe, round-trip and foreign-shape protection.

### Main versus tip: exact packet bytes

Created the requested detached worktree at `/private/tmp/tessera-LR-7e-base`
from **`ad93e333`**, with target directory
`$HOME/.cache/tessera-target/LR-7e-base`. A temporary sidecar integration probe
parsed each packet and serialized `Recipe::to_json()` on both base and LR-7e.
Both pairs passed direct `cmp`, in addition to equal BLAKE3 fingerprints:

| Packet | Bytes, base and tip | BLAKE3, base and tip |
| --- | ---: | --- |
| Original C1 packet (PV15.4, Upright Off, default center/focal keys) | 11075 | `bd82c6ac0009c1f11342a8d837117c3c6f598591f43feb7096b1217d8cca0dbb` |
| Extended C1 packet | 11276 | `9b94b7dff899062b78c11e353b61aa95735145585db9a59e4396782c92c41de9` |

The extended packet adds `UprightVersion="151388160"`,
`UprightPreview="false"`, `UprightTransformCount="6"`,
`UprightTransform_0="1,0,0,0,1,0,0,0,1"`, and
`ChromaticAberrationR="0"` / `ChromaticAberrationB="0"`.
Both exact packet literals and main-derived hashes are pinned in
`crates/sidecar/tests/lr7e_acr_compat.rs`. These are serialized-recipe compatibility
checks, not Adobe pixel-parity claims. There is no base-versus-tip byte diff.
The temporary probes were removed, the detached worktree removed, and its
external target directory cleaned (899 files / 256.2 MiB); both paths were
verified absent.

### Final LR-7e gates

Environment: `PATH="$HOME/.cargo/bin:$PATH"`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-7-upright`,
`CARGO_BUILD_JOBS=3`, `RAYON_NUM_THREADS=3`.
Before the final gate, `cargo clean -p import-lrcat -p sidecar -p engine-api`
removed 10,369 files / 2.1 GiB. No production changes followed the gate.

| Command | Result |
| --- | --- |
| `cargo test --locked --release -p import-lrcat -p engine-api -p sidecar -p merge -p tessera-ffi -- --test-threads=3` | **PASS**, exit 0; **918 passed, 0 failed, 30 ignored**, 121 target summaries; clean build 8m 13s |
| `cargo clippy --locked -p import-lrcat -p engine-api -p sidecar -p merge -p tessera-ffi --all-targets -- -D warnings` | **PASS**, exit 0; 40.48s |
| `cargo fmt --all -- --check` and `git diff --check` | **PASS**, exit 0 |

The 30 ignores are existing opt-in benchmark, exclusive fixture/runtime and
real-catalog acceptance tests; no test filter or threshold was added or changed.
Both release synthetic scale tests (`streaming_import_memory_is_flat_and_time_bounded`
and `ffi_streaming_memory_is_bounded`) passed. Existing libraw C `sprintf`
deprecation warnings appeared during compilation; Rust clippy passed with
warnings denied. No Swift changes, so no Swift gate/build was required.
No app launch, no real user catalog access, no remote write, and no Cargo.lock or
board.json changes. The untracked coordinator-owned `LR-RULINGS-FROM-A.md` remains
untouched. The worktree and target created solely for base comparison were removed.
