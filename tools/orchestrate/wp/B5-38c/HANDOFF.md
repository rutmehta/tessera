# B5-38c — pre-merge review fixes

Branch `wp/B5-38`, commits on top of `37e82784`. No rebase, merge, or push. RED commit: `fea66274`. Fix commit: `40603f83`. The following `docs(B5-38c)` commit records this handoff.

This handoff supersedes B5-38b's content-change behavior, inferred library destination, and photo-publication indexing descriptions.

## Findings

- **F1 fixed.** A protected path's alias wins when its recipe exists. Path aliases now retain both the current `content_hash` and stable `recipe_key`; legacy string aliases remain readable. Rewriting the source bytes keeps the existing recipe/XMP destination. The current hash is associated with that object through `content/<hash>.json`, so a later unaliased renamed path can discover it. Lookup remains read-only: aliases update in memory during lookup and persist on the next successful metadata save. The rewrite regression checks edit values, stable object location, the saved current hash, and discovery after rename. A second regression covers a rewrite matching another previously edited original: both established paths retain their distinct recipes, including after a subprocess restart. New content aliases cannot override an established path's recipe reference.
- **F4 fixed.** The shared HDR/Panorama/Enhance publisher admits only the new DNG using `scan_file`. The subsequent shared recipe/XMP persistence refresh also uses `scan_file`; leaving that second call as a folder scan would still import siblings. The integration regression leaves previous exports plus an unrelated JPEG in the destination, checks that exactly the two inputs and one output are indexed, and verifies unchanged root count.
- **F5 fixed.** A protected inferred library destination falls back to `<Application Support>/Imported Libraries`, not the support directory itself. The existing synthetic protected-catalog regression checks this destination and successful import without custom options.
- **F2 deferred/reported.** Previously unaliased byte-identical protected originals can still discover and share one content-addressed recipe. Independent duplicate recipes need an explicit identity/duplicate policy. F1 preserves already-established distinct path recipes when a later rewrite makes their contents identical; that does not solve initial duplicate discovery. No new import-report warning is implemented.
- **F3 deferred.** Full-content hashing still occurs on a FileVersion cache miss. Unchanged versions reuse cached digests; the existing 8192-entry cache still clears wholesale when full. LRU/size accounting and bounded-hash alternatives are not implemented in this package.
- **F6 deferred.** Reads/writes before source-store registration can still use the default support store before a later explicit registration routes them elsewhere. Store ownership/migration requires a separate API/lifecycle change. `default_options` still uses the existing infallible signature and `expect` for `support_dir`; converting it to an error result and updating Swift callers is not included. `support_dir` derives a path from the engine DB path, rather than checking whether that directory currently exists.

## Index compatibility

B5-37's explicit-file API originally still inserted a root row. Index migration **010** makes `folder.root_id` nullable, preserving folder IDs and dependent file/image rows. An explicit-file scan can then admit an original without opting its parent into folder discovery. An explicit folder scan creates its root and adopts previously unrooted descendant folders. Tests cover an empty index, later folder adoption, a reconstructed version-9 schema with real dependent rows, and foreign-key integrity/enforcement after migration. The supported index schema advances from 9 to 10; old binaries reject this newer rebuildable index. Coordinate migration numbering at integration; this branch has not been rebased under review.

## Tests-first evidence

RED commit: `fea66274` — `test(B5-38c): reproduce rewritten protected edits and export folder admission`.

- `/tmp/B5-38c-red-sidecar.log`: rewritten bytes select a missing recipe.
- `/tmp/B5-38c-red-index.log`: both explicit-file root regressions fail on excess root count.
- `/tmp/B5-38c-red-merge.log`: six indexed images instead of three. The first attempt had a test-only moved-Arc compilation error; it was corrected and rerun before the RED commit.
- `/tmp/B5-38c-red-default.log`: support root instead of `support/Imported Libraries`.
- Additional collision regression, run before its corrective alias-format change: `/tmp/B5-38c-red-collision.log` showed one established path receiving the other's exposure. Its GREEN result and subprocess restart are in `/tmp/B5-38c-green-sidecar-final.log`. This additional regression is included with the fix commit.

Focused GREEN checks passed for storage, both index regressions, protected merge, and inferred import destination. The offline subprocess regression also checks legacy string-alias compatibility in the full run.

## Gates

Passed, exit 0:

```sh
cargo test --locked --release -p sidecar -p export -p index -p tessera-ffi -p import-lrcat -p tessera-cli -p cull --no-fail-fast
cargo clippy --locked --all-targets -p sidecar -p index -p tessera-ffi -- -D warnings
cargo fmt --all -- --check
```

The release gate includes the complete FFI suite, importer scale and FFI streaming-memory tests, CLI publication guards, and every new regression. Aggregate results: 951 top-level tests passed, zero failed, 38 ignored; the two nested sidecar subprocess checks also passed. Existing ignored tests were not changed. Logs: `/tmp/B5-38c-release.log`, `/tmp/B5-38c-clippy.log`, `/tmp/B5-38c-fmt.log`; matching `.status` files record exit codes.

`(cd apps/mac && ./build-ffi.sh)` passed (exit 0) and generated an arm64 archive. Regenerated Swift/C bindings have no source diff. Log: `/tmp/B5-38c-build-ffi.log`.

**Swift gate PASSED, exit 0, literal `SWIFT GATE OK`.** Swift build passed (`Build complete! (28.64s)`). Ran the unmodified gate as `CI=1 bash -x tools/orchestrate/swift-gate.sh`, matching the prior handoff's existing CI mode because normal mode was documented to stall in AppKit. This is not a claim that the default non-CI gate passed; no test or script was modified and no new skips were added.

```text
Executed 889 tests, with 9 tests skipped and 0 failures (0 unexpected) in 147.304 (147.380) seconds
Test run with 5 tests in 2 suites passed after 0.023 seconds.
SWIFT GATE OK
```

`MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth` passed in this run (0.925 seconds); the prior window-capture failure did not recur. Logs: `/tmp/B5-38c-swift-gate.log` and the complete preserved `/tmp/B5-38c-swift-details.log`. A read-only log follower kept the test log before the gate removed its temporary file.

All build commands for this task ran serially, using `PATH="$HOME/.cargo/bin:$PATH"`, `CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-38"`, `CARGO_BUILD_JOBS=1`, `MACOSX_DEPLOYMENT_TARGET=15.0`, `RUST_TEST_THREADS=1`, and `TESSERA_APP_DIR=/tmp/B5-38c-app-store`. Fixtures use temporary support directories or the isolated standalone store. No personal catalogs are read.

## Boundaries

No changes to `board.json` or `Cargo.lock`. No GUI app launched, no `~/Pictures` catalogs opened, no push/rebase/merge. Any AppKit/window capture comes only from the requested Swift test harness. All commits are local and retain the requested co-author trailer.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
