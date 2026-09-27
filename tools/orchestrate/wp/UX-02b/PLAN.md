# UX-02b — durable Review resume: source audit and scoped contract

Status: approved bounded implementation in progress on `codex/review-resume-persistence`, based on `origin/main` commit `11fc33906e9900039c07c88cbc18db3d4263c7eb`. The coordinator approved one latest queue per canonical library path in isolated app support, explicit recovery without automatic replay, recipe-authoritative statuses/group checks, and preservation of existing queue ownership. Product implementation and deterministic tests are complete for parent review; final focused gate evidence is recorded in `EVIDENCE.md`. No app GUI launch or user-data access was performed.

## Existing behavior and source evidence

- `AgentController.queue`, `queueOwner`, `queueGeneration`, `showReview`, progress, and failures live in memory. `didFinish` (`apps/mac/Sources/Tessera/Agent/AgentController.swift`) creates/replaces or merges a queue only after `run_agent` returns; only a non-redo nonempty success sets `showReview`.
- Auto Edit captures the selected target IDs at Start (`AutoEditSheet` calls `itemIDs(for:)`, then `start(itemIDs:)`). The FFI request gets image IDs, guardrails, provider, library folder, and optional instruction, but not a run ID or UI scope. Provider and the target list are not stored as one durable run record.
- Per-photo recipe data is already durable. `crates/agent/src/lib.rs` writes `unknown.tessera_agent_v1` while executing and at completion. It contains a schema `version`, current history group and report; `crates/tessera-ffi/src/agent_runs.rs::review_item` reconstructs the latest item, and `Engine::agent_provenance` exposes it. Accept/Revert update that recipe's `review_status`. This is only the latest agent group for one photo; it does not enumerate a run or preserve a photo-level error when no recipe was written.
- The current `AgentRunReport` contains `items`, `cancelled`, and `provider`; `AgentReviewItem` contains stable image ID, group, critic detail, steps, status, and error. It has no run identity, captured scope/target-set identity, timestamps, or durable queue order/cursor.
- `ReviewNavigationState` keeps selected ID, anchor, ephemeral queue generation, and redo draft in memory. UX-02a keeps completion non-modal; preserve that behavior: launch should make a valid Review destination available, not force-switch from Library or steal focus. The resume record persists only the selection and list anchor, not the draft or runtime generation.
- Ownership hardening is substantial and must remain the action boundary: `queueTarget` captures the exact `EngineLibrary` object, stable photo identity and `queueGeneration`; `currentItem` requires that exact object and generation, then resolves the current array position from `itemOfImage`; mutation status publication rechecks owner/generation/group after awaits. UX-02-ownership records tests for foreign queue/run completion and recipe mutation races. The new durable layer must rebind to the newly opened library; it must never deserialize an old object owner, generation, or integer item index.
- UX direction is consistent across `apps/mac/DESIGN.md` §12, `docs/design/workspace-redesign/ux-scope.md`, `tools/orchestrate/wp/UX-02a/{IMPLEMENTATION.md,RESULTS.md}`, `tools/orchestrate/wp/UX-02-ownership/RESULTS.md`, and `tools/orchestrate/audits/ux/REPORT.md`: Review is a navigable destination; keep current ordering/status/actions; add durable run recovery/conflict reporting as a separate slice. The audit’s broader run history, cross-image sync, accepted-only export, and run-scoped rollback are follow-on proposals, not current acceptance.

## Approved minimum contract

Persist one **latest resumable Review queue snapshot per library**, preserving the existing same-provider queue merge and explicit redo merge; a new run with no compatible queue starts a replacement snapshot. This provides “resume this queue after relaunch” without silently expanding into a full run-history browser or changing the editing engine.

The record needs:

- `schemaVersion`, a monotonic `recordRevision`, a durable `queueID`, and `updatedAt`.
- A library key derived from the canonical folder URL and duplicated canonical path for collision/mismatch checks. This record is local to the selected Tessera app-support directory (including `--app-dir`); it does not promise cross-machine sync or transparent folder relocation.
- The captured UI scope (`selection`, `view`, or `shoot`), a concise source/filter description for display, and the exact ordered target `ImageId` list captured at Start. Resume never reevaluates the old selection/filter against today's Library.
- Run facts that cannot be reconstructed from each photo: provider display name and model (never API keys, auth headers, request/response bodies, or Keychain values), start/update timestamps, and terminal run state (`running`, `completed`, `partial`, `cancelled`, `interrupted`, or `failed`). The persisted failure detail is a generic per-target message.
- Per target: stable `ImageId`, captured filename for a readable unavailable row, captured target ordinal, prior/latest agent group ID where known, and a per-target terminal outcome when known. Preserve a safe error for failed targets even when no recipe exists. Do not persist the entire current `AgentReviewEntry` as a second independent truth unless a field cannot be reconstructed.
- Review cursor `selectedImageID` and list `anchorImageID`. Do not persist the redo draft/instruction; it may contain sensitive free-form user text and UX-02a promises only session-local draft survival.

Reconstruct detail/status from the current recipe when possible. On opening the same library, read the durable target set, resolve each ID through that newly opened `EngineLibrary`, and query its current `agent_provenance`. A row is actionable only when the current photo’s latest agent group matches the record’s expected group. Read `accepted`/`reverted` from the recipe’s `review_status`, not a stale cached copy. Keep errors/missing/superseded rows visible with actions disabled and a precise reason. Never fall back to matching by filename or a former item index.

On every restore, create a **new runtime `queueGeneration`**, bind the queue to the exact currently opened `EngineLibrary`, and construct new `ReviewTarget`s from that owner. Retain the existing same-process post-await checks. A stale write/callback must not replace a newer record revision or publish into a different library. If a group was replaced by a later run, retain the old queue row as superseded/unavailable rather than accepting or reverting a different agent group.

Persist an intent record before starting an agent run, then atomically advance it as run/target outcomes become known. If the process exits while the record says `running`, restore it as `interrupted`; reconcile what can be proven from current per-photo recipe groups and keep unproven/unstarted targets explicitly unknown. Never automatically replay AI/network work after launch. Explicit Retry starts a new run from an explicit fresh target capture.

Store the record under a dedicated app-support `ReviewRuns` namespace keyed by a hash of the canonical folder path, with the full path in the record to detect collisions. Use atomic file replacement and serialized/revision-checked updates; surface read/write failures in Review without blocking the Library. Keep unsupported future schemas read-only and preserve the bytes; malformed files should produce a recoverable “Review history unavailable” state, not silently reset or overwrite them. Keep per-image recipe status authoritative; the manifest stores queue membership and run facts.

This storage location intentionally defines a local relaunch contract. Moving a library to another path or another machine does not automatically find its app-support record; missing IDs remain unavailable rather than attaching to a same-named photo. If portable/movable queue history becomes a requirement, select a library-owned manifest and migration/identity policy in a separate reviewed scope.

## Identity caveat to resolve before implementation

The engine API contract (`crates/engine-api/src/id.rs`, `CONTRACTS.md`) describes `ImageId` as sidecar-stored and surviving moves/renames. The current index implementation (`crates/index/src/lib.rs::stable_id`) derives the indexed 128-bit ID from the file path, and `crates/tessera-ffi/src/catalog.rs::document` rejects a recipe sidecar whose `image_id` differs from the current indexed ID. Therefore this plan treats `ImageId` as stable only while the same catalog/path identity resolves. It does **not** promise folder relocation or filename fallback. Before claiming move/rename restoration, reconcile the documented and implemented image-identity contracts with the engine owner.

## Implementation boundary and sequence

1. Add pure serializable `ReviewRunRecord`/`ReviewTargetRecord` values and a tested app-support store. Keep persistence out of `View.body`; all writes occur at explicit run and mutation boundaries.
2. At Start, capture scope and exact stable IDs and write the `running` intent before dispatch. Add only the minimum run ID/target checkpoint callback needed to distinguish completed, failed, interrupted and not-started targets; do not store provider secrets or free-form prompts.
3. After `run_agent` returns, persist its summary and target group IDs/errors, then continue existing `didFinish` queue behavior. For Redo, persist the updated per-photo group linkage and merge exactly as today. The recipe remains authoritative for Accept/Revert status; keep failures retryable and show their current-session detail.
4. On `EngineLibrary` installation, look up only that library’s record, rehydrate current recipe detail, classify missing/foreign/superseded entries, relink to current indices, and assign a fresh runtime generation/owner. Keep Library as the active workspace until the user chooses Review. Restore selected/anchor IDs only when present; otherwise choose the existing deterministic first-entry fallback with a visible status.
5. Wire safe error and persistence status into the existing Review destination. No changes to `Document/**`, engine image editing algorithms, global theme, Library selection semantics, or Auto Edit scope behavior.

The coordinator approved the defaults above before implementation: latest queue, local app support, and no automatic replay.

## Deterministic test scenarios

Use the existing scripted planner, temporary JPEG folders, temp app-support directories, and the current real-engine ownership test fixture patterns. No network provider, GPU, full UI launch or live user data is needed.

1. **Same-library relaunch:** capture a two-photo `Selection` run; complete it; create a new `AppModel` and newly scanned `EngineLibrary` from the same folder/app-support; restore exact ordered IDs, provider/scope/count, confidence order, status/detail, selected and anchor IDs. Entering Review is explicit; Library view/source/selection remain unchanged.
2. **Status round trip:** Accept one row and Revert another; reconstruct through fresh engine/library instances; recipe `review_status` wins, counts match, stable queue order does not jump, and action targets use the newly opened owner.
3. **Index reordering:** insert/remove or update catalog rows so dense `itemID`s change; restore by `imageID`, resolve to current positions, and operate on the intended photos.
4. **No scope expansion:** alter current selection/filter after the run or open a different library; restore never adds today's visible/selected rows to the frozen target set. A B record cannot adopt A's queue.
5. **Failure without recipe:** a failed target retains a generic safe reason without an invented `groupID` or recipe status. If its photo still resolves, explicit Redo remains available; group actions stay disabled.
6. **Interrupted run:** persist `running`, simulate a target whose recipe is still `in_progress`, and reopen. The queue is explicitly `interrupted`; proven recipe results may be shown, unknown/not-started targets remain unknown, and no planner is invoked on relaunch.
7. **Superseded group:** after snapshot, create a newer agent group for one image. Restore keeps the old row visible but disabled with a superseded reason; `accept`, `revert`, `redo`, selection advance and completion callbacks cannot target the new group through the old record.
8. **Missing/renamed path:** remove or relocate a target. Restore shows the captured name plus unavailable status; duplicate filename in another folder never resolves it. Record the current path-derived-ID limitation rather than claiming moved-folder support.
9. **Corrupt/unknown schema and I/O failure:** malformed JSON and a future `schemaVersion` do not crash/block folder open or get silently overwritten. Read/write failure is surfaced; the in-memory queue is not labeled durable. A stale `recordRevision` update cannot replace a newer writer’s record.
10. **Run merge/replacement:** preserve current completion behavior: a compatible same-provider nonempty queue and an explicit redo merge targeted photos, preserve unaffected status and row identity, then use the existing ordering; a run without a compatible queue replaces the snapshot.

## Separate UX04 follow-up: RAW overlay interactions

The Loupe overlay source/gate slice is on main as `b18ab0bb`. Its isolated GUI check used only generated JPEGs; it cannot establish RAW-specific control/pointer behavior. Keep these as a separate UX04 acceptance, not UX-02b work:

- On a ready RAW with Crop, a mask component tool, and a Develop picker available, verify clicks/drags outside the overlay’s actual controls pass through to the intended canvas/tool while overlay text itself stays click-through. Verify the overlay controls still open/dismiss normally and do not steal pointer gestures.
- Exercise soft-proof UI in ready LUT, no LUT/profile unavailable, loading, and failure states; labels must describe status truthfully and never show “on” before a LUT is applied.
- Repeat at minimum supported width with long RAW filename and AX full-name disclosure. Verify Escape closes each disclosure and the next Escape follows the underlying tool/workspace route. Keep keyboard and pointer observations separate.


## Implemented behavior and validation

The implementation adds `ReviewResumeStore` and reconnects it at run start, run completion/failure, cursor changes, and library installation. Reopened queues resolve stable image IDs through the newly installed library, mint a fresh runtime generation, verify each saved group against current recipe provenance, and leave missing, unknown, or superseded targets visible but unavailable. A live same-path run is not treated as interrupted when the application installs a replacement library object; its completion is persisted and then rebound to the current object. Cursor updates made during a run are merged into the latest matching queue revision on both success and failure. Failed rows remain explicit redo targets; group actions require a currently matching saved group.

The store has a side-effect-free `load` and an explicit `restore` that marks an unfinished run interrupted only during new-session restoration. It validates schema, library identity, target IDs/ordinals, and monotonic revisions, preserves unknown/corrupt records on read failure, and uses atomic file replacement. An impossible `Int.max` target ordinal is safely reindexed before adding targets. Provider failures are persisted only as a generic safe message; prompts, keys, and request/response bodies are not stored.

Focused validation selected 47 store, review queue, navigation, ownership, and assist tests; all passed with process exit 0. See `EVIDENCE.md` for the exact command and preserved earlier RED/failure transcripts. The gate does not establish GUI relaunch behavior or full application acceptance.
