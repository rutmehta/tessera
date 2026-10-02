# LR-2c validation evidence — 2026-10-01

Local lane: `wp/LR-2-tone-curves`, rebased onto
`02ae81960e104376bcfa6545d39515c3d1592454` without squashing the eight prior commits.
RED: `85662ed3`; implementation: `c2df1958`.

## Final result

All LR-2c functional regressions pass. The requested full test gate is **not
fully green**: the unchanged Liquify performance test exceeds its 250 ms p95
limit, including in a serial isolated rerun. Do not describe this as a passing
full gate or as a proven LR-2 performance regression; no base-versus-tip timing
comparison was performed. No threshold or assertion was weakened.

| Run | Passed | Failed | Ignored | Filtered | Result |
| --- | ---: | ---: | ---: | ---: | --- |
| Initial clean eight-crate run | 1260 | 2 | 49 | 41 | Stale FFI error-text expectation and Liquify timing |
| Final clean refresh, requested six crates | 1115 | 1 | 47 | 37 | Liquify timing only |
| Corrected saved-process test, isolated | 1 | 0 | 0 | 216 | Pass |
| Liquify timing, isolated serial | 0 | 1 | 0 | 14 | Threshold still exceeded |

Counts are separate runs, not unique tests summed across runs. Exclusion names
may match several test targets, so filtered counts differ from the 23-name list.
The eight-crate run additionally covered changed `image-core` and
`pipeline-adobe`; their suites passed. Neither crate changed after that build.
Late importer/GPU changes and the corrected FFI assertion were then covered by
the final six-crate refresh. Final eight-crate Clippy with `-D warnings` passed (54.57 s);
`cargo fmt --all --check` passed. The refresh script returned 101 solely
because it preserved the failed Cargo test status.

| Liquify 20 MP run | Median event | p95 event | Full-resolution apply |
| --- | ---: | ---: | ---: |
| Initial broad run | 229.8 ms | 343.9 ms | 4298 ms |
| Isolated, `--test-threads=1` | 320.6 ms | 600.0 ms | 2981 ms |
| Final broad refresh | 220.6 ms | 266.9 ms | 2874 ms |

The saved-process failure was an outdated `Adobe PV3–6 required` expectation
left behind when LR-2b added PV1–2 rendering. It now expects `Adobe PV1–6
required`, still rejects revision 99 and still checks no writer construction.
It passes both alone and in the final full FFI unit suite.

## Reproduction and logs

Environment: `PATH=$HOME/.cargo/bin:$PATH`,
`CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-2-tone-curves`,
`CARGO_BUILD_JOBS=3`, `RAYON_NUM_THREADS=3`.

`tools/orchestrate/wp/LR-2/gates-c.sh` cleans all eight relevant crates and runs
Cargo tests with `--locked --features import-lrcat/fixture --no-fail-fast`, then
Clippy (`--all-targets -- -D warnings`) and `cargo fmt --all --check`.
The first run stopped before static checks because Cargo tests returned 101.
The final refresh script preserves that test exit status while still running
Clippy and formatting. It cleans `import-lrcat`, `pipeline-gpu`, and `tessera-ffi`
after their final edits, then tests all six requested crates. The other touched
crates were cleaned before the original gate and unchanged afterward.

Durable local artifacts: `/Users/rutmehta/tessera-evidence/LR-2c/`:

- `lr2c-gates.log`: complete initial eight-crate run.
- `lr2c-refresh.sh`, `lr2c-refresh.log`: exact final refresh and results.
- `lr2c-process-green.log`: exact saved-process rejection regression.
- `lr2c-latency-serial.log`: isolated timing rerun, assertions unchanged.
- `lr2c-clippy.log`: additional prior eight-crate Clippy pass.
- `lr2c-red*.log`: observed RED failures before fixes.
- `lr2c-digest-green.log`, `lr2c-resident-green.log`,
  `lr2c-schema-green.log`, `lr2c-import-final-targets.log`: focused GREEN evidence.

## Coverage and boundaries

RED failures covered inactive settings/hash/history drift, extended-curve
overwrite, user-facing approximation warnings, matrix contract, legacy
precedence, legacy GPU rejection, B&W channel-toning loss, resident effects
fallback and a digest-only PV2010 row changing modern settings. The schema
predicate first failed compilation because its API was absent.

The final Cargo tests cover the original golden unchanged; byte-identical
absent/disabled-field behavior; normal plus extended curves; exact source and
info-only approximation diagnostics; Lua/XMP parity; one replayable import edit;
Adobe-family legacy gating, Shadows=5 and legacy precedence; schema predicates;
CPU/Adobe B&W channel toning; native/Adobe GPU legacy sessions against CPU;
resident B&W with and without local tone. The matrix guard has negative controls
for lost source, missing diagnostics, and accidental warnings.

Synthetic fixtures only. No real catalog or external RAW fixture was opened.
The exact 23 external-RAW exclusions are listed in `gates-c.sh` and copied in the
refresh script. Existing ignored benchmark tests stayed ignored. No Swift gate
or app launch was performed.

All 107 main matrix keys remain (109 final including Recovery/Blacks aliases).
`tests/golden.rs` matches the rebased main verbatim; original digest:
`d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`.
No Cargo.lock, Cargo.toml, dependency, board, or Swift changes. No push.

Adobe calibration and internal B&W ordering remain unverified, so the active
LR-2 mappings are explicitly approximate, source-preserving and info-only.
The shared LR-SCHEMA helper was absent on rebased main: this lane supplies the
tested `required_schema_version_lr2()` predicate, with coordinator integration
required at merge. This branch alone does not globally write/refuse schema v4.
