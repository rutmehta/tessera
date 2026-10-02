# LR-2d — shared schema and diagnostics conversion

Branch `wp/LR-2-tone-curves`, Machine B, 2026-10-01. Local commits only;
the coordinator owns the force-push. This supersedes LR-2c's standalone schema
hook and ad-hoc diagnostics channel. Rendering algorithms are unchanged.

## Rebase

Rebased the eleven LR-2/2b/2c commits from `a6a8144e` onto
`origin/wp/LR-DIAG` at `38684d9b`, without squashing. Resolved the matrix guard
conflicts by keeping LR-DIAG's injectable guard and all negative tests, plus
LR-2c's exact-source checks. Synthetic imports select Adobe PV2010 for legacy
keys and HDR mode for extended curves. Kept the shared diagnostics legend.
All 107 base matrix rows remain, no other lane's row changed, and LR-2 adds
its `Blacks` and `Recovery` alias rows (109 total).

## LR-SCHEMA and first-lane checklist

- Registered `monochrome` (enabled only), `curves_extended` (present), and
  `legacy_pv2010` (present) in `V4_FEATURE_PREDICATES`. Each has an
  `assert_bumped_only_when_present` test. Deleted `required_schema_version_lr2`;
  its former integration test now calls the shared API.
- First real entries lift the writable ceiling to 4. Default and disabled
  monochrome (even with a nonzero dormant mixer) still require/write 3.
- The bundled and 300-image synthetic catalog fixtures contain no active LR-2
  feature: their schema-3 assertions remain intact. A separate synthetic
  import test proves each active feature writes 4 without mutating the caller.
- Original `import-lrcat/tests/golden.rs` is verbatim unchanged, including digest
  `d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`.
  No golden was re-pinned.
- Inspected `sidecar/tests/roundtrip.rs` and `merge/tests/recipe.rs`: neither
  fixture uses these features, so equality assertions need no change. Updated
  LR-2b's active-tone roundtrip expectation to schema 4; process version and
  the caller's schema remain unchanged.
- A new sticky-bump test proves a saved/reloaded schema-4 recipe stays 4 after
  the feature is removed. `pinned_raw` already accepts the writable range.
- Added an FFI journal test: saving enabled monochrome through
  `save_local_recipe`, closing and reopening the journal, yields schema 4 in
  the envelope and preserves unknown envelope data. The existing no-feature
  legacy-envelope test remains. Older pre-v4 FFI builds cannot read a schema-4
  recipe through `get_recipe_json`; this shared LR-SCHEMA behavior is unchanged.

## LR-DIAG

- Every LR-2 approximate writer calls `crate::diagnostics::push_approximate`
  (the in-crate spelling of `import_lrcat::diagnostics::push_approximate`).
  Entries carry lane `LR-2`, the matrix recipe path, and the existing reason.
- Removed the `lrcat_develop_diagnostics` writer. No private shim, direct shared
  key writer, engine-api info writer, or ad-hoc replacement channel was added.
  LR-2 tests and guard readers use `diagnostics::entries()`.
- All 21 approximate rows are tested through both Lua and XMP. The shared
  guard checks field presence/non-default value, retained source, an
  info/approximate entry naming the exact matrix field, and zero warnings.
  LR-DIAG's negative tests and translated-with-diagnostic rejection remain.
  LR-2c additionally checks exact source literals and nonempty reasons.
- The shared FFI report automatically groups these entries; no app or Swift
  changes are needed. These remain approximations, not Adobe pixel-parity claims.

## Compatibility

Existing LR-2c no-op, single-import-history, replay, and GPU-to-CPU fallback
checks remain in the final gate. No rendering implementation changed. Synthetic
fixtures only; no external RAW images or real catalog used. No Cargo.lock,
dependency, board, Swift gate, app build, push, or install changes. The supplied
untracked `LR-RULINGS-FROM-A.md` is left untouched.

## Validation

RED commit: `a245f71b`. Observed four schema-unit failures (missing predicates
and missing sticky bump), the schema-4 journal failure (3 instead of 4), one
LR-2c missing-shared-entry failure, the active import schema failure, and four
matrix failures including the new XMP coverage. Existing shared guard negative
cases passed. The first green sweep exposed the stale LR-2b schema-3 assertion;
its replacement explicitly expects 3 before use and 4 after serialization.

Final gate evidence is recorded below. Reproduce with
`bash tools/orchestrate/wp/LR-2/gates-d.sh`: dedicated target directory,
three Cargo jobs and Rayon threads, package clean before tests, nine requested
crates including merge and tessera-ffi, all-target clippy with `-D warnings`,
and workspace fmt. The script explicitly lists the external-RAW exclusions;
existing ignored tests remain ignored. Timing thresholds are never changed.

### Clean broad run

`cargo clean` removed 54,209 package build files (21.0 GiB); the nine-crate test
build then completed in 3m 14s. The full `--no-fail-fast` run completed:
**1,353 passed, 1 failed, 50 ignored, 41 filtered instances**, exit 101.
The 23 named external-RAW exclusions are in `gates-d.sh`; some names occur in
multiple targets. Existing ignored tests were not enabled.

The sole failure was
`tessera-ffi` test `document_liquify_ui::brush_latency_on_a_20_megapixel_layer`:
median 250.9 ms, p95 **690.2 ms** against the unchanged **250 ms** limit,
maximum 1088.7 ms, full-resolution apply 2829 ms. Its other 14 tests passed.
No threshold, performance assertion, or functional test was weakened.

The full run confirms all new LR-2d tests, the LR-DIAG writer scan, both matrix
import formats, the original golden, inactive import/history regressions,
legacy/B&W GPU-to-CPU equality, and the FFI schema-4 journal. All other requested
crate targets passed. This is not a fully green test gate; no base-versus-tip
performance comparison was performed, so the timing result does not establish
a new LR-2d performance regression.

Local raw logs: `/tmp/lr2d-gates.log`, `/tmp/lr2d-red-schema.log`,
`/tmp/lr2d-red-diag.log`, `/tmp/lr2d-red-import.log`,
`/tmp/lr2d-red-journal.log`, `/tmp/lr2d-green.log`.


### Serial Liquify rerun

Command (same environment, after the broad run finished):

```sh
cargo test --locked -p tessera-ffi --features import-lrcat/fixture \
  --test document_liquify_ui brush_latency_on_a_20_megapixel_layer \
  -- --exact --nocapture --test-threads=1
```

Exit 101: **0 passed, 1 failed, 14 filtered**. Median 249.4 ms, p95
**357.6 ms**, maximum 444.4 ms, full-resolution apply 3736 ms; the limit remains
250 ms. Raw log: `/tmp/lr2d-liquify-serial.log`. These are separate runs, not
unique-test totals to sum. Both timing failures remain reported.

`gates-d.sh` includes the serial rerun, continues through clippy and fmt even if
there is a test failure, and preserves a nonzero gate status. The initial run
used the same clean/test command; serial timing and lint/format were then run
separately because its original fail-fast shell stopped after Cargo exit 101.

### Final lint, formatting, and audits

- The first clippy pass found `clippy::chunks_exact_to_as_chunks` in the new XMP
  test fixture builder. Replaced it with `as_chunks::<2>().0.iter()`; no
  production behavior changed. After `cargo clean -p import-lrcat`, the entire
  matrix target passed again: **14 passed, 0 failed**.
- Final nine-crate `cargo clippy --locked ... --all-targets
  --features import-lrcat/fixture -- -D warnings`: **exit 0**, 21.01 s.
- Final `cargo fmt --all --check`: **exit 0**.
- `git diff --check` and `bash -n gates-d.sh`: pass.
- Byte comparison against `a6a8144e` confirms `golden.rs` is unchanged.
  Matrix audit confirms every base row remains and every non-LR-2 row is
  unchanged. No Cargo.lock, Cargo.toml, or board edits in LR-2d.
- Native LibRaw compilation printed its existing C++ `sprintf` deprecation
  warning. No Rust clippy warning remains.

Final local logs: `/tmp/lr2d-matrix-refresh.log`,
`/tmp/lr2d-clippy-final.log`, `/tmp/lr2d-fmt-final.log`.

## Local commits

- Rebased lane tip before LR-2d: `d616f39d`.
- RED: `a245f71b` (`test(LR-2d):`).
- Fix: `cbc4c595d62e8e2761ec2f9b0aa03cd525c25d29` (`fix(LR-2d):`).
- The following `docs(LR-2d):` commit records this handoff and reproducible gates.

All three LR-2d commits carry the requested Claude Opus 5.5 co-author trailer.
No push was performed. The outstanding gate limitation is **Liquify p95 in
both broad and serial runs**; all other requested test targets and final lint
and formatting checks pass.
