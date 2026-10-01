# LR-0 — translation matrix and synthetic retention guard

Completed locally on Machine B, 2026-10-01. Branch: `wp/LR-0-matrix`.
Coordinator reads this file; no mailbox or push was used.

## Exact commit chain

- Requested main base: `e6c3e5da`.
- Actual clean starting HEAD: `87ff1ff173e4d6d2053a534b7cfd9343aef906e1`
  (the brief's Machine B ownership update directly atop that base).
- RED: `f6df1f4d95c1868084e399d5458585d10f455cfc`
  — `test(LR-0): guard translation matrix against retained synthetic imports`.
- GREEN: `bc6c05f9946d3a6f7f1a6bbede90357bf95c5f95`
  — `feat(LR-0): inventory Adobe translation paths and lane limitations`.
- The following `docs(LR-0):` commit adds this handoff only. Its hash is reported
  in the final agent summary; it cannot embed its own hash in its contents.

All lane commits end with the requested Claude Fable 5.1 co-author trailer.

## Delivered

`docs/coordination/LR-TRANSLATION-MATRIX.md`: 107 explicit/family/context rows;
21 retained, 78 unsupported-diagnostic, 8 translated. These are source-inventory
counts, not catalog frequencies. Lane row counts: LR-1 1, LR-2 46, LR-3 3,
LR-4 11, LR-5 6, LR-6 1, LR-7 39. Eight perspective controls were already
translated before LR-0. No new translation is claimed.

The matrix covers unmapped KEY_MAP entries, pending structures, extended curves,
legacy CA controls, named brief families, nested mask diagnostic cases and an
unknown-key catch-all. It records existing recipe paths or missing capabilities,
per-lane limitations and generic invalid/duplicate/whole-row fallback handling.
Residual lane allocations are proposed for coordinator review.

`crates/import-lrcat/tests/translation_matrix.rs`: two cheap tests, using only
invented Lua literals through the actual develop decoder. Every translated row
must provide a concrete key/value and valid recipe JSON pointer; retained source
or codec diagnostics fail the check. Matrix inventory coverage is checked against
unmapped/pending KEY_MAP entries and EXTENDED_TONE_CURVE_KEYS. A negative control
falsely calls PointColors translated and verifies rejection for retained source.

## RED / GREEN evidence

At the RED commit, before creating the matrix:

```text
cargo test --locked -p import-lrcat --test translation_matrix
matrix_guard_rejects_a_retained_key_claimed_as_translated ... ok
translation_matrix_matches_synthetic_import ... FAILED
translation matrix must exist: ... No such file or directory
1 passed; 1 failed; finished in 0.00s
```

Initial test-profile build: 25.39 s. This was an actual failing test, not the
optional empty RED allowance. The test file was unchanged in GREEN.

After adding the matrix:

```text
cargo test --locked -p import-lrcat --test translation_matrix -- --nocapture
matrix guard checked 8 translated synthetic imports
2 passed; 0 failed; finished in 0.01s
```

## Gates

Environment used for every build/test/clippy invocation:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-0-matrix
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

Gates at GREEN, measured under shared-machine load with `/usr/bin/time -p`:

| Command | Result | Wall time |
| --- | --- | --- |
| `cargo test --locked -p import-lrcat -p engine-api` | PASS: 161 passed, 0 failed, 1 ignored across 22 result summaries | 52.39 s |
| `cargo clippy --locked -p import-lrcat -p engine-api --all-targets -- -D warnings` | PASS | 14.23 s |
| `cargo fmt --all -- --check` | PASS | 3.29 s |
| `git diff --check` | PASS | not timed |

The ignored test is the existing release-only scale/timing/allocation test;
no tests were skipped manually. No other crate was modified, so no additional
`-p` gate was needed. Logs on this machine: `/tmp/tessera-LR-0-red.log`,
`/tmp/tessera-LR-0-green.log`, `/tmp/tessera-LR-0-tests.log`,
`/tmp/tessera-LR-0-clippy.log`, `/tmp/tessera-LR-0-fmt.log`.

## Limits and downstream blockers

No blocker remains for LR-0. No translation or CPU-render parity work belongs to
this source-only lane. The guard tests Lua develop decoding, not full catalog
materialization, XMP parity, rendered pixels or exhaustive structured variants.
The eight translated examples test absence of retention/diagnostics and path
existence; they are not Adobe output goldens. Downstream lanes still need the
brief's synthetic import/render acceptance tests before promoting structures.

Recipe gaps are documented, especially HDR curves, monochrome mixer, legacy
brightness/PV2010 fidelity, imported mask/depth resources, per-component enabled
flags/nested group trees, detailed bokeh, arbitrary homographies and cloud removal
results. Existing point-color range, AI category/model and perspective fields do
not by themselves establish full Adobe fidelity. Unknown families are open ended.

## 29c compatibility / scope

The live exact-source slot is
`unknown["lrcat_develop_source"]["properties"][adobe_key]`, with `lua-values`
or `xmp-fragments` shape. Partially decoded and inactive pending structures remain
retained unconditionally. Legacy CA diagnostic retention may instead be under
`crs:*`; raw-text/cell-descriptor fallbacks cover whole-row failure/oversize cases.
The matrix and guard respect those distinctions.

No production Rust, lua_develop.rs, xmp.rs, recipe schemas, dependencies,
Cargo.lock, board.json, fixture goldens or Swift files changed. Consequently no
recipe output bytes change for any input. Only the matrix, test and this handoff
are committed. All fixtures used were synthetic. No private catalog was accessed,
no Swift gate ran, and no app was launched. Local commits only; coordinator owns
review and integration.
