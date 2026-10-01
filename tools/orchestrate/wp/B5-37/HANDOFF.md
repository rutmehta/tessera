# B5-37 — catalog-scoped Lightroom indexing

## Scope and root cause

`LightroomImportController.startImport` calls the UniFFI `LrcatImport.apply` entry point. `TesseraCore/LightroomImport.swift` supplies mapping/report models and renders the returned indexed count; it does not index files. The Rust importer formerly reduced resolved catalog originals to containing folders and called the recursive index scan. Any unrelated supported image below those folders entered the index and inflated the report.

The app's import-completion callback also called normal Open Folder, causing another recursive scan (including the same-folder rescan shortcut).

## Changes

- Index only deduplicated canonical catalog file paths after root relocation; never scan the mapped roots or containing folders. Virtual copies and missing originals do not create scan work. Existing accessible originals whose edits are skipped remain eligible for indexing.
- Reuse `Index::scan_file`; mark interrupted indexing cancelled so resume can finish it.
- Load the existing index after import using `EngineLibrary.openIndexed`, bypassing both normal folder scanning and the same-folder rescan shortcut. Ordinary Open Folder retains its scan behavior.
- Explicit single-file scanning accepts generic `.raw` originals. Folder discovery's extension filter is unchanged. The catalog-copy check exposed this pre-existing extension mismatch: eight accessible originals initially produced only seven index entries.

## Regression evidence

- Synthetic catalog generator: five accessible catalog originals, one missing original, one virtual copy, plus three unrelated images in the same mapped root (sibling, nested descendant, and another root child).
- Before fix: import indexed seven entries instead of five; regression failed.
- After fix: five indexed entries, no unrelated images; repeat import updates zero; normal Open Folder finds all eight images.
- Swift bridge regression: load one indexed original, add an unrelated original, open the existing index and retain one; normal Open Folder then finds two.
- Explicit `.raw` regression: recursive scan remains zero; explicit file scan changes from zero before the fix to one after it.

## Catalog-copy qualification (counts only)

The provided read-only catalog COPY was used through a command-line Rust test calling the FFI engine/import API. A separate read-only SQL query enumerated referenced originals. The eight accessible originals were copied into disposable mapped roots; isolated support/library storage kept all writes away from personal originals. The source catalog copy was compared byte-for-byte before and after. No GUI or screen capture was used.

- Catalog records: **21,656**.
- Accessible originals: **8**.
- Imported: **8**.
- Indexed/report count: **8**; independently queried index rows: **8**.
- Missing originals: **21,648**.
- `apply` duration: **4.691 seconds**.
- Supplied observed baseline: **199 seconds**, **14,802** indexed.
- Observed reduction against that baseline: **194.309 seconds (97.64%)**; **14,794** unintended index entries avoided.
- Full verification duration, including catalog opening/parsing and setup: **206.76 seconds**. The apply timing is not an end-to-end UI timing. This is a staged-original comparison to the supplied baseline, not a controlled same-machine A/B benchmark; the shared machine was under substantial swap pressure.

The opt-in test is `catalog_copy_indexes_eight_accessible_references`, requiring `TESSERA_LRCAT_COPY`; it asserts eight accessible originals and refuses catalog inputs outside temporary storage. Catalog/photo names and paths are intentionally omitted here.

## Gates

Commands used the requested external `CARGO_TARGET_DIR` and Cargo path. Build commands ran serially.

- `cargo test --locked --release -p index`: **PASS**, 56 passed, 2 ignored.
- `cargo test --locked --release -p tessera-ffi`: first run failed the existing GPU export/slider L2 assertion in `export_batch_does_not_starve_slider_drag` under load.
- `cargo test --locked --release -p tessera-ffi -- --test-threads=1`: **PASS**, 536 passed, 29 ignored. The opt-in catalog-copy qualification was run separately and passed.
- `cargo clippy --locked --all-targets -p index -- -D warnings`: **PASS**.
- `cargo clippy --locked --all-targets -p tessera-ffi -- -D warnings`: **PASS**.
- `cargo fmt --all -- --check`: **PASS**.
- `cd apps/mac && ./build-ffi.sh`: **PASS**, bindings regenerated, no generated-file diff.
- `tools/orchestrate/swift-gate.sh`: **NOT GREEN**. Swift build succeeded. The standard run recorded a failure in `MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth`, then stalled after shell layout with the dispatch thread soft limit exhausted by AppKit animation workers. Its owned XCTest process was terminated after sampling confirmed the stall.
- An unchanged gate-script retry injected `--parallel --num-workers 1` into its `swift test` invocation via an exported Bash function: all 862 discovered XCTest cases ran serially with process isolation, without filters. It exited 1 and printed `SWIFT GATE FAILED`, listing the mask layout test plus `DocumentDitherCheckboxTests.testRefreshDoesNotPublishAndUsesLatestCallbackThenDismantles` and `DocumentInspectorActionButtonTests.testReuseRefreshesActionAndDisabledOrDismantledCannotInvoke`. The two native control tests passed in the standard run; their isolated methods do not initialize `NSApplication` before invoking native accessibility actions.
- The mask layout test unconditionally invokes the existing window `screencapture` helper, then fails because `CGImageSourceCreateWithURL` returns nil. Further capture retries were not attempted under the package's no-screen-capture constraint. No test was weakened or filtered out to manufacture a green gate.
- Direct changed-path check: `swift test -c release -Xswiftc -enable-testing --filter IncrementalLibraryTests/testImportedLibraryOpensWithoutDiscoveringUnrelatedPhotos`: **PASS**, exit 0, 1 XCTest passed in 0.060 seconds; verified its named pass, not only the `(0 unexpected)` text.

**Required `SWIFT GATE OK` was not obtained. This package is not fully gate-qualified for integration.** The source changes are locally committed for review; the existing screen-dependent Swift gate and animation/test-isolation issues remain for the coordinator.

## Commits

- `91fed8de` — `test(B5-37): restrict catalog indexing and preserve folder discovery` (confirmed failing regression).
- `db172a98` — `fix(B5-37): index only resolved catalog originals after import`.
- This handoff is the following `docs(B5-37):` commit.

## Boundaries and remaining limitations

Only this worktree was modified. No board or lockfile changes; no manual GUI launch; commits remain local. The required existing Swift gate attempted its window-capture test as documented above; the CLI catalog qualification used no GUI or capture. Existing unrelated index entries from a previous broad import are not deleted. Catalog parsing performance is unchanged. Loading the imported index does not narrow an already-populated shared index to only this import's membership; this package prevents new unrelated discovery.
