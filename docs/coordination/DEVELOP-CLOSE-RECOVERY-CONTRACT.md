# Stage B: safe Develop close failure contract

Source-only audit of proposal `76586e43` (`tools/orchestrate/wp/UX-03/DEVELOP-WRITER-LEASE-PROPOSAL.md`) against `codex/format-version-preflight` source snapshot `36ce3092`. No product edits, runtime, builds, tests, or lease changes were made.

## Current failure and ownership gap

- `DevelopController.close()` flushes coalesced settings, marks `closed`, removes listeners, then ignores `session.close()` errors (`apps/mac/Sources/TesseraCore/DevelopController.swift:155-175`). `flushPending()` removes the patch before `setSettings` succeeds and reports failure only through a callback (`:377-394`).
- `AppModel.closeDevelop()` clears the view's controller, owner, history, and failure callback before close finishes. It retains only `PendingDevelopClose(owner, token, Task<Void, Never>)`; that task cannot report failure (`apps/mac/Sources/Tessera/App/AppModel.swift:1751-1783`).
- Close is called after transitions mutate state: `viewMode.didSet` (:205-218), `returnToLibrary` (:960-975), `returnFromPhotoEdit` (:1083-1094), `install` after replacing `library` (:468), and selection changes (`notifySelection`, :1163-1167). Those call sites cannot veto navigation on a failed save.
- The Layers barrier returns `Task<Void, Never>` and always opens after it completes (`AppModel.swift:906-925, 1020-1030`). Review preview likewise loads after the barrier without a save result (`AgentReviewWorkspace.swift:138-149`). Agent start, accept, and revert await `prepareForAgent` and proceed unconditionally (`AgentController.swift:427-433, 572-582, 610-615`; `AppModel.swift:1787-1808`).
- `releaseDevelop(for:)` uses `try? flush`, then closes without surfacing a failure (`AppModel.swift:2050-2058`; no other in-repo caller was found). `AppDelegate` has no `applicationShouldTerminate` save gate; it only disconnects tether in `applicationWillTerminate` (`TesseraApp.swift:295-300`).
- Existing Develop errors are transient `statusMessage` text set through `onFailure`; close clears that callback. There is no Develop recovery banner or retry UI. The closest current interaction precedent is the layered-document close alert with Save, Cancel, and Don’t Save (`DocumentWorkspace.swift:313-332`).
- This depends on the native Stage A retry repair. Rust `DevelopSession::close()` currently calls `shared.close()` even when `flush()` failed (`crates/tessera-ffi/src/develop.rs:2422-2428`), while the save worker clears `due` and retains only a one-shot error string (`:1202-1231`). Swift alone cannot keep a backend session save-capable after this close path.

## Minimum user contract

A close is a two-phase operation: first request and await a save-capable close; only after success (or explicit discard where safe) may the app commit the requested navigation, replace the library, open Layers, let Review read saved pixels, start/mutate Agent work, or terminate.

When save fails, show a persistent, photo-specific recovery surface, not only the status strip. Keep the `DevelopController`, its pending patch/session, and the exact strong `EngineLibrary` owner plus `imageID` in an observable recovery entry. The alert identifies the photo and failure, states that the requested action did not happen, and offers:

- **Retry Save**: invoke close/flush again on the same retained controller. On success, remove the recovery entry and resume the captured action if its owner and navigation-intent token are still current. On repeated failure, keep the entry and refresh its error.
- **Keep Editing / Cancel**: abandon the pending navigation and restore the original photo workspace with the previous settings/session available. For folder change, keep the old library open and cancel the new folder request.
- **Discard Edits**: close without trying to save and continue only when the backend can truthfully guarantee what is discarded. Stage A explicitly allows recipe publication before an XMP/index error and does not make those files transactional. Therefore the UI must not offer or label this as “discard unsaved changes” for a partial-publish/repair-pending failure unless the backend adds a safe, owner-checked rollback. In that state, offer Retry and Keep Editing; disclose any already-published recipe if a future recovery choice intentionally abandons auxiliary repair.

For Quit, intercept `applicationShouldTerminate`: first attempt the same save gate; on failure, present **Retry Save / Discard and Quit (only if safe) / Cancel Quit**. Cancel keeps the process and recovery owner alive. Do not rely on `applicationWillTerminate` for saving: it is too late to veto termination. Do not claim crash/power-loss recovery from in-memory retention.

## Minimum ownership/API changes

1. Make `DevelopController.flushPending()` report success/failure and preserve the coalesced patch until `session.setSettings` accepts it. Make `close()` an awaitable result/throwing operation whose repeated callers share the in-flight result. Do not mark closed or detach recovery callbacks before the backend confirms successful close. Add an explicit backend discard/abort only with a precise durability-state contract.
2. Update Rust close so a failed flush leaves a retryable session/worker alive; successful retry drains and closes once. Keep Stage A's retry-pending auxiliary-write state distinct from the last error. Expose whether an error is wholly volatile or follows recipe publication so UI does not promise rollback it cannot perform.
3. Replace `Task<Void, Never>` close/open save barriers with a result-bearing barrier keyed by captured `(EngineLibrary identity, imageID)`. Store recovery strongly by this key with controller, failure, and retry state. Only remove it after successful close or safe explicit discard. Keep all presentation and status updates generation/owner checked.
4. Route transitions through an async close gate **before** mutating selection, `viewMode`, or `library`. Do not put `await` inside the existing `didSet` after-the-fact close. Capture the user intent, old workspace bookmark, old owner, and intent token; apply the transition only after success. A later navigation request must supersede or invalidate a stale pending continuation safely.
5. If close fails, block all result consumers: Layers must not open; Review must show a save-failure state rather than request saved pixels; Agent start/accept/revert must not launch and must release busy/mutation ownership; folder install must not replace the old library; Quit must return cancel. Recovery remains addressable after the viewport detaches.

## High-risk migration points

`AppModel.viewMode` observer, `openFolder` → `install`, `returnToLibrary`, `returnFromPhotoEdit`, `enterReview`, `notifySelection`, `openDevelop`, library-update item removal, layered-copy request, and `prepareForAgent` are all direct or indirect close callers. The Save-as-you-leave flow has to cover keyboard navigation and shortcuts as well as toolbar buttons. Search found three barrier consumers: Layered handoff, Review preview, and Agent start/accept/revert. `releaseDevelop` is currently unreferenced internally but must still become result-bearing if kept as API.

## Deterministic acceptance scenarios

- Inject a `setSettings` rejection with a pending coalesced patch. Assert patch remains pending, close reports failure, controller/session remain save-capable, and a second close after removing the fault writes that same edit before successful close.
- Inject save-worker failure after recipe rename but before XMP/index. Assert no false success on repeated close; retry without new edits repairs auxiliary state. Assert UI does not offer rollback/discard as though the recipe had never published.
- For loupe next/previous and Review Edit → Review/Library, fail the close and assert focus, selection, and mode remain on the original target. Retry saves and then performs exactly the captured transition; Cancel leaves the editor usable.
- For folder switch, fail close and assert the old library remains installed and the requested folder is not scanned/installed. Retry then switches once; Cancel retains old folder. Repeat after the old photo disappears in an in-place remap and verify recovery still holds the original owner and image identity.
- For Layers and Review preview, fail the barrier and assert no document opens and no saved-preview request/invalidation runs. After successful retry, exactly one handoff/read proceeds.
- For Agent run, accept, and revert, fail their captured close barrier and assert no engine mutation starts; clear busy/source ownership and keep Review actions available. Retry then allows exactly one requested mutation.
- For app termination, fail close and assert termination is canceled and the recovery action is visible; Retry success allows termination, while Cancel keeps app/session open. Test repeated concurrent close callers receive the same outcome and do not double-close.
- Exercise explicit discard only for a backend state proven safe to abandon; confirm no retry/save is silently scheduled afterward and the status accurately describes any already-published data.

No UI styling, new edit feature, writer lease, or `BDocument` changes are proposed here. Stage C lease activation and batch API remain out of scope until Stage A and this Stage B contract are implemented and gated.

## Coordinator scope for implementation

The initial recovery UI offers Retry Save and Keep Editing/Cancel Quit only.
Do not implement Discard until a separately reviewed durability/rollback contract
exists. Stage A repairs failed publication but does not change close semantics;
Stage B must explicitly keep failed-close sessions usable and gate downstream
work. All UI changes remain unimplemented at this document checkpoint.

## Peer review additions — 32db07de

B's source-only caller review is preserved at
`tools/orchestrate/wp/B5-16/DEVELOP-CLOSE-CALLER-REVIEW.md`. A verified the mask
queue clearing, cancelled-open cleanup and last-window termination paths in source.
Stage B must retain rejected and unattempted mask edits without replaying accepted
brush points; transfer failed cancelled-open cleanup to an owner/image recovery
entry before its task record disappears; gate folder side effects and last-window
close as well as Quit; unwind Agent busy state and recorded running intent when
no mutation launched. Export's saved-versus-live source boundary remains a source
audit item, not a verified guarantee. These are implementation requirements and
unrun acceptance cases, not completed behavior. Stage A native retry is now merged
(main d4274a68); native failed-close recovery remains separate.

### Export/Print source boundary confirmed by A

`AppModel+Output.swift:68` and `:98` call `try? d.session.flush()` before
presenting Export and Print. They neither submit the controller's pending settings
and mask queues nor propagate a native save failure. ExportController.start
(:273–289) and watermark preview (:218–237) then invoke `exportBatch`; native
`export.rs:803–814` reads the authoritative disk recipe/XMP under the catalog lock.
This is a saved-recipe reader, not a snapshot of unsubmitted visible controls.
Stage B must route these launch paths through a result-bearing host-and-native
save barrier, including matching owner/image pending cleanup recovery. Do not
claim the catalog lock flushes host edits or repairs a prior failed save. Initial
acceptance must inject host patch rejection and native post-recipe failure and
assert no export/print launch until retry succeeds. These checks are UNRUN and
no output product behavior changed in this source audit.
