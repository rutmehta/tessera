# Review preview timeout source audit (read-only)

Candidate checked: `workspace-redesign/codex/psd-current-ffi` at `729962d92524b0a6ed79675f05d2e1f3a3598670` (clean checkout). No source files changed. `AgentReviewLayoutTests.swift`, `ShellLayoutHarness.swift`, `AgentReviewWorkspace.swift`, `ThumbnailLoader.swift`, `PreviewEvents.swift`, and `AppModel.swift` Review barrier code are byte-identical to source at `65fa6a33`.

## New evidence changes the leading hypothesis

Root reports the old `65fa` release test binary passes this focused test while the user preview is open, whereas the combined candidate full suite failed in the debug configuration. Thus preview-window occlusion is not a sufficient explanation, and “debug is slower” is not established: root reports the focused release pass and debug failure both took about 34–35 s. Build configuration is still an uncontrolled difference to eliminate with the candidate release rerun, but it should not be treated as the cause by itself. There are no `#if DEBUG` branches in the review view, preview loader, or engine-library Swift paths inspected. If candidate release passes, compare the exact flight/lifecycle traces and suite ordering before assigning cause; if it fails, investigate the source-independent loader/window lifecycle hypotheses below.

## Source path and liveness risks

- `AgentReviewLayoutTests.check(_:)` creates eight real `ContentView`/`ReviewCurrentPreview` hosts (4 sizes × 2 appearances) before calling `checkReadyPreview`; each harness window runs 3 × 250 ms layout settles, is ordered behind other windows, then gets `orderOut` and has its controller removed. Those real views start asynchronous preview loads, but the loop never waits for each request/teardown to settle.
- Each `ReviewCurrentPreview` instance has independent `@State` request/task/token. `.onChange(of: identity, initial: true)` starts `load()`. The load awaits `pendingDevelopSaveBarrier`, checks token/library/selection/generation, invalidates the shared loader entry, then requests `.preview`. `.onDisappear` cancels its task/request. Whether teardown reliably delivers `onDisappear` before the next host's task resumes is a concrete lifecycle question.
- `ThumbnailLoader` deduplicates by `(EngineImageReference identity, tier)`. A canceled ImageIO/FFI call cannot be stopped: the detached task awaits the queued `BlockOperation`, and `pump()` refuses a second in-flight request for the same image/tier until the first returns. Repeated host teardown can therefore cancel the only subscriber while a decode is still running; subsequent mounts may sit queued behind that old flight. If a decode is slow/stuck, the 15 s poll sees no cached image. This is plausible but unproven.
- There is also a silent failure path: `renderResult` swallows `embeddedPreview` errors via `try?`; nil bytes, invalid image-source bytes, or failed `CGImageSourceCreateImageAtIndex` produce no image. `pump()` then exits without caching or calling the view completion when `pending == false`. The view only changes `loading` after 30 s, longer than the test's 15 s limit, and gets no failure reason. This may cause the exact timeout even when the loader did run.
- The test assertion observes shared cache state, not the particular Review view's callback. On success it proves some request inserted the image, but on failure it cannot identify whether the view guard/barrier/request was skipped, decode failed, or an old flight was canceled/blocked.
- The develop save barrier awaits captured pending open and close tasks. This test does not open Develop, so normally both should be absent and the barrier immediate; instrumentation should still record whether either key existed and how long each wait took.

## Diagnostics that separate these causes without weakening the test

Use one opt-in trace flag scoped to this test path; keep the 15 s threshold and assertion unchanged. Correlate events with a per-view token, selected image ID, owner ObjectIdentifier, and loader flight UUID:

1. In `ReviewCurrentPreview.load`, trace mount/identity values, each guard outcome, barrier open/close presence and wait duration, invalidate, request return (`nil` cache hit vs request ID), callback entry/acceptance, and `onDisappear` cancellation. This answers stale identity, stale owner, task cancellation, and missing lifecycle teardown.
2. In `ThumbnailLoader`, expose a test-only snapshot of pending/running flight keys, subscriber count, and same-key wait age. Trace cache hit, flight created/joined, pump start, render start/end/result (`image`, `pending`, nil/error), delivery/cache insertion, and `finished`. Tag output by the same flight UUID. Do not add retry/fallback or increase deadline to make the test pass.
3. At the engine boundary, report whether `embeddedPreview(imageID,maxPx:2560)` threw, returned bytes/pending, and whether ImageIO decoded dimensions. For JPEG, expected is bytes with `pending == false`; RAW-only PreviewReady waiting is not expected for these generated JPEG fixtures.
4. In `PreviewEvents`, trace the observer count for matching imageID/maxPx at subscription and PreviewReady. This detects a lost event only if the fixture unexpectedly routes through pending RAW behavior.
5. At timeout, emit `window.isVisible`, `window.occlusionState`, `NSApp.isActive`, outstanding view tokens, cache status, and loader flight snapshot. This tests window-server interference without touching/closing the user preview. Old `65fa` passing with that preview open already weakens occlusion as primary cause.

A direct post-failure `ThumbnailLoader.render(item,.preview)`/engine probe may distinguish a bad image/FFI response from a missed UI request, but should run only after preserving the original failure and recording all flights; doing it before the timeout could populate the same cache and mask the defect.

No cancellation or deadline behavior should be relaxed. The key source-level reproduction target is whether canceled background hosts leave a same-key render occupying `running` beyond the next mount's 15 s window, versus an immediate no-image render/error with no callback.

## Preserved runs

Candidate729962d9 Debug full suite:508 XCTest,1skip,1failure;5SwiftTesting pass. Identical-source focusedDebug reproduces the same nil-preview failure. Exact65fa Release baseline binary, without rebuild and with normalpreview still open, passes the focused test. Logs remain under betterSSD/tessera-validation/psd-copy-operation/{swift-729962d92524,baseline-layout-65fa}. Matched candidateRelease run is pending. No root cause or general readiness claim follows from changing build mode.
