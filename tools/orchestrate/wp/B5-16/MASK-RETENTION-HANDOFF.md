# Coalesced mask pending-edit retention candidate

Request: a25ad2ff-96db-4623-95f5-af64fffdff57.
Base: origin/main 46c61b5f. Candidate branch: codex/mask-pending-retention.
The existing wp/B5-16 branch and its CODEX-TAKEOVER.md remain preserved at 32db07de.

Commits, in review order:
- c156c623: six deterministic fault test sources, UNRUN.
- 9ee4957d: successful reentrant acknowledgement test and callback-cycle cleanup, UNRUN.
- 986512b6: product implementation confined to DevelopController+Masks.swift.

Source diff: git diff 46c61b5f..986512b6 -- apps/mac/Sources/TesseraCore/Develop/DevelopController+Masks.swift apps/mac/Tests/TesseraCoreTests/DevelopMaskPendingRetentionTests.swift

Accepted operations are acknowledged individually. A rejected operation and unattempted suffix remain pending. Rejected group patches restore older fields underneath newer per-field values. Successful callbacks cannot erase newer same-key coalesced changes. MainActor in-flight bookkeeping blocks recursive drains and buffers new brush samples during submission; accepted brush batches are not replayed after a later operation fails. The entry is removed with defer, without retaining the controller.

Seven test expectations: rejected component retains groups/parameters; later group rejection does not replay accepted brush/component/group prefix; parameter rejection retains only its suffix; reentrant replacement survives failure and nested flush does no work; group field merge preserves untouched older fields; success preserves newer same-key values; rejected brush batch survives and successful submission preserves new samples. The fake uses the generated noHandle subclass path, not a real image/render session.

Validation: source inspection and git diff --check only. All tests UNRUN and Swift compilation UNVERIFIED on B. A must compile and independently review before acceptance; no B workloads, apps, benchmarks or heartbeat restart occurred.

Limits: Bool remains attempt semantics, not persistence success. This does not implement result-bearing close, close recovery, structural-edit rollback, noninteractive-final submission retention, or begin/end-stroke lifecycle recovery. Existing single latest component slot remains unchanged. Backend rejection is assumed to mean the submitted operation was not accepted; partial backend application is not repaired here. No automatic retry loop was added. DevelopController.swift, AppModel, generated bindings and global UI are untouched. A retains sole main merges.

## Source revision after A withheld acceptance

Request 13fb8907-3153-411e-9972-a88c75330fd3 identifies a source liveness blocker, not a runtime failure. Original candidate commits are preserved. New test commit e4ffb7b3 adds four UNRUN tests (11 total): two accepted reentrant generations with nil handler; the same with a synchronous handler; an older queued drain invalidated by an explicit rejected attempt; and no automatic follow-on after a rejected reentrant batch. Async tests await the actual queued Task handles through an internal read-only observation property, without fixed sleeps or manually flushing successful follow-ons.

The product revision queues one MainActor task for pending work only after a successful batch. Each task consumes at most one batch and may schedule one successor after another success. A per-controller UUID distinguishes task generations. An explicit flush removes/cancels the previously queued task before attempting work, so its later callback cannot retry an intervening rejection or remove a newer generation. Task closures weakly capture the controller. The task removes its registry entry before draining, including when the controller has disappeared. Closed controllers reject the queued flush. Reentrant synchronous handlers still encounter the in-flight gate; progress no longer depends on those handlers.

Bounded means one queued task per controller and one batch per task, not a cap on a continuously arriving stream of successful edits. Failures retain pending work for a future explicit/user-driven attempt without scheduling their own retry. Close recovery and noninteractive/structural/begin-end-stroke semantics remain outside this change. No tests, compiler, apps, bindings generation, benchmarks or heartbeat restart ran on B. Source diff check only; A compilation/review remains required.
