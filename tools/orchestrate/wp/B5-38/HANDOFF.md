# B5-38 — Lightroom-owned originals stay read-only

Branch: `wp/B5-38`; base: `1a6f01e5`.

Local commits, in tests-first order:

- `188f9ce6` — `test(B5-38): protect Lightroom-owned photo and sidecar destinations`
- `aab7d1cb` — `fix(B5-38): keep Lightroom-owned sources read-only`
- The following `docs(B5-38)` commit contains this handoff.

## Result and root cause

Sidecar paths were derived from the original's parent without checking Lightroom ownership. Import therefore wrote adjacent XMP and `.edits` recipes inside Lightroom bundles. Several restore, sync, and export paths also published raw bytes without passing through the atomic sidecar writer.

The shared policy now lives in `sidecar::Sidecar`. It recognizes case-insensitive `.lrdata`, `.lrcat`, `.lrcat-data`, Lightroom Catalog Previews, and Smart Previews path components, plus directories containing a `.lrcat` entry. Checks cover lexical paths and canonical existing prefixes, including symlinks and destinations whose parent does not yet exist. Publication is refused before creating directories or temporary files inside a protected tree.

Protected originals use the existing recipe/XMP formats in `.edits/lightroom/<hash-prefix>/<canonical-path-hash>.json` and `.xmp`, located outside the outermost protected root. Normal originals retain their existing sidecar paths. Full-path keys prevent same-name collisions and remain stable when the original goes offline. This is the existing `.edits` mechanism with a protected-source namespace, not a new database or recipe format. The destination guard also validates the private store itself.

The policy covers ordinary sidecar writes, cull persistence and rollback, People history restoration, raw smart-preview sync, import publication, and rendered/original export destinations. The index stamps the private recipe and XMP paths so later edits invalidate cached metadata. Import emits a counted `Read-only originals` report issue explaining why no adjacent sidecar was written and where metadata is stored. Generated Swift binding changes are documentation only; no ABI changes.

Catalog-directory scans are cached using directory modification/identity metadata, with a bounded cache. A regression verifies that adding a catalog revokes earlier write permission. This avoids repeatedly scanning shared ancestors on every recipe access: the local six-test cull session suite dropped from 106.32 seconds with the uncached implementation to 1.66 seconds with caching. These are diagnostic timings, not a controlled benchmark.

## Tests-first evidence

The test commit adds 11 regression tests across six files. Before the fix, focused runs failed for direct publication, protected import, cull rollback, People history, export destinations, and offline private-store identity. Assertions were retained through the fix.

Coverage includes:

- Five fixture originals under `X.lrdata` and five under `Foo.lrcat-data`: import and resume preserve all protected directory entries and bytes, create no adjacent sidecars, and report five read-only originals per fixture.
- Imported exposure/grade survive in the private store; a subsequent recipe edit persists and changes the index recipe hash without changing the protected tree.
- Normal-folder imports continue writing adjacent XMP and recipe files.
- Direct writes, symlink aliases, catalog-containing directories, missing destination parents, read-only library destinations, rollback/history/sync, original export, and rendered export are guarded.
- Adding a catalog invalidates cached ownership, and taking an original offline preserves its private-store identity.

No real catalog or supplied catalog copy was opened. All import counts above are synthetic fixture counts.

## Verification

Builds were run serially with:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-38
export CARGO_BUILD_JOBS=1
```

Release test totals, including integration tests:

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| sidecar | 60 | 0 | 0 |
| cull | 50 | 0 | 0 |
| index | 55 | 0 | 2 |
| export | 100 | 0 | 7 |
| tessera-ffi | 539 | 0 | 28 |
| Total | 804 | 0 | 37 |

Commands completed successfully:

```sh
cargo test --locked --release -p export
RUST_TEST_THREADS=1 cargo test --locked --release -p sidecar -p cull -p index -p tessera-ffi
cargo clippy --locked --all-targets -p sidecar -p cull -p index -p export -p tessera-ffi -- -D warnings
cargo fmt --all -- --check
(cd apps/mac && ./build-ffi.sh)
```

An earlier parallel test-runtime run failed the existing `develop::export_batch_does_not_starve_slider_drag` adaptive-L2 timing threshold during other machine activity. The full final run with one test thread passed, with assertions unchanged. Rust logs: `/tmp/B5-38-export-tests.log`, `/tmp/B5-38-release-final.log`, `/tmp/B5-38-release-concurrent-failure.log`, `/tmp/B5-38-clippy.log`, and `/tmp/B5-38-fmt.log`. FFI build log: `/tmp/B5-38-build-ffi.log`; the resulting archive is arm64.

### Swift gate: FAILED; required SWIFT GATE OK not obtained

The normal `tools/orchestrate/swift-gate.sh` run built successfully but stalled after its four ShellLayoutTests passed. A process sample showed the dispatch soft limit of 80 threads reached, with workers blocked in AppKit `NSAnimation` and the XCTest main thread awaiting async completion. Only this worktree's stalled test process was terminated. Evidence: `/tmp/B5-38-swift-normal.log`, `/tmp/B5-38-swift-gate-normal.log`, `/tmp/B5-38-xctest.sample`.

The repository's existing `CI=1` mode skips some local window-server tests. A complete `CI=1 tools/orchestrate/swift-gate.sh` rerun exited 1 and printed:

```text
Build complete! (20.37s)
Executed 861 tests, with 9 tests skipped and 1 failure (0 unexpected) in 185.127 (185.227) seconds
Test run with 5 tests in 2 suites passed after 0.024 seconds.
SWIFT GATE FAILED (exit 1):
MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth
```

That existing test calls the window-capture harness even in CI mode. Its failure was `could not create image from window`, followed by an invalid image file and `XCTUnwrap` of `CGImageSourceRef` at `MasksPanelLayoutTests.swift:88`. The gate was not rerun after identifying its capture dependency, given the instruction prohibiting screen capture. No test assertions, UI product sources, or layout harness code were changed to obtain a green result. Evidence: `/tmp/B5-38-swift-gate.log` and the saved partial detailed log `/tmp/B5-38-swift-ci-details.log` containing the failure.

This package therefore has passing Rust checks and FFI generation, but does **not** satisfy the mandatory Swift gate. Integration requires resolving the independent layout-harness constraints and running the gate to literal `SWIFT GATE OK`.

## Scope and remaining boundaries

- No GUI app was launched or screen capture directly requested by the agent. The required test suite itself invoked its existing window-capture helper; that dependency is disclosed above.
- No changes to `Cargo.lock` or `board.json`; commits are local only.
- Existing adjacent sidecars already inside protected trees are not removed or migrated: those files remain untouched.
- Private store keys follow canonical source paths; moving originals changes their keys. If the outside-tree destination is unwritable, publication fails rather than falling back inside the protected tree.
- The policy addresses the photo-sidecar and import/restore/sync/export paths described above; this handoff does not claim an audit of every unrelated application filesystem operation.
