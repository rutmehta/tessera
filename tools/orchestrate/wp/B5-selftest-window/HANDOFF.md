# B5-selftest-window: background document self-tests

## Problem
`--nonactivating` (M2-53) sets the accessory activation policy, and SwiftUI then never creates the `Window("Tessera")`
scene. No document view appears, so every self-test that starts from the document view, a panel, a sheet or a menu
never ran and printed nothing. B5-12b had a transform-only fix (its own window) in `TransformSelfTest.swift`.

## Mechanism (`apps/mac/Sources/Tessera/App/SelfTestHost.swift`)
- `AppDelegate.applicationDidFinishLaunching` calls `SelfTestHost.launch(model:)`. It does nothing unless
  `--nonactivating` is given **and** a document self-test is requested (any of `--document-selftest`,
  `--tools-selftest`, `--filter-selftest`, `--styles-selftest`, `--retouch-selftest`, `--vector-selftest`,
  `--transform-selftest`, `--liquify-selftest`, `--channel-paint-selftest`, `--camera-raw-selftest`, in `--flag <dir>`
  or `--flag=<dir>` form, or env `TESSERA_CHANNELS_SELFTEST` / `TESSERA_TEXT_SELFTEST` / `TESSERA_STACK_SELFTEST`).
- 0.3 s later, if there is no visible regular window, it hosts `ContentView.root` in a regular `NSWindow` subclass with
  `canBecomeKey`/`canBecomeMain` false, ordered `.below` everything (never `orderFront`, `makeKey` or `NSApp.activate`).
  It uses a window, not a panel: AppKit doesn't count panels, so closing an alert would quit the app.
  Prints `selftest-host: host window <id>`.
- It then starts every requested self-test directly (each `startIfRequested` is idempotent), so `--new-document` is no
  longer needed. Channels and text wait for an open document: if neither `--new-document` nor `--open-document` is
  given, the host opens a blank document for them. Stack is started through `DocumentStack.shared.attach(model)`.
- `SelfTestHost.raiseForCapture(_:)` replaces the self-tests' own `orderFrontRegardless()` calls (document, filter,
  tools, styles, channels). In the background it does nothing; foreground runs behave as before.
- Early exits now count as failures. In every self-test, a `log("FAIL …")` line increments `failures`, and silent
  guards (no pixel layer, no reopened viewport) log FAIL. A run that could not happen ends in `done, N failure(s)` with
  N ≥ 1 instead of a silent 0. Channels and text "no document" now print `FAIL no document`.
- Removed the transform-local `AuditWindow`. `TransformSelfTest` calls `SelfTestHost.ensureWindow` because it can
  start from the menu build before the 0.3 s launch hook.
- `FilterSelfTest` and `StylesSelfTest` got `started` guards, since they can now be started twice.
- `BackgroundAuditWindow` (panel) is still used by `--timing-selftest` only. It is not a document self-test and was
  left unchanged.

## Launch recipe (background, never focus-stealing)
```
open -g -n --stderr <log> [--env TESSERA_<CHANNELS|TEXT|STACK>_SELFTEST=<dir>] apps/mac/build/Tessera.app \
  --args --nonactivating --app-dir <scratch>/app --folder <library-folder> <self-test flag>
```
- Always pass `--nonactivating`. `--app-dir` keeps the user's defaults untouched.
- Tests that use Edit in Layers (`camera-raw`, `retouch`, `filter`, `tools`, `document`) need `sample.dng` in
  `--folder`. `liquify` needs `--open-document <image>`.
- Screenshots: tests write `<dir>/<name>.req` (window id) or print `step <n>-<name> window-id <id>` and wait for
  `<dir>/ack-<nn>`. Capture with `screencapture -x -o -l <id>`, which works behind other windows.
- Wrapper: `tools/orchestrate/wp/B5-selftest-window/run-background-selftest.sh <name> [extra args]` does all of the
  above: scratch dirs, fixture copy, req/ack handling, `lsappinfo front` before/after, and killing its own PID.
  It exits 0 only for `done, 0 failure(s)` with the frontmost app unchanged.
  Names: transform vector channel-paint camera-raw retouch filter styles tools document liquify channels text stack.

## Verification (Machine B, 2026-09-29)
- `apps/mac/build-ffi.sh` OK; `Support/make-app.sh debug` built and verified.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (778 tests, 3 skipped, 0 failures; includes new `SelfTestHostTests`).
- Background runs via the wrapper (frontmost app `Arc` before and after every run, no Tessera process left):
  | self-test | result |
  |---|---|
  | transform | done, 0 failure(s) (61 ok checks) |
  | channels (env) | done, 0 failure(s) (19 ok), no `--new-document` |
  | camera-raw | done, 0 failure(s) (9 ok) |
  | channel-paint | done, 0 failure(s) (22 ok), no `--new-document` |
  | vector | done, **1 failure(s)** (88 ok), no `--new-document` |
- Baseline on main dd2cf300: the same vector launch printed nothing in 20 s.
- The vector failure is a test result, not a harness problem: `check fill-only drag previews coalesce to frames FAIL 1`
  (step 372; needs ≥ 5 latency samples and got 1). The likely cause is that an occluded background window gets
  almost no presented frames during the 60 Hz synthetic drag. It needs a decision: accept it for background runs,
  or measure it differently.
