# Smart Preview Swift UI source handoff — UNRUN

Request: d7764a2f-a47f-4542-84b6-b1459a144d8b. Accepted through the B Git mailbox before source work. Branch: `codex/smart-preview-ui`; exact base `fa7372b95ad4e54a695adff877e64bde6e5fcdb2`. Save As branch `codex/save-destination-swift` remains at `3ca3e9c9`, unchanged. A alone owns native implementation, generated bindings, compiler/GUI gates and main integration. B resource hold and paused heartbeat remain in force.

## Checkpoints

- `8f98ff26`: six deterministic test methods, source only.
- `dbe266b6`: two additional generation/owner negative-control tests, source only.
- `3a9995f3`: production Swift integration.
- No Rust, generated bindings, Document/Save As, theme, dependencies, or existing preview app changed.

## Product/source map

- `apps/mac/Sources/TesseraCore/SmartPreviewController.swift`: actor-isolated injected async API; value snapshots and badge labels; explicit Original/Smart Preview routing; selection generation plus per-image mutation revision suppression; serial batch per-photo outcomes and cancellation only between photos. Current native operation drains and reports its actual outcome before cancellation skips remaining photos. Dirty discard fails before native discard; native must still enforce this atomically. Conflict/stale/failed are never success, and sync additionally requires ready, original available and no pending edits. No source retry after a failed opener.
- `apps/mac/Sources/TesseraCore/SmartPreviewNative.swift`: direct frozen API mapping; every blocking native call in a detached utility task. **Requires A's native API and generated bindings before this branch can compile.** No generated-source edits, conditional availability stub or fabricated success.
- `apps/mac/Sources/TesseraCore/DevelopController.swift`: additive default-original route argument; actual native session constructor selected off MainActor; immutable actual `sourceRoute` on returned controller.
- `apps/mac/Sources/Tessera/App/AppModel.swift`: explicit persisted preference defaulting to Original when no choice is saved (decision bd498fd4). RAW opens query fresh native state and route through the existing pending-open/recovery registry; stale/cancelled produced sessions still close through that registry. Original is allowed only for a clean, safe online source; missing preview falls back visibly to Original. Source changes and batch actions first use the existing recipe-read save reservation; source reopen happens only after actual barrier completion and release. Batch reserves captured image IDs through native drain. Owner and load-generation guards preserve current selection/Library. Labels read the actual installed route, not merely preference.
- `apps/mac/Sources/Tessera/Library/SmartPreviewMenu.swift` and minimal `AppCommands.swift`: Build/Discard/Sync selected engine-backed RAW photos, progress, cancellation, per-photo results and copy-all report; preference and current source/state accessibility identifiers. Render at most 100 result menu rows; all results retained/copyable. No automatic build, polling, timer or eager status scan across the catalog.
- `apps/mac/Sources/Tessera/Grid/ThumbnailCell.swift` plus two call sites in `ThumbnailBrowser.swift`: cached status chips and AX labels, refreshed through the existing targeted item notification. No native calls from thumbnail rendering. Previously unvisited thumbnails have no Smart Preview cache badge until selected or processed in a batch.

## Tests — all UNRUN on B

`apps/mac/Tests/TesseraCoreTests/SmartPreviewUITests.swift`:

1. Requested native route and no fallback after proxy opener error.
2. Missing-preview safe fallback, unsafe Original refusal, stale/conflict/failed refusal, offline proxy route.
3. Offline/pending/conflict/stale and actual-route labels.
4. Old selection completion cannot replace the new selected photo.
5. Read started before a batch cannot overwrite the newer dirty outcome.
6. Cleared selection/separate owner ignores old completion.
7. Cancel drains the active build, starts no second build, retains its outcome and marks successors not started.
8. Conflict/dirty sync/throw are not success; dirty discard does not invoke native discard.

Only source inspection and `git diff --check` performed. No Swift compiler, tests, app launch, benchmark, FFI regeneration, GUI, or heartbeat restart on B. These are written tests, not passing evidence.

## A integration and acceptance gates

Generate/link the exact frozen five Engine APIs plus SmartPreviewInfo/State, then compile with Swift 6 strict checking. Independently review the source and exercise the eight tests and existing Develop recovery/navigation tests. Native admission must still provide stable per-image writer ownership across normal and proxy sessions, atomic dirty-discard refusal, durable local saves and conservative reconnect; UI state is not a filesystem lock. Status methods are uncancellable reads; native performance and cancellation duration are unmeasured.

Real GUI gates: mixed RAW/non-RAW selection, serial Build progress and between-photo Cancel, clean Discard and dirty refusal, preference switching during a live/pending/failed close, actual-source badge, generation changes during opening, offline edit/save/relaunch, stale/conflict preservation, successful reconnect/Sync and original-quality export. Verify no old session leaks on cancellation. Check menus/AX and truncated thumbnail status at realistic grid sizes. The existing export path is unchanged and must surface native unavailable/conflict errors; no proxy export or upscaling path is introduced.

This is a source handoff only, not a working app, measured speed improvement, full RAW/camera qualification or lossless/pixel-identical reimport claim. Original photos were not touched by B.

## Follow-up cd101ed1 — full-asset status cost and thumbnail disclosure

Validated exact target and accepted before changes. A reports main46469b08 contains bounded journal/codec, with native integration still pending. This does not change the Swift branch base or generated API.

- Tests-first `f9bfa533` and `bf80fa22`; production `3a0f2cbf`. The suite now has **13 methods, all UNRUN**. The old concurrent-read tests now honor native read drainage while preserving stale-result and newer-batch outcome assertions.
- One active selection validation per controller. Superseded queued selections do not call native; the already-running native operation drains. Same-selection notifications reuse its task/result. Develop opening shares the selection validation instead of independently hashing/decoding both assets. A stale opener cannot retarget current selection.
- A batch drains any active selection validation before native mutation. Reads requested during the batch do not launch. Returned per-photo native state supplies badges; no immediate redundant validation for those results.
- Save callbacks invalidate cached status without launching asset validation. The next explicit selection/open/source-change or new **Check Status for Selected Photo** action obtains it as needed. Source changes explicitly invalidate after actual save/close. Native open still MUST revalidate source/journal identity and writer admission: the cached snapshot is a routing hint, never authority to bypass pending offline edits. Failed native opens still do not fall back.
- Off-MainActor detached native calls remain unchanged. No per-cell reads, timers, polling, catalog-wide validation, builds or benchmarks introduced. An old owner's in-flight native read cannot be interrupted; it drains and is not published to a new owner. This is not a cancellation-latency guarantee.
- Menu disclosure: “Library thumbnails show the last synchronized image. Smart Preview edits appear there only after Sync.” Offline or pending-edit thumbnail/AX badges append “Thumbnail: last synchronized image.” This does not claim the offline Library thumbnail depicts current Develop edits. Freshness badges are invalidated after saves rather than fabricated from an assumed dirty state.
- New test coverage: repeated selection/opening shares one read; rapid selection validates active+latest only; save invalidation starts no read and explicit refresh does; stale opener cannot change selection; offline/pending thumbnail disclosure. Existing read-to-build case additionally records that build does not overlap the active read.

Only source inspection and `git diff --check` performed. A still owns compiler, all 13 model tests, native integration and GUI. Verify actual background execution/full-asset read counts, callback ordering, shared-result invalidation, native rejection of stale/dirty Original opens, post-Sync thumbnail refresh, and visible/AX disclosure in the integrated app. No compression-ratio or editing-speed promise is present. A's single Sony size observation and absence of speed measurements are not generalized into product claims.


## Product decision bd498fd4 — Original default, explicit offline choice

Exact target validated and accepted before source changes. Tests-first `87bd828c`; production `d5a59393`. The combined branch retains all status-coalescing and thumbnail-disclosure changes above. It now has **15 deterministic test methods, all UNRUN on B**.

`SmartPreviewPreference.read` defaults an absent value to false without writing any value; explicit saved true/false preferences remain unchanged. AppModel uses this helper and the existing key. No migration overrides a prior choice. Regression checks absent/default Original, saved opt-in/Smart Preview, saved false and no implicit persistence.

With Original selected but a valid preview present offline, routing reports the exact Library → Smart Previews → Use Smart Preview action instead of telling the user to build or synchronize an already-existing offline preview. A contextual menu button invokes the same explicit preference/save/reopen path. Missing preview still asks to reconnect the original before building. Stale/conflict/failed states still block; a failed native opener never silently switches source. The new regression covers clean and dirty valid offline previews, explicit opt-in and the missing-preview negative control.

Rationale supplied by A (not measured or rerun on B): matched 2460×1638 warm edit proxy CPU 384.7 ms vs original Metal resident including readback 33.07 ms (roughly 11.6× slower); CPU original 818.2 ms does not justify automatic proxy selection on the existing Metal path. This single measurement is not a universal camera/performance claim. UI makes no compression or speed promise.

Native API unchanged. Source/whitespace review only. A must compile and execute all 15 tests, confirm persisted preference behavior and the actual offline menu/reopen/save flow, and own integration/main. No B workloads, apps, benchmarks, heartbeat or writer changes.

## Review bcf6cb50 — keep local proxy-save presentation through invalidation

Validated exact target and accepted before changes. Tests-first `2c51982c`/`d2fa3526`; production `8d35e9f6`. Combined suite now **18 methods, all UNRUN on B**. A reports only the initial eight at c6d05aa3 passed; that does not validate the later 15- or 18-test candidates.

AppModel's successful save callback now forwards the actual completing controller's immutable source route, under its existing owner/image guards. SmartPreviewController stores a separate presentation-only local-save record for Smart Preview saves. Native routing snapshots still clear and their generations/revisions invalidate exactly as before; autosave launches no native read.

Library thumbnail/AX and selected menu warning retain “Smart Preview · Local edits saved · Status needs refresh · Thumbnail: last synchronized image”. Historical offline evidence is explicitly labeled “Original last checked offline”; it is not current availability. Original saves do not fabricate proxy-save evidence. Ordinary status invalidation does not erase the record; fresh dirty/offline/conflicting status retains it, while validated clean online-ready or clean removal retires it. Current native status remains the display source when available.

Opening uses only a newly available validated routing snapshot, never presentation state. Regression asserts warning survives proxy-save invalidation with no extra reads, opener performs a new read, clean fresh result replaces the warning, failed validation cannot open using presentation, and Original save does not invent a proxy badge. Existing status coalescing, default Original, explicit saved preference, offline action and native revalidation contracts remain unchanged.

Source/whitespace inspection only; no builds/tests/apps/benchmarks or heartbeat/writer changes on B. A owns native compilation, all current tests and integrated GUI validation, including actual autosave callback source identity and thumbnail/AX/menu warning persistence.

## Review 06afa311 — follow replacement status checks during opening

Validated exact B target and accepted before changes. Tests-first `a0a6389e`; production `dd84ed57`. Combined suite **20 methods, all UNRUN on B**. This follow-up changes only SmartPreviewController product source and its existing test file; proxy-save presentation fix remains intact.

Opening now captures a photo-selection identity separately from status-read generation. After the captured task drains, a newer same-photo Check Status generation is awaited instead of treating its temporarily missing snapshot as failure. Each repeat follows a real observed generation replacement; no timer, polling, busy retry, native error fallback or duplicate status read. A photo change invalidates opening even if selection changes back to the same image before the old read finishes. Task cancellation and batch admission remain checked. Native opening still revalidates independently.

Two gated-read regression: hold read 1, prove opener captured it, request explicit same-photo refresh, complete read 1, hold read 2, prove opener now awaits generation 2 without completing, release read 2 and require its exact snapshot with exactly two native calls. Negative control changes selection away and back and requires CancellationError without reviving the old opener. Internal nonescaping `willWait` observer identifies the exact captured-await boundary for deterministic tests; production passes a no-op. XCTest deadlines are failure bounds, not product timing workarounds.

Source inspection and git diff --check only. A must compile/run all current tests and validate Check Status during actual opening. No offline Library startup/relaunch changes or speculative bindings: waiting for A's exact cached-preview-session API contract. No B workloads/apps/benchmarks/heartbeat/writer changes. A owns runtime and main.

## Offline Library request 889b3f34 — declaration-only relaunch and read-only host

Exact target validated and accepted before edits. Tests-first `dac7126e`; product `c5e001e5`. Existing status-generation and proxy-save badge fixes remain intact. Source/whitespace review only: **all 20 SmartPreviewUITests and 6 OfflineLibraryRoutingTests UNRUN on B**. No compile, app, benchmark, heartbeat or writer change. A owns Rust, generated bindings, runtime gates and main.

`TesseraCore/LibraryOpenRouter.swift` validates absolute lexical/no-.. paths and performs one worker-side folder-availability probe. ENOENT selects cache; permissions, non-directory and ordinary online scan/decode errors remain errors. The original and cached closures are mutually exclusive. `EngineLibrary.open` uses it; `cachedPreviews` invokes the frozen additive `engine.openSmartPreviewLibrarySession(folder:)`, reads declaration rows/groups/changeSequence, and never indexes/lists/canonicalizes/stats the original. It opens only the local Engine support store. **The new binding is intentionally not hand-generated; source requires A's native API/binding integration.**

`TesseraApp` lets an absent remembered folder reach the router. `AppModel` normal folder/recent open and full reopen use it; same-folder rescan probes before indexing, switching to cached on absence. Same-folder shortcut compares lexical paths, avoiding original symlink canonicalization before that probe. Cached reopen takes the full factory path, so reconnect or membership refresh is explicit. Cached mode skips catalog listener/autosync, library.json adapter, assist/people/understanding/profile/Review resume startup. Narrow guards in those existing helpers are required to prevent original-folder work during install. Full reload does not restore an incompatible album/People source into a cached library.

`LibraryAccessMode` is visible in the workspace header with declaration-only/no-subfolder disclosure and Reopen Library. Empty mode asks to reconnect/build/reopen. Cached ItemStatus is explicitly a declaration, not an invented Unedited phase, in grid/AX/inspector. Catalog culling/basket/people mutation routes reject clearly; related menu/inspector controls are disabled, groups navigate locally, basket membership is empty, and no derived-status/album refresh is requested. Default Original/explicit Use Smart Preview, current opening revalidation, Develop journal editing and presentation-only pending-thumbnail warning remain unchanged. No Document/Save As/load/activation edits.

Six deterministic router/mode tests cover missing-folder routing without invoking original scan/list closure, empty result/guidance, online decode error preservation, availability permission error preservation, explicit offline-online-offline transition, and read-only/path validation. These are **not** real Engine factory or AppModel relaunch integration tests. A must compile with actual generated binding, execute all 26 tests, verify native recursive membership/bounded journal validation/read-only enforcement, and run actual missing-folder relaunch + cached empty + reconnect/reopen + RAW explicit-preview edit/save/relaunch GUI checks. Confirm no original directory recreation/list/index or per-cell full-asset validation, no startup ancillary jobs, and real local thumbnail behavior. Cached membership is not current pixel/hash/original-availability acceptance. No performance/compression claim.

## Alias authority review c13b4fe0 — capture online index path before disconnect

Exact B target validated and accepted before source work. Tests `ea40431c` + `782752f3`; production commit recorded in the following Git history. The initial offline source ef307d8d was defective: native lookup is lexical, while scan stored the caller alias and AppModel remembered it before successful indexing. This correction does not weaken the native contract.

`EngineLibrary.scan` now retains the already-returned canonical index `handle.path` as `folder`, preserving the caller's friendly title. AppModel persists only successful, current-generation opens using that library folder; last-folder and recent history replace the requested alias, deduplicate the canonical path and retain unrelated missing recents using lexical comparisons only. The internal `rememberOpenedFolder` adapter accepts an isolated UserDefaults suite for regression without changing personal preferences. Successful full reopen/rescan and cached transitions use the same persistence adapter. A rescan whose online index returns a different canonical path constructs a new library/session against that identity rather than relabeling the old CullSession. That exceptional online identity-change branch performs a fresh scan; it is never taken for offline absence. No offline symlink resolution, alias guessing, native API/binding change or Document edit.

New regression uses a real online EngineLibrary.scan through a temporary symlink to an empty folder, asserts canonical folder plus friendly title, persists it through the actual AppModel history adapter into an isolated suite, and verifies alias/canonical deduplication and preservation of an unrelated missing recent. It then removes the alias and moves the original, reopens the saved path through real availability detection with an injected cached closure, and requires exactly the captured canonical path with no original closure. This is stronger than a mocked online path, but still does not assert real Smart Preview membership or actual GUI launch. All **27 tests UNRUN on B** (20 Smart Preview +7 offline routing); no compiler/apps/workloads/heartbeat/writer changes. Source inspection and git diff --check only.

A must compile/execute, verify the native Sony alias-disconnect fixture and actual persisted relaunch, plus rescan/reconnect behavior. Preexisting history containing only an already-disconnected alias has no captured canonical authority; this correction deliberately cannot infer that identity. Reconnect/open it once to record the native path. Full offline factory/bindings/GUI gates from the preceding handoff remain pending on A. A alone integrates main.
