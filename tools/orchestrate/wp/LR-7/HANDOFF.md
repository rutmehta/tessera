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
