# B5-38b — Machine A review rework

Branch: `wp/B5-38`. Added on top of `4d6da349`; the original three B5-38 commits were not rebased. Local commits only. This supersedes B5-38's protected-path and store-location descriptions.

- `f5f69b4e` — `test(B5-38b): cover precise guards and app-owned protected edits`
- `375623ff` — `fix(B5-38b): keep protected edits in app support and preflight publication`
- The following `docs(B5-38b)` commit contains this handoff.

## Per-item result

1. **Exact protected components.** Removed the entire ancestor-directory catalog scan and its cache. Protection is now case-insensitive, anchored to component suffixes `.lrcat`, `.lrdata`, and `.lrcat-data`; the two Lightroom previews bundle patterns are included by `.lrdata`. Substring names such as `Smart Previews for client` and `catalog.lrdata.backup` remain writable. A neighboring `.lrcat` does not protect the photo folder or alter its sidecar destinations. Lexical and canonical-prefix checks still cover symlinks and not-yet-created destinations. Deleted `adding_a_catalog_revokes_previously_writable_directory` and replaced it with the narrower regressions.
2. **App-owned store and stable identity.** Protected metadata uses the existing engine support directory supplied by `Engine::open`, the same root selected by `EngineLibrary.supportDirectory` (`--app-dir`, `TESSERA_APP_DIR`, or `~/Library/Application Support/Tessera`). Engine scanning/import and reopen register source roots (reopen uses cached canonical index paths without probing originals) so the shared sidecar/cull readers use that directory. Protected recipe and XMP objects are `<support>/.edits/lightroom/objects/<prefix>/<BLAKE3-of-file-content>.json` and `.xmp`. Canonical-path hashes are secondary aliases under `paths/`. Normal sources retain their existing adjacent sidecars. The tests rename `Photos` to `Photos 1`, reopen the engine, and resolve the imported exposure. Protected recipe envelopes adopt the current path-derived index ID on read; normal foreign-image checks remain intact.
3. **Merge/enhance publication.** `merge::publish_photo`, shared by HDR, panorama, and enhancement, preflights its folder before mkdir/temp creation and its final filename before publication. Protected inputs publish into the first destination from the last successful export workflow, or the existing UI default `~/Pictures/Tessera Export` when none is saved. This uses the existing `LastExport.json`, not a new output preference. Canonicalized output paths agree with the index even through symlinks or macOS `/var` aliases. The HDR regression asserts the actual output is in the chosen export folder and the protected source directory still contains only its two inputs.
4. **Destination preflight and XMP repair.** `catalog::xmp_path` no longer selects a protected adjacent Lightroom XMP. `catalog::write_paths` chooses and guards both destinations before recipe publication; `persist_with_packet`, Develop save, and Develop repair use it. Import also preflights both sidecars. Tests preserve adjacent XMP bytes, successfully publish recipe plus store XMP, and inject a post-recipe failure followed by a successful repair/close. The import/edit fixture also includes an invalid adjacent Lightroom packet that subsequent Tessera editing leaves untouched.
5. **Safe inferred library folder.** If the photos' common folder or catalog-parent fallback is protected, `default_options` chooses the engine's support directory. A synthetic catalog whose entire fixture is under `.lrdata` imports five photos with untouched default options. Explicit protected library destinations still fail before directory creation, with a library-folder-specific error.
6. **Export preflight and accurate reporting.** FFI batch export guards the destination before mkdir. Lower export publishers, library destinations, restore paths, and sidecars use distinct operation names in their errors. `Read-only originals` is added only after a successful per-photo write and includes up to five actual source paths. Resumed/skipped/failed candidates are not counted. Regressions cover pre-mkdir refusal, restore/library/export wording, examples, a zero-write resume, and a failed candidate excluded from the count and examples.

## Identity fallback and scope

Path lookup performs no filesystem writes. Saving a recipe or XMP persists its canonical-path alias in the app store. If the source is offline, lookup uses the remembered/persisted alias; a subprocess regression verifies restart behavior. If neither readable source content nor an alias is available, lookup falls back to a canonical-path key, which cannot follow a rename. A newly discovered renamed path is content-resolvable while online; saving metadata establishes its durable offline alias.

The selected identity is file content: byte-identical sources share an edit object, and changed bytes select a new object. Hashes are streamed and cached against size, modification time, and Unix file identity/change time; this avoids rehashing an unchanged RAW on every sidecar access. This package does not introduce a new recipe/database format or migrate the experimental B5-38 path-keyed files from user folders.

Policy checks now precede publication. Existing atomic-per-file, recipe-authoritative save/repair semantics remain for unrelated I/O failures; this is not a multi-file crash-atomic transaction. The regression deliberately exercises that existing repair path and verifies it no longer retries an adjacent protected XMP forever.

## Tests-first evidence

The RED commit is `f5f69b4e` (`test(B5-38b): cover precise guards and app-owned protected edits`). Production changes were applied only after the regressions ran against B5-38.

- `/tmp/B5-38b-red-sidecar.log`: catalog-neighbor/substrings and app-store/rename tests failed.
- `/tmp/B5-38b-red-save-export.log`: corrected Develop fixture failed with the adjacent-sidecar refusal; batch export failed because the protected destination directory had already been created.
- `/tmp/B5-38b-red-targeted.log`: merge output was still in the source folder; default import and remount regressions failed. Initial Develop/export fixture issues in this log were corrected and rerun in the preceding log.
- `/tmp/B5-38b-red-lrcat.log`: five import regressions failed, including remount resolution, unsafe default, report behavior, and library refusal wording.
- `/tmp/B5-38b-red-restore.log`: restore rejection lacked its operation-specific error.

The merge assertion compares canonical directories to account for macOS `/var` aliases; it still requires the chosen export folder and unchanged protected inputs. The first full run exposed two sidecar fixture documents that directly changed settings without advancing history. Those fixtures now use `Recipe::edit`; the rename/offline assertions were retained, and the entire sidecar crate was rerun successfully.

## Gates

All build commands were serial, using:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-38"
export CARGO_BUILD_JOBS=1
export MACOSX_DEPLOYMENT_TARGET=15.0
export TESSERA_APP_DIR=/tmp/B5-38b-app-store
```

`TESSERA_APP_DIR` isolates standalone sidecar fixtures; engine fixtures pass their own temporary support directories.

Release tests (all integration tests included; final results after the fixture correction):

| Crate | Passed | Failed | Ignored |
| --- | ---: | ---: | ---: |
| sidecar | 62 | 0 | 0 |
| export | 100 | 0 | 7 |
| index | 55 | 0 | 2 |
| tessera-ffi | 545 | 0 | 28 |
| import-lrcat | 41 | 0 | 0 |
| cull | 50 | 0 | 0 |
| Total | 853 | 0 | 37 |

```sh
RUST_TEST_THREADS=1 cargo test --locked --release -p sidecar -p export -p index -p tessera-ffi -p import-lrcat -p cull --no-fail-fast
RUST_TEST_THREADS=1 cargo test --locked --release -p sidecar
```

The combined command completed all targets and exited 101 solely for the two malformed sidecar fixtures described above. Its other five crates, including the full FFI suite, passed. The subsequent full sidecar run on the corrected fixtures exited 0. Logs: `/tmp/B5-38b-release-final.log` and `/tmp/B5-38b-sidecar-final.log`. The sidecar count excludes the nested subprocess's duplicate test-harness summary.

Passed, exit 0:

```sh
cargo clippy --locked --all-targets -p sidecar -p cull -p index -p export -p tessera-ffi -- -D warnings
cargo fmt --all -- --check
(cd apps/mac && ./build-ffi.sh)
```

Logs: `/tmp/B5-38b-clippy-final.log`, `/tmp/B5-38b-fmt.log`, and `/tmp/B5-38b-build-ffi.log`. FFI generation produced an arm64 archive. Regenerated Swift/C bindings have no source diff; there are no ABI changes. A clippy-only single-element test loop was simplified, followed by the final complete sidecar rerun reflected above.

### Swift gate: FAILED; required `SWIFT GATE OK` not obtained

Ran the original gate script with `CI=1` and shell tracing (no script/test modifications):

```sh
CI=1 bash -x tools/orchestrate/swift-gate.sh
```

The prior B5-38 handoff records the normal, non-CI mode stalling in AppKit/window-server tests on this machine. This package used the existing CI mode and did not repeat that normal-mode stall. Its skips are included below; this is not a claim that the default, non-CI gate passed. A read-only log follower preserved the temporary Swift test log before the script removed it.

```text
Build complete! (8.91s)
Executed 861 tests, with 9 tests skipped and 1 failure (0 unexpected) in 147.600 (147.670) seconds
Test run with 5 tests in 2 suites passed after 0.025 seconds.
SWIFT GATE FAILED (exit 1):
MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth
```

The failure was `could not create image from window`, followed by `XCTUnwrap failed: expected non-nil value of type "CGImageSourceRef"` at `apps/mac/Tests/TesseraCoreTests/MasksPanelLayoutTests.swift:88`. This is the same capture failure recorded for B5-38, consistent with the stated locked-screen limitation. No UI/test assertions were changed and no new skips were added.

Evidence: `/tmp/B5-38b-swift-gate.log` and the complete preserved `/tmp/B5-38b-swift-details.log`. Rust checks, FFI generation, and Swift build pass, but a successful required Swift gate remains unverified and needs a usable window-capture environment.

## Boundaries

- No real Lightroom catalog or catalog copy was opened; imports used generated synthetic fixture catalogs.
- No Tessera GUI app was launched. Any AppKit/window-capture behavior mentioned in the gate results comes from the required Swift test harness.
- No changes to `Cargo.lock` or `board.json`; no push, rebase onto main, or merge.
- Existing Lightroom originals and adjacent metadata are never removed/migrated by this package.

Co-Authored-By: Claude Fable 5.1 <noreply@anthropic.com>
