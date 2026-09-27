# UX-01 — Library / Photo Edit workspace

Status: first workspace slice implemented; automated gates and bounded background visual acceptance passed. Remaining interactive/performance and follow-up polish limits are stated below. Source owner: Machine A, isolated managed worktree `codex/workspace-redesign`, base `98ce450`. No main merge/push or foreground activation. Document internals and Rust/FFI source are unchanged.

## Scope and decisions

The recovered design/prototype is durable under `docs/design/workspace-redesign/` (design recovery commit `e3f6e48`). Precision Graphite is the coordinator’s provisional direction; this slice retains existing theme tokens and amber identity. It does not assert historical approval or perform a palette sweep.

Library preserves Grid/Loupe/Compare inspection and culling; explicit Photo Edit shows one target, Develop/Masks tabs, domain-specific undo, and a return bookmark keyed by photo identity. The native browser preserves an anchor through later sidebar layout passes, cancels it on navigation, and invalidates retained positions on live identity/order changes. Source/folder changes leave Edit. Exiting through Back, direct mode change, Document or replacement library settles completed edits and disarms crop/mask/HSL/detail/Upright pointer tools. Library Loupe cannot consume stale edit pointer gestures. Explicit selftest entry paths now enter Photo Edit.

The existing Library Loupe is fit-only: it has no independent zoom/pan state to restore. Compare’s existing zoom toggles are retained. This slice introduces no Loupe zoom subsystem.

The Library-side action is **Open in Layers…**. Its disclosure captures a source photo and states that first open creates rendered pixels, while an already-open copy reopens unchanged without refreshing from later photo settings. Source settings and document save/history stay separate. No new-copy API, live RAW layer, or Document implementation change was made.

## Validation evidence

- `swift-focused.log`: first release compile failed at `ContentView.swift` toolbar builder’s eleventh top-level child (`extra argument in call`). No tests ran. Toolbar actions were factored into a second `ToolbarContentBuilder`.
- `swift-focused-repair.log`: repaired release build succeeded; 27 focused tests ran with 1 failure. All state/keyboard/anchor/theme tests passed. Shell geometry passed but the new empty Masks tab exposed one native control while the old harness smoke check required two; all eight Masks size/appearance combinations hit that assumption. The state-specific check now requires the real picker plus Edit/Masks/target state; no containment or overlap limits changed. Existing Document inspector overflow stays under its pre-existing expected-failure annotation, owned by B.
- Parent review additionally required invalidating the resolved return position when live image keys/order change; the final browser regression covers reflow and subsequent reorder→resize.
- `swift-focused-final.log`: release build and 26 focused tests passed, zero failures (0.431 seconds). Source manifest: `final-source.json`, aggregate `07b6935efcb56608b9a96c70367cc5f28a7e9ab7174cccac3ed05c99f8c7dbef`.
- `swift-full.log`: full release suite passed on that source: 411 XCTest cases, 1 skipped, zero failures in 122.339 seconds, plus 5 Swift Testing tests passed. All 40 background shell layouts passed; existing Document inspector expected failure retained. Capture hashes/paths: `layout-before-copy-repair.json`.

FFI cache provenance is recorded in `ffi-cache-provenance.json`. Rust source at this branch’s base equals `c0d4535`, and generated Swift/header bytes match the source-compatible existing archive. This Swift-only slice adds no FFI APIs, so it does not rebuild Rust. The existing archive emits a deployment-target warning for its blake3 object (26.5 versus the linked 15.0 target); it is retained as a cache limitation, not hidden.

## Limits

No remaining Develop P01/P11 presentation/performance acceptance is claimed. Empty Masks/preview-only inspector layout is covered; real photo adjustment, active mask gesture, completed crop/Upright behavior and repeated existing-copy recipe divergence need the specified interactive acceptance workflow. No durable Review, queue persistence, batch sync or live RAW Document work ships here. Parent is reviewing the final source and visuals before integration.

## Visual review follow-up

Coordinator and implementer inspected Library and Edit captures. Geometry was correct, but the reused Loupe overlay still showed culling shortcuts/best-frame instructions in Photo Edit, and empty Masks incorrectly instructed “Open a photo in the loupe (E)”. The final copy repair removes Edit cull badges, uses workspace-specific shortcuts, gates the Masks overlay button to Edit, and uses the actual Develop unavailable/loading reason in empty panels. `swift-copy-repair.log` verifies the repaired source: release build and 28 focused tests passed with zero failures in 54.998 seconds, including all 40 layouts. Fresh captures are in `layout-final-manifest.json`. The full suite above predates this small visual-copy repair and is not represented as verification of later source.

## Acceptance map

| Contract | Evidence / limit |
|---|---|
| One active edit target; Library selection and Compare return | WorkspaceNavigationTests and stable-key tests pass |
| Native anchor through sidebar reflow; no hidden-grid focus snap; live reorder cancels stale position | WorkspaceBrowserRestoreTests pass in unshown native window |
| Domain undo, text/key priority, no cull keys or stale pointer tools in Edit exit paths | WorkspaceNavigationTests, KeyFocusTests, DocumentKeyRoutingTests pass |
| Fixed Develop/Masks and explicit target/status | Preview-only matrix plus one ready Sony RAW check passed with enabled Develop and selected-mask controls; full interactive gesture acceptance remains open |
| Source/folder changes drop old return state | Navigation tests pass |
| Layered-copy disclosure capture/cancel and existing Cmd-E routing | Tests pass; real engine repeated-copy/current-recipe divergence is documented, not newly automated |
| Geometry, theme tokens, background-only captures | Full suite 40-layout matrix + ThemeLint; final visual-copy rerun passed |

No source changes under `Sources/Tessera/Document`, `Sources/TesseraCore/Document`, Rust crates, generated FFI, or other agents’ worktrees. Existing Document regression tests ran in the full Swift gate. The one skipped XCTest is the pre-existing opt-in generated 20,000-file engine measurement, not a redesign acceptance test.

## Final ready-photo verification and visual review

`WorkspaceReadyPhotoTests.testRealRAWDevelopAndMasksRemainReadyAcrossInspectorTabs` uses the production shared model/controllers with a scratch Sony ARW copy, waits for ready status and a final rendered frame, and checks the one-RAW target, enabled native adjustment sliders, containment/overlap, selected mask, unchanged controller and unchanged history across inspector tab switching. A real linear mask is created only in the scratch session. Release test passed: 1 test, zero failures, 5.478 seconds (`swift-ready-raw.log`). Two 1440×900 dark window captures show populated histogram/Basic and selected Mask 1/local adjustments.

Coordinator and implementer independently visually inspected Library, preview-only Edit/Masks, and both ready-RAW tabs. Coordinator accepted the first slice with the two existing dense-layout details queued for UX04 polish. Six representative PNGs are committed under `screenshots/`; all 42 final captures are hash-manifested externally. Prohibited activation assertions passed. The reused Loupe still has technical info/shortcut text partly over photo pixels in the ready-RAW capture, and the existing Masks “Components” label wraps at this inspector width. The new workspace header/target/tabs remain outside pixels and readable. These are recorded follow-up polish items, not hidden by geometry success.

Product source remained unchanged after the 28-test final copy-repair gate. Only the independent ready-photo test was added afterward and passed separately. `final-verified-source.json` records the final source/test hash set; `commands.json` records all commands and exit codes. No extra Rust/Xcode build, foreground launch, presentation latency claim, main merge or push was performed.

Source/docs `git diff --check` is clean when raw `.log` evidence is excluded. Four compiler logs retain eight original diagnostic source-excerpt lines ending in spaces; those bytes are preserved rather than rewriting historical evidence.
