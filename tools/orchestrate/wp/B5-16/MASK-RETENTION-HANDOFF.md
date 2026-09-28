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
