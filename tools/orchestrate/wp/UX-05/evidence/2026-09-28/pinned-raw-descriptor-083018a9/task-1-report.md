# Task 1 — pure pinned RAW descriptor report

## Current candidate

Commit: `771a508f1d43a5ee7ad815b69987829d2e8062ac` on `codex/pinned-raw-descriptor`, based on `5f33e174198731c607ce4fa91e73586cc4505bd4`. The commit changes only `crates/engine-api/src/pinned_raw.rs`, `crates/engine-api/src/lib.rs`, `crates/engine-api/tests/pinned_raw.rs`, and `crates/engine-api/CONTRACTS.md`.

The pure descriptor stores private immutable declarations and exact recipe bytes. It requires schema 3, a matching explicit owner, explicit Raw source and Native revision 2, a settings object, default geometry, a positive declared asset length, a closed decoder route, and a lowercase-normalized 1–16 character ASCII-alphanumeric suffix. It rejects recursive duplicate decoded JSON keys and recursively unknown current-settings keys and type/array-shape loss. It parses `DevelopSettings` only; it creates an in-memory `Recipe` solely to use the established `recipe_hash()` implementation. History, selection, ids and unknown top-level metadata remain opaque and exact recipe bytes are retained. The separate input identity covers declared asset digest/length, owner, exact recipe payload digest, route and normalized suffix; it excludes locator hint. No asset verification, filesystem access, decoder/render/session/writer call, or pixel eligibility claim is made.

## Evidence

Focused command: `cargo test -p engine-api --test pinned_raw --release --jobs 2`, with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated`. The durable child-process runner is `/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor/run.py`. Final committed focused run `focused-committed` exited 0. Raw log: `/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor/focused-committed.log`; return code: `focused-committed-returncode.txt`. Before/after source snapshots are `focused-committed-before.json` and `focused-committed-after.json`; all four file hashes, full tracked-tree hash, HEAD and clean status/diff hashes match.

Earlier attempts are preserved as separate raw logs and direct return-code files:

- `red-tests` (101): compile failure because the module was absent; no assertion executed. This is not behavioral RED evidence.
- `green-focused` (101): initial draft compile error, unclosed enum delimiter.
- `green-focused-2` (101): initial draft compile errors for missing `Clone` and a partial move.
- `green-focused-3` (0): initial 6-test draft passed (corrected after exact log review).
- `green-focused-4` (101): meaningful behavioral RED; 8 assertions passed, and the extreme numeric conversion test failed because a large JSON number cast to non-finite `f32` then serialized as `null`.
- `green-focused-5` (0): numeric representability check fixed that failure; 9 tests passed.
- `focused-final` (0): 9 tests passed after direct formatting.
- `focused-current` (101): test compilation error from using `str::replace` with a count argument.
- `focused-current-2` (0): expanded suite passed.
- `focused-committed` (0): committed source-bound run passed.

The early runner versions captured child stdout/return code and tracked diff/status hashes, but did not hash untracked source contents. Those attempts are retained as historical drafts; only `focused-committed` is source-bound final evidence. The runner evolved in place before the final run, so exact early runner source versions were not preserved; this is a provenance limitation. The final runner source is preserved as `run-focused-committed.py` beside the logs.

Formatting: `rustfmt --edition 2021` was applied only to the new Rust module and integration test. A broad `cargo fmt -- ...` invocation initially changed six unrelated engine-api Rust files; they were restored exactly, and no unrelated file is in the commit.

## First review checkpoint (historical)

At the initial candidate commit, the full/adjacent engine-api tests, crate Clippy and final acceptance review were pending. The review and gates were completed afterward; see the later sections below.

## Review round 1 and scoped fixes

Astra's exact-diff review is recorded in `/Users/rutmehta/Developer/tessera/.superpowers/sdd/2026-09-28-pinned-raw-descriptor/task-1-review.md`. It requested discriminating tests and small maintainability fixes. The fixes are committed as `12b354fd35effa5972d5743fbe353aee01abfb6e` after `771a508f`:

- Current nondefault exposure now differs from a valid stale history state; the test checks retained bytes, expected `RecipeHash`, and changed input identity. Opaque malformed history remains a separate test.
- Recursive duplicate controls use recognized nested settings keys, escaped decoded-equivalent nested names, and duplicate descriptor members. Valid duplicate-free payloads are also accepted.
- Mask component arrays exercise flattened extra members and nested ModelRef members; a valid typed component is the positive enum control. Camera and lens profile bare-string legacy forms and canonical forms pass; unknown profile members fail. Positive/negative exposure and nested array f32 overflow cases are covered.
- Header coverage now includes settings object/empty object behavior, missing/null owner, missing process object/family/revision, unsupported Adobe and Native revisions, schema type/version cases, and declared length/canonical suffix identity controls.
- Removed `clone()` on the Copy route, folded numeric loss detection into the recursive shape check, pinned the temporary Recipe hash to explicit Native revision 2, and moved invariant 19 into the invariant section.
- Per root's ruling, the additive API change bumps `CONTRACT_VERSION` to 1.7.0; the exact assertion in `tests/m532_channels.rs` was updated. The commit touches those five paths only.

A precommit expanded focused attempt `review-fix-focused` exited 101 after 19/20 passed because the test changed the already-valid enum tag `sky` to itself while expecting an error. This was a bad negative fixture, not a source failure; it was corrected to `future_kind`. The passing, committed command is again `cargo test -p engine-api --test pinned_raw --release --jobs 2`, exit 0, 20 passed/0 failed. Raw log/return code: `/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor/review-fix-committed.log` and `review-fix-committed-returncode.txt`. Before/after source freezes: `review-fix-committed-before.json` and `review-fix-committed-after.json`; five assigned-file hashes and full tracked-tree hash match at clean HEAD `12b354fd35effa5972d5743fbe353aee01abfb6e`. Runner metadata includes exact command, CWD, target directory and rustc/cargo versions in `review-fix-committed-metadata.json`. The runner used is preserved as `/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor/run.py`, which rejects reused labels.

The exact-diff reviewer has been asked to re-review this scoped commit. Full/adjacent engine-api tests, formatting/strict Clippy and final acceptance remain pending that review.

## Final gates and current head

Astra's scoped re-review cleared the fixes in `12b354fd`; root independently verified the five changed-file blobs and clean tracked tree. The subsequent strict Clippy findings were test helper style only. Two small commits corrected them: `d3a9320d7f2347a4797071a986b8a3eda48eb1ae` and `083018a967d2ba1b07570824e885baca1e9d4314`. Final branch head is `083018a967d2ba1b07570824e885baca1e9d4314`, clean. The latter commits only adjust the mask fixture initializer after the first attempted textual replacement did not match the formatted source.

On the final head, all three scoped gates pass with per-command direct child return codes and matching source snapshots:

- `cargo test -p engine-api --release --jobs 2`: exit 0. Totals: 63 unit tests, 35 integration tests, 0 doc tests; 98 passed, 0 failed. Log: `/Volumes/betterSSD/tessera-validation/pinned-raw-descriptor/final-engine-tests-rerun.log`; metadata and source snapshots use the `final-engine-tests-rerun-*` prefix.
- `cargo clippy -p engine-api --all-targets --jobs 2 -- -D warnings`: exit 0. Log/metadata/freezes use `final-clippy-rerun2-*`.
- `rustfmt --check --edition 2021 --config skip_children=true crates/engine-api/src/lib.rs crates/engine-api/src/pinned_raw.rs crates/engine-api/tests/pinned_raw.rs crates/engine-api/tests/m532_channels.rs`: exit 0. Log/metadata/freezes use `final-rustfmt-check-rerun-*`.

Preserved failed gate attempts: `final-fmt` (Cargo fmt rejected unsupported `skip-children`; no files were changed), `final-clippy` (identified two test-only lints), and `final-clippy-rerun` (one test fixture initializer remained after an attempted replacement did not match). Each has a raw log, return code, metadata and before/after snapshots. The final direct rustfmt command supersedes the failed Cargo-fmt option; final Clippy and full crate test runs supersede the lint failure attempts.

Final test coverage includes exact recipe-byte serialization and metadata, opaque history, nondefault current settings and RecipeHash, identity dimensions and locator exclusion, duplicate decoded JSON keys at recipe/settings/descriptor depth, recursive unknown keys and flattened mask/ModelRef contents, legacy/canonical profile forms, explicit headers and schema versions, unsupported process/route cases, geometry, byte length, suffix normalization, numeric overflow, and distinct recipe owners. No app, GPU, RAW decoder, filesystem capture or workspace build was run.
