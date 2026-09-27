# B5-09b implementation status: retouch model downloads, non-blocking cancel, Content-Aware Fill enablement

Follow-ups to Sol's (Machine A) on-screen pass of B5-09. Verified on Machine B in the background only: the app was
launched with `open -g -n`, never activated or raised, driven by the extended `--retouch-selftest`, and only its own
windows were captured (`screencapture -x -o -l <window number>`). No real model weights were downloaded.

## Findings, root causes and fixes

### 1. LaMa "not installed" in a fresh app dir; no way to fetch LaMa / DDColor / DRUNet

**Root cause.** B5-09 deliberately had no acquisition path for retouch models: the Remove bar and the Neural Filters
sheet only said "not installed" and named the file. In a fresh `--app-dir` the cache `<app dir>/models/cache` is empty,
and weights in the engine's test cache are not visible to the app. Separately, retouch.rs's lookup could look elsewhere
than where downloads go: when `TESSERA_RETOUCH_MODEL_CACHE` was set it *replaced* the app cache, so a model
downloaded by the app would not have been found.

**Fix.**
- Engine (`crates/tessera-ffi/src/document/retouch.rs`): the app cache `<support>/models/cache` (where
  `ModelDownloads` writes) is always searched; `TESSERA_RETOUCH_MODEL_CACHE` is now only an extra, earlier location.
  Missing models are reported at the app-cache path. `RetouchModel` gained `version` (the pinned registry version the
  download is requested with), so the app does not hardcode versions. The missing-model message now says the app asks
  first per Settings ▸ AI ▸ Allow model downloads.
- App: `RetouchModelDownloads` (TesseraCore) routes LaMa, DDColor and DRUNet through `ModelAcquisition.shared`, the same
  `ModelDownloads` open/request flow and Settings ▸ AI preference M2-51 uses for AI Denoise. Nothing downloads unless the
  user asks for an operation (or clicks Download LaMa); with downloads off nothing is requested, the UI says so and
  links to Settings ▸ AI (`openSettings`). The operation that asked (a Remove stroke, Remove Selection, Remove
  Distractions with LaMa, Neural Filters Apply) is remembered and runs by itself when the download completes; a failure
  drops it and shows the reason with Retry. Inline progress reuses M2-51's `ModelProgressRow` in the Remove options bar
  and in the sheet (Apply becomes "Download and Apply"). If a download reports ready but the engine still does not find
  the file, the UI says so instead of showing "installed".

### 2. Cancel stayed on "Cancelling…" for over two minutes

**Root cause.** `DocumentRetouch.cancel()` only set the engine's cancel flags and printed "Cancelling…"; `busy` stayed set
until the blocking engine call returned. PatchMatch notices the flag very late (measured here: **343 s** after Cancel on
80 % of `sample.dng`; PatchMatch granularity is Machine A's M5-33).

**Fix.** `RetouchJobs` (TesseraCore) runs one apply at a time. Cancel marks the job abandoned, calls the engine's
cancel on a background queue and returns immediately (measured 0.1 ms); the options bar is idle at once and shows
`Stopping the cancelled Remove… N s`. The late result comes back as `.discarded` and never reaches History; if the
engine had already committed it before it saw the flag, that step is undone. A new job (stroke, Remove Selection,
Content-Aware Fill, distractions, neural apply) is refused with `The cancelled Remove is still stopping in the engine
(N s); try again when it has stopped` until the abandoned call has returned (refused, not queued, because the engine's
cancel flags are per session and a second apply would re-arm them under the first). The Neural Filters sheet uses the
same jobs (Cancel while applying returns the sheet to idle).

### 3. Edit ▸ Content-Aware Fill could not be invoked with an active marquee

**Reproduction.** In a fresh sequence (Marquee tool drag, then the Edit menu) the item enables correctly once the menu is
updated (self-test: `with a marquee …: enabled`, then invoked through `NSMenu.performActionForItem`, one
`Content-Aware Fill` row). The only path that keeps it disabled with a marquee is `retouch.busy != nil`: the item is
disabled while an apply runs (self-test check `Content-Aware Fill disabled while a Remove runs ok`), and the old Cancel
kept `busy` set until the engine returned, i.e. for minutes after a large Cancel (finding 2). So this was a consequence
of the blocking cancel.

**Fix.** Enablement is `RetouchMenuState.contentAwareFillEnabled(layerKind:hasSelection:jobRunning:)`: a pixel layer or
smart object, a selection, and no job *running*; an abandoned job no longer disables it (invoking it then gets the
"still stopping" message). Self-test check `Content-Aware Fill enabled again right after Cancel ok`. No AppCommands hook
was needed. Note: SwiftUI leaves the NSMenuItem's state stale until the menu is updated (logged `as SwiftUI left it
(before the menu is opened): disabled`); opening the menu updates it, so this is not a user-visible bug, but a
background check must call `menuNeedsUpdate` / `update()` first, as the self-test does.

## Self-test changes (`--retouch-selftest`)

Background-safe: no `NSApp.activate`, `makeKeyAndOrderFront`, floating level or `orderFrontRegardless`; step lines name
the window (and sheet) number. A background app never builds its menu bar, so the test also starts from the document
view (`RetouchSheets`); launch with `--new-document` (the blank document is closed first). New steps: Content-Aware Fill
invoked from the real Edit menu item (disabled without selection, enabled with a tool-drawn marquee); Cancel returns at
once, refusal while stopping, late result discarded; model downloads through a local stand-in `ModelDownloadRequesting`
(queued, 10 progress events, ready; writes nothing) with a scratch UserDefaults suite for the Settings toggle: downloads
off (nothing requested, reason shown, stroke dropped), allowed (stroke waits, progress, Remove runs by itself and gets
the engine's missing-model error because nothing was written), Neural Filters Download and Apply.

Evidence (`evidence/`): `retouch-selftest.log` (`done, 0 failure(s)`, 25 checks ok) and window-only captures,
downscaled: `05-content-aware-fill`, `06-slow-job-running`, `07-slow-job-cancelled` (idle bar + "Stopping the cancelled
Remove…"), `08-slow-job-refused`, `10-neural-colorize-sheet` (Download and Apply), `14-download-off`,
`15-download-progress`, `16-download-then-remove`, `17-download-neural-progress-sheet`, `18-download-neural-applied-sheet`.
The captures of the unfocused window show the options bar overlapping the toolbar and the status bar clipped; the same
happens at step 1 before any B5-09b UI is shown (the document window's content is taller than 900 pt since the Channels
panel was added, and SwiftUI clips it top and bottom). Not from this package, outside its paths.

## Tests and gate (worktree root, `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-09b`)

- `cargo test --locked --release -p tessera-ffi`: exit 0, 52 result lines, 284 passed, 0 failed. New in
  `tests/document_retouch_ui.rs` (`test result: ok. 9 passed; 0 failed`):
  `retouch_models_are_looked_up_where_model_downloads_put_them` (same version and path as the downloader's registry
  over `<support>/models/{models.toml,cache}`; a file placed there is the one verified: a corrupt one is a SHA-256
  mismatch for both the downloader and Remove with LaMa, History unchanged). Lib: `test result: ok. 51 passed`.
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean. `cargo fmt --all -- --check`: clean.
- `(cd apps/mac && ./build-ffi.sh && swift build --jobs 2 && swift test --jobs 2)`: `Executed 280 tests, with 0 failures
  (0 unexpected)` and `Test run with 5 tests in 2 suites passed`. `DocumentRetouchTests`: 17 tests (8 new: jobs cancel /
  abandon / refuse / discard with a fake slow engine; the Remove tool over a fake slow backend whose late commit is
  reverted; Content-Aware Fill enablement (pure and from a real document's marquee / deselect / running / cancelled
  job); downloads allowed → progress → run, off → nothing requested, failure → reason + retry, forgetting a waiting
  operation).
- `xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS' -derivedDataPath
  "$HOME/.cache/tessera-derived-data-B5-09b" -jobs 2 build`: `** BUILD SUCCEEDED **`.
- engine-api unchanged. Bindings regenerated (`RetouchModel.version`).

## ACCEPTANCE §Z

Steps 320, 323, 326, 328 and 333 updated for the new behaviour; new 334 (download LaMa then Remove), 335 (failure and
Retry), 336 (downloads off), 337 (Neural Filters Download and Apply), 338 (Cancel does not wait), 339 (Content-Aware
Fill enablement); verdict and identifier appendix extended.

## Left for Machine A (Sol)

- A real download: fresh `--app-dir`, Allow model downloads on, LaMa stroke (334) and Colorize / JPEG Artifact Removal
  Download and Apply (337) over the network, then relaunch to see them installed and Auto report `Remove: LaMa`. Here the
  downloader was a stand-in and nothing was written, so "the engine finds the downloaded file" is covered by the Rust
  path / integrity test, not by a real file.
- 335 (network failure and Retry) on a real network.
- PatchMatch cancellation granularity (M5-33): the engine still runs a cancelled large PatchMatch for about 340 s on
  `sample.dng`; the UI no longer waits for it, but a new removal is refused until it stops.
