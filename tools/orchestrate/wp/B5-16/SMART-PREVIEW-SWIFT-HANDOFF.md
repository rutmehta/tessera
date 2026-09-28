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
- `apps/mac/Sources/Tessera/App/AppModel.swift`: explicit persisted preference defaulting to Smart Previews when available. RAW opens query fresh native state and route through the existing pending-open/recovery registry; stale/cancelled produced sessions still close through that registry. Original is allowed only for a clean, safe online source; missing preview falls back visibly to Original. Source changes and batch actions first use the existing recipe-read save reservation; source reopen happens only after actual barrier completion and release. Batch reserves captured image IDs through native drain. Owner and load-generation guards preserve current selection/Library. Labels read the actual installed route, not merely preference.
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
