# LR-4d: shared mask approximation diagnostics

Local lane `wp/LR-4-parametric-masks`, 2026-10-01.

**Implementation complete; the full gate is not entirely green.** All requested
crate suites were run, including repository RAW fixtures. After serial reruns,
the only remaining failure is the unchanged Liquify p95 assertion: 349.0 ms in
the broad run, 322.6 ms serially, against a 250 ms limit. No threshold was changed.

## Rebase and scope

Rebased all 23 LR-4..LR-4c commits from `1fb4fdc8` onto fetched
`origin/wp/LR-DIAG` at `38684d9bdb34f6f6b9178f8589a839f68cb23832` without
squashing. Conflicts were confined to the translation-matrix test and matrix
prose. Kept the shared LR-DIAG guard and all its negative tests. All 107 base
matrix keys remain; this branch has 114 keys. No dependency manifests,
Cargo.lock, board.json, Swift, or application changes in LR-4d.

## Diagnostics conversion

- Removed the LR-4 top-level-array writer. Both XMP and Lua (which goes through
  the XMP translator) now call `diagnostics::push_approximate` via the shared
  mask-source recorder. There is no shim or direct write of the shared key.
- Converted LR-4 tests to `diagnostics::entries`. Deleted the lane-local
  `check_mapping` guard. Adapted the shared guard's importer and source lookup
  to slash-qualified matrix keys; all shared conditions remain enforced.
- Diagnostics name concrete populated recipe paths, retain exact source, use
  lane `LR-4`, level `info`, status `approximate`, and emit no warning. Retained
  Midpoint/Roundness metadata does not claim its own translated field.
- The umbrella `MaskGroupBasedCorrections` row names
  `/settings/locals/adjustments`, matching its ordered-composition reason across
  brush, range, and gradient imports. Component-specific matrix paths are
  unchanged. A regression checks every approximate LR-4 row's key, exact field,
  lane, level, status, reason, retained source, and absence of the legacy channel.
- Only the four already-promoted brush/colour fixture envelope pins change
  (indices 0, 1, 7, 8 in `lr4b_retained`), because their diagnostics changed shape.
  Unsupported fixture pins and the 44-group / 2,000-row goldens are unchanged.

## Schema and rendering

`V4_FEATURE_PREDICATES` retains `mask_component_disabled`, `mask_groups`, and
`mask_luminance_bounds`, each with `assert_bumped_only_when_present`. Traversal
includes nested/disabled subtrees, retouch components, and history base.
Dabs use existing BrushStroke points/radius/feather/flow/erase; colour models use
existing ColorRange samples/amount. They introduce no field an older writer
would drop. A new test pins their schema-3 byte-stable round-trip. No renderer,
GPU admission, mask ordering, depth limit, or buffer reuse code changes in LR-4d.

## Validation

The tests-first commit is `607b555a`. `red-matrix.log` records three failing
matrix tests before conversion; ten shared guard controls passed. The converted
matrix suite has 13 passing tests.

Implementation commit: `19aab189` (`fix(LR-4d):`). All LR-4d commits carry the
requested Claude Opus 5.5 co-author footer.

Build and test environment:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-4-parametric-masks"
export CARGO_BUILD_JOBS=3 RAYON_NUM_THREADS=3
```

`cargo clean -p import-lrcat -p engine-api -p tessera-ffi` preceded the original
broad gate, and was repeated before the final remaining-crate gate
(`gate-clean.log`, `gate-clean-final.log`). No thresholds were edited.

The original requested 11-crate invocation is recorded in `gate-tests.log`.
It completed compositor (330 passed, 11 ignored), filters (153 passed,
8 ignored), and image-core (118 passed, 2 ignored), including RAW fixtures.
It then stopped with three matrix failures: that invocation had compiled the
importer before the final umbrella-field adjustment, while the matrix file had
already moved to `/settings/locals/adjustments`. Its old binary still emitted
the luminance-bounds path. Those three failures pass in the freshly rebuilt
final importer run and the clean remaining-crate run. Rendering-crate source
is identical to `1fb4fdc8`; the earlier completed rendering suites remain valid.
The three crates' doc-tests were run separately and passed (`gate-doc-tests.log`).

The final clean remaining-crate command is:

```sh
cargo test --locked --no-fail-fast -p import-lrcat -p engine-api \
  -p pipeline-cpu -p pipeline-gpu -p sidecar -p merge \
  -p tessera-ffi -p tessera-mcp
```

Supplemental final checks:

| Check | Result | Evidence |
| --- | --- | --- |
| `cargo test --locked -p import-lrcat --no-fail-fast` | 109 passed, 1 ignored | `gate-import-final.log` |
| `cargo test --locked -p tessera-ffi lr4` | 19 passed | `gate-ffi-final.log` |
| `cargo clippy --locked --workspace --all-targets -- -D warnings` | exit 0 | `gate-clippy.log` |
| `cargo fmt --all -- --check` | exit 0 | `gate-fmt.log` |

## Complete requested coverage

Counts before the two serial reruns (the first three packages come from the
completed original broad run; the other eight from the fresh clean run):

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| compositor | 330 | 0 | 11 |
| filters | 153 | 0 | 8 |
| image-core | 118 | 0 | 2 |
| engine-api | 114 | 0 | 0 |
| import-lrcat | 109 | 0 | 1 |
| merge | 53 | 0 | 1 |
| pipeline-cpu | 172 | 0 | 3 |
| pipeline-gpu | 144 | 0 | 13 |
| sidecar | 70 | 0 | 0 |
| tessera-ffi | 583 | 2 | 30 |
| tessera-mcp | 84 | 0 | 0 |
| **Total** | **1930** | **2** | **69** |

The fresh eight-crate command exits 101 because of the two FFI assertions below;
it uses `--no-fail-fast`, so every requested target and doc-test ran. No RAW test
was excluded. The 69 ignores are the suites' existing ignored tests.

## Serial reruns and remaining gate failure

Both reruns use `--exact --test-threads=1 --nocapture`, with the same Cargo/Rayon
environment above. They ran sequentially after the broad gate ended.

| Assertion | Broad run | Serial rerun | Requirement / disposition |
| --- | --- | --- | --- |
| `brush_latency_on_a_20_megapixel_layer` | p95 349.0 ms; median 248.7 ms; failed | p95 322.6 ms; median 251.8 ms; failed (exit 101) | p95 < 250 ms; **still failing**, threshold unchanged |
| `export_batch_does_not_starve_slider_drag` | 0/120 frames at L2; render p90 6.0 ms; failed L2 assertion | 118/120 frames at L2; render p90 8.9 ms; passed (exit 0) | At least 90% at L2 and render p90 < 16 ms; **passes serially** |

All five images exported successfully in both export runs. The serial Liquify
run used the same 5472x3648 layer, cell 16, and 1824x1216 proxy as the broad run.
Its full-resolution apply was 4356 ms (broad: 3343 ms). These measurements were
made on the shared Machine B host; a serial test harness does not isolate other
lanes' processes.

Evidence: `gate-remaining-final.log`, `gate-liquify-serial.log`,
`gate-export-serial.log`, and `gate-serial-status.log`. `gate-counts.json` records
machine-readable counts and rerun results. After taking the serial reruns into
account, 1931 unique requested tests have passed, one remains failing, and 69
remain ignored. This is a combined-coverage count, not a claim that the broad
Cargo invocation exited successfully.

The Liquify implementation and latency test are unchanged by LR-4d. The archived
LR-4c serial run in `../LR-4/gate-lr4c-liquify-rerun.log` also failed that same
250 ms assertion at 307.3 ms p95; it is historical context, not a substitute for
the current measurements. No unrelated rendering/performance fix was made.

## LR-4c guarantees verified

- The 44-group and 2,000-row golden files and their expected values are unchanged
  from `1fb4fdc8`, and both tests pass in the clean final run.
- CPU mask-tree tests pass, including disabled seed behavior, nested
  intersection/subtraction/inversion, and the pre-allocation nesting bound.
  Engine-api accepts eight levels and rejects the ninth.
- GPU admission tests reject nested trees and four-bound ranges before dispatch;
  fallback/render comparison tests pass. All rendering-crate code (including
  first-child buffer reuse) is identical to `1fb4fdc8`.
- FFI's `lr4c_component_inversion_precedes_range_intersection` passes, as do
  the Dabs, colour-range, erase-dab, retained-source, and schema-4 journal tests.
- Final work is local only. No push, board update, dependency/lockfile change,
  Swift gate, application build, or real Lightroom catalog access.
