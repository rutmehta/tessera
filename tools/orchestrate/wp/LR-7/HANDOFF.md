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
