# B5-31 — Reuse an open image document

Branch: `wp/B5-31`. Local commits only; coordinator owns pushing/integration.

## Bug and entry point

Opening a library image in Layers a second time created another document tab. With
the image document and a different document already open, this produced a third
tab instead of selecting the first one.

In this checkout `AppModel.enterPhotoEdit()` enters the photo editor. The Layers
handoff is `createRequestedLayeredCopy()` → `DocumentWorkspace.editInLayers()`.
`DocumentTabs` renders `workspace.documents` and already selects tabs correctly.

## Cause and fix

`openDocumentFromImage` creates a fresh backend on each call. `install` only
deduplicates backend object identity, so repeated image opens bypassed its check.
`DocumentSummary.sourceImageId` already records the engine library image ID, or
`file:<path>` for stub image opens. `PhotoItem.id` is a transient library row index;
`DocumentSummary.path` is a save destination, not an image source for flat images.

`editInLayers` now looks for an existing source ID (or matching standardized saved
file path) before loading. A match selects the existing controller, respects
`activateDocument` and status-publication ownership, and completes once with
`.installed`. Unmatched items follow the existing load path. No tab-strip or Rust
changes were needed.

## Tests first

Test-only commit: `4eff9bfb` — `test(B5-31): reproduce duplicate Open in Layers tabs`.

`DocumentReuseTests` contains three hosted XCTest cases; none creates a window or
activates the app:

- Repeat a real stub image open after opening another document, with a changed
  library row index; require the original controller and two documents.
- Repeat with caller-owned status/navigation; require unchanged caller state and
  exactly one successful completion.
- Repeat a real engine-backed library image open; verify its recorded source ID,
  original controller identity, and unchanged document count.

Before the fix, `swift test -c release -Xswiftc -enable-testing --filter
DocumentReuseTests` exited 1: all three tests failed controller-identity and
document-count assertions (`3` instead of `2`), six assertion failures total.
The test-only commit preceded the implementation change.

All three regression cases passed during the first full gate run after the fix.

## Required gate

Command:

```sh
export PATH="$HOME/.cargo/bin:$PATH"; cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

The first run built successfully but failed an existing native modal probe:

```text
Build complete! (130.73s)
Executed 864 tests, with 3 tests skipped and 4 failures (0 unexpected) in 512.031 (512.274) seconds
✔ Test run with 5 tests in 2 suites passed after 0.026 seconds.
SWIFT GATE FAILED (exit 1):
DocumentSaveSheetProbeTests.testClosingCapturedParentDuringRealFolderChooserDrainsNativeSheet
```

That test's timer did not close the captured parent inside the real NSOpenPanel
modal loop. No changes were made to the probe or implementation between gate runs.

The exact-command retry exited 0 and printed:

```text
Build complete! (19.65s)
Executed 864 tests, with 3 tests skipped and 0 failures (0 unexpected) in 287.005 (287.119) seconds
✔ Test run with 5 tests in 2 suites passed after 0.025 seconds.
SWIFT GATE OK
```

All three `DocumentReuseTests` passed again. The previously failing native probe
also passed (1.897 seconds), confirming that failure was transient in these runs.
Gate success is based on the script's exit status and `SWIFT GATE OK`, not XCTest's
`(0 unexpected)` text. Verification completed on 2026-10-01.

Vendored LibRaw and existing Swift test compiler warnings appeared during builds.
No GUI app was manually launched and no screen capture was taken. Rust sources,
`Cargo.lock`, and `board.json` remain unchanged.
