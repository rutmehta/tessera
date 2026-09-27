# UX-02a — Review destination, navigation only

Base: main4925677. This implements the destination/navigation portion of UX-02-PLAN.md; relaunch persistence remains UX02b. Existing queue sort, merge, status and engine action semantics remain authoritative. Root coordinates review and all heavy gates.

## Concrete design

Review is an explicit non-Document workspace reached by an always-available Review(count) toolbar/menu action, including empty/all-reviewed queues. Completion updates the badge and existing toast; it does not open a modal or steal focus. Library sources/filter/multi-selection remain unchanged while Review owns selection.

A virtualized stable-ID list occupies the left of the destination. The selected detail shows Current preview, a named single-photo scope, user review status separately from critic result, rationale/steps, and explicit owner-validated actions. The Library inspector and filmstrip are hidden. Empty, failed, missing and foreign-library rows are truthful and remain visible. Preview callbacks are guarded by image ID and queue generation; there is no invented before/after baseline.

Model-owned Review navigation stores selected image ID, visible list anchor, and draft keyed to image+UUID generation. Status updates do not reorder rows. Redo merge can reorder while selection remains on the same image. Accept-and-next advances only after successful current-target acceptance, then uses queue.next(after:). It never advances on failure/rejection. Revert does not auto-advance.

Entering Review snapshots the existing Library bookmark once. Review→Edit uses a separate stable identity for the actual Develop target, including photos outside the Library filter; it must not select another visible row or clear the filter. Edit from Review hides the Library filmstrip and follows queue photos with previous/next. Back unwinds Edit→Review→Library and restores the native Library anchor after layout. Deliberate folder/source changes clear navigation. Document mode takes precedence.

Review routes keys after text/control/sheet/Document guards. Arrows move the review selection, Escape cancels the draft before backing out, and cull/accept-all letters cannot reach Library. Undo in Review is a no-op with Edit photo to undo recipe changes guidance; no claim that Accept learning is undoable.

## Ownership boundary

b516_review owns AgentController, AgentReviewSheet, AgentPanels. Agreed API: ReviewTarget with entry, EngineLibrary and UUID generation; queueTarget/currentItem; accept(target,completion:Bool); revert(target); redo(target,instruction:); readonly reviewLibrary/reviewGeneration. Reusable presentation extraction waits for that agent's commit/permission. No business logic copied into the view. Document/**, TesseraCore/Document/**, Rust and generated bindings stay untouched.

## Verification sequence

1. Navigation/state and queue-cursor tests first; RED under the shared slot.
2. Implement state, explicit target and destination; integrate agreed owner API.
3. Focused review/workspace/key/theme tests; keep legacy queue order/merge tests.
4. Background Review empty/populated/error/busy/long-name layout states and ready-target round trip. No activation.
5. Full Swift gate on frozen final source if coordinator requires; no Rust rebuild for navigation-only work.

No work is marked complete until corresponding gates run. Existing UX01 overlay/wrapping polish remains UX04.

## Integration details from source inspection

`focusedItem` is a cached observable summary, not a computed selection accessor. The explicit Review edit target must be resolved by stable key inside `refreshFocusSummary`, with no fallback to the underlying Library focus when the reviewed photo is missing. Library `focus`/`selection` stay untouched; the normal Develop controller still receives the resolved photo. Close/deactivate that session when returning to Review.

The destination should use the existing inspector column for selected rationale/actions (`AgentReviewInspector`), with virtualized list and fit Current preview in the detail area. Hide the Library source sidebar and filmstrip, not the Review inspector. Remember the Library inspector visibility and restore it when leaving Review. This keeps actions accessible within the 960pt minimum without a new split/window sizing system.

Guard Review in `perform`, `selectAll`, Undo/Redo, Layers handoff, and keyboard routing before Library/People/Develop routes. Header target derives from selected queue row while Review owns focus; never show the Library's unrelated focused filename. Missing/foreign targets keep their row/name but have disabled mutation/edit actions and an explicit reason.

Exact EngineLibrary identity is the ownership API's rule. Merely opening the same folder path in a new EngineLibrary does not reattach an old queue in this phase; no misleading reconnect/reopen promise is made. Persisted/canonical owner recovery belongs to UX02b.

## Required session lifecycle dependency

Root review identified two races before Review actions: already-closing sessions after `develop` becomes nil, and in-flight session opens that have not installed a controller. Isolated AppModel commits 4b52933, 35f8590 and 6fd51b2 retain exact-library/stable-image tasks, cancel only matching opens, await late-controller cleanup, and clear only matching active loading state. The ownership agent owns Core close coalescing/flush order and real Run/Accept/Revert regression tests. These source-only commits are not independently claimed green.

The first navigation RED ran before implementation: `AgentReviewNavigationTests.testOpeningReviewedPhotoOutsideFilterPreservesLibraryPlace`, release build23.29s, 1test/4expected assertion failures/0unexpected. It demonstrates the former source/filter/selection/focus mutation. The original output remains `swift-navigation-red.log`.

Independent review caught two scope issues before validation: `showInLoupe` is also
People/Tether inspection, and a future Review→Edit key path could reopen a session
after an agent save barrier. The final design preserves the legacy inspection
helper and uses `openReviewPhotoForEditing` for Review. Regression tests cover
both. The original RED tested the former Review call through `showInLoupe`; final
navigation tests exercise the newly separated Review route. No failing evidence
was discarded. Targeted `agent.isMutating(imageID:library:)` guards are shared by
Review entry and the central session open; unrelated library/photo opens remain
allowed. The ownership agent owns the controller's mutation ownership registry.

Busy visual coverage is a clearly labelled presentation-only fixture using the
same row and inspector with explicit busy/can-edit presentation inputs. It starts
no engine operation and claims no outcome; real entry/race tests provide behavioral
evidence. The background harness cannot materialize SwiftUI's drawn-button AX tree
without an assistive client. Busy labels/value are present in the view; containment,
spinner/disabled styling and screenshots are the bounded visual checks. A live
screen-reader traversal is not claimed.
