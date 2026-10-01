# LR-3d evidence

Synthetic regression evidence and local Rust verification for the Machine B
retouch follow-up. No personal catalog, Swift gate, app launch, board edit or
push. All new fixtures are generated; references to an existing timing fixture
are called out separately below.

## Rebase and changes

The ten existing lane commits were rebased without squashing onto `c7254291`,
which includes LR-SCHEMA `02ae8196`. No conflict resolution was required. All 107 inherited matrix rows are preserved;
only the two retouch rows change.
The comparison worktree was detached from the subsequently observed
`origin/main` at `87536669` (B5-42/42b accessibility merge). A file diff confirms
no Rust crate or Cargo manifest/lockfile difference between these base commits.

RED commits: `36a30ea4`, `bb4d3353`, `a9845aed`, `6b54a3ee`.
Functional fixes: `6693781e`, `72be4015`.
The final documentation commit follows the verification and cannot name itself.

## RED observations

- Import retouch suite: 2 passed, 7 failed. It exposed discarded source,
  duplicate history, missing diagnostics/schema bump, and rejected provenance.
- FFI `lr3d` render selection: 1 passed, 4 failed. Independent second-spot
  feather/opacity passed; stage ordering, target resolution, opacity union and
  immutable source snapshot failed for the expected reasons.
- Schema predicate harness: failed because `retouch` was absent from the list.
- Pipeline geometry/cache selection: both tests failed. After fixing the stage
  key, the geometry fixture was corrected to use supported crop rotation rather
  than the explicitly unsupported recipe-level EXIF orientation field.
- Exact GPU/CPU pixel equality initially failed with maximum absolute difference
  about `1.23e-6`. The fix uses the CPU f32 chain for retouch on either backend;
  the assertion remains bit equality, including non-default tone and a local
  adjustment.
- A follow-up inactive-spot check failed at reduced resolution: disabled or
  zero-opacity spots had changed the resampling order. Early resampling now
  requires an enabled spot with positive opacity; the nonempty-list renderer
  requirement remains unchanged. The preliminary broad run was stopped so the
  final gate could run against this correction.
- MCP requested-size regression failed at a 32px request. It passed after using
  the requested retouch level for both RGB and graph previews.

## Focused GREEN coverage

- Import retouch: 9 passed; schema-by-content import fixture: 1 passed.
- Matrix guard: positive synthetic imports plus separate negative conditions
  for missing field/source/info and nonzero warnings.
- Schema predicate harness: passed. FFI journal stores schema 4: passed.
- Real-brush FFI integration: 16 passed before the final broad gate. It includes exact
  CPU/GPU comparison, two spots at 50%/25% opacity and 50%/100% feather,
  non-default contrast/saturation, independent pixel-space clone/heal reference,
  tone-plus-local stage order across all three render paths, source/union behavior,
  inactive-spot identity, scaled invocation dimensions,
  file/print/HDR export, library previews, headless detail sessions, and synthetic
  EXIF 5–8 export orientation after crop and distortion.
- Catalog/direct-render integration: 2 passed. Pipeline error/geometry/cache:
  3 passed. MCP assembly/requested-size regression: passed.

No Adobe-rendered evidence was used. These tests establish Tessera behavior and
backend equality, not Adobe retouch equivalence. Both source keys remain
`approximate` with exact source retained and info diagnostics, no warnings for
successfully mapped keys.

## Full Rust gates

Final clean gate at functional tip `72be4015`: **passed** in 2,269.49 s.
Across 266 test-result groups: **1,514 passed, 0 failed, 60 ignored, 47 filtered**.
This includes all twelve packages listed below. `cargo clippy --all-targets --
-D warnings` passed in 104.48 s; `cargo fmt --all --check` passed in 2.56 s.
`git diff --check` passed, and the Cargo manifest/lockfile diff is empty.

Environment for the final clean build and **both** timing sides:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-3-retouch
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
export CARGO_PROFILE_DEV_DEBUG=0
export CARGO_PROFILE_TEST_DEBUG=0
export CARGO_INCREMENTAL=0
```

Debug symbols and incremental compilation were disabled after disk headroom
fell to about 11 GiB. Optimization remains the ordinary unoptimized test profile;
no test threshold, Cargo manifest or lockfile was changed. The first broad run followed a full
`cargo clean` of the dedicated target; the final restart cleans all twelve listed
workspace packages with `cargo clean -p ...`. Logs live outside the cleaned
target under `/tmp/lr3d-*.log`.

The full command is `cargo test --locked` for `import-lrcat`, `engine-api`,
`pipeline-cpu`, `pipeline-gpu`, `brush`, `image-core`, `export`, `previews`,
`tessera-mcp`, `sidecar`, `tessera-ffi`, and additionally `merge`, with
`--no-fail-fast -- --test-threads=3`. Explicit `--skip` filters exclude RAW-fixture
checks and the two timing checks (run alone below). The exact filter names are
in [LR-3d-GATE-SKIPS.txt](LR-3d-GATE-SKIPS.txt), one `--skip` per line. Default ignored tests remain
ignored. This is not a full real-RAW gate.

`cargo clippy --locked` uses those same packages, `--all-targets -- -D warnings`.
`cargo fmt --all --check`, `git diff --check`, and a manifest/lockfile diff check
are also required. Existing LibRaw C++ deprecation diagnostics are separate from
Rust warnings-denied checking.

## Serial base versus tip timing

The Liquify test ran serially, base then tip, with identical settings. Both
failed the unchanged `p95 < 250 ms` assertion (exit 101). This demonstrates a
base failure on this machine, but does not establish that tip has no latency
regression. Other lanes were observed running tests on the shared machine;
these single runs are not a controlled throughput comparison.

The export/slider test was **not run on either side**: it requires five existing
RAW fixtures and has no synthetic mode. The task also requires synthetic fixtures
only. An explicit clarification was requested, but no exception was received.
No RAW fixture was opened for this comparison, no substitute fixture was used,
and neither the test nor its threshold was changed. This leaves blocker 4
partially unresolved even though the non-RAW functional gate passed.

The baseline has its own target directory and detached worktree. Commands:

```sh
cargo test --locked -p tessera-ffi --test document_liquify_ui brush_latency_on_a_20_megapixel_layer -- --exact --nocapture --test-threads=1
cargo test --locked -p tessera-ffi --test develop export_batch_does_not_starve_slider_drag -- --exact --nocapture --test-threads=1
```

| Test | Base `87536669` | LR-3d tip |
| --- | --- | --- |
| 20MP Liquify brush latency | FAIL: median 239.1 ms; p95 344.2 ms; max 428.0 ms | FAIL: median 213.2 ms; p95 375.4 ms; max 462.8 ms |
| Export versus slider starvation | BLOCKED: real RAW exception not authorized | BLOCKED: real RAW exception not authorized |

Liquify baseline open/apply: 3,021/2,758 ms; tip: 2,176/2,728 ms. The 120
brush-plus-preview events use a 5,472×3,648 synthetic layer and 1,824×1,216 proxy.
Test-only wall time: base 48.45 s, tip 41.27 s. Cargo wall time including builds:
base 255.82 s, tip 76.08 s. Logs: `/tmp/lr3d-timing-{base,tip}-liquify.log`;
command/exit records: `/tmp/lr3d-timing-results.json`.

The temporary baseline worktree and its dedicated target were removed after
measurement.
Full-sensor loupe retouch remains a PERF follow-up; see [HANDOFF.md](HANDOFF.md).

## Golden and merge obligations

The final synthetic 2,000-image digest is
`7022e432ed77c0c42227de06f331090e8a43d4e06ca4749959f136a26b659763`.
The 200 retouch rows now include exact source, approximate info diagnostics,
one import history entry and conditional schema 4. Non-retouch import fixtures
continue to require/write schema 3.

Recompute the combined digest with LR-6. Preserve `parse_without_retouch` and
LR-4's following mask hook. Replace the marked private diagnostics shim and
matrix reader with LR-DIAG's shared helper/entries API when rebasing onto it.
