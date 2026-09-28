# Develop exclusive admission: tests-first checkpoint

Status: implementation/source-review checkpoint in progress on `codex/develop-exclusive-admission`, based on main `f627c4a071b7e4a35c2a114917cb726ab793d458` plus separately integrated Swift admission tests/docs from `049bfe95`. Tests-only checkpoint `d61512ed` produced the two intended behavioral REDs and the foreign-disk control passed; the current production diff has not been compiled or run and must receive root/Astra source review before GREEN runtime. The earlier histogram branch and BetterSSD target symlink remain preserved.

## Current REDs and controls

The focused tests in `crates/tessera-ffi/src/develop.rs` now encode these behaviors:

- `second_develop_editor_cannot_replace_first_edit` becomes the cross-Engine same-destination admission test. It expects the second `open_develop_session` to return the pinned current-API diagnostic `conflict: Develop destination already has an active editor`, and verifies no writer was constructed on the second Engine. After the first close, it verifies reopen succeeds and sees the first editor's saved settings.
- `direct_recipe_replacement_conflicts_without_publishing_during_develop` submits a validated changed Recipe to `set_recipe_json` while the session is active. It pins the same explicit `BridgeError::Failure` diagnostic and compares recipe/XMP bytes plus indexed recipe hash before and after, so an error returned after partial publication is also caught.
- The stale-owner defense test now models a nonparticipating/foreign disk writer by directly writing the recipe and XMP after editor open. It remains separate from Engine admission and proves the saved OwnerBaseline still blocks publication and preserves foreign bytes.
- Existing selection-during-Develop and active-editor saved-recipe histogram tests remain controls. Selection remains an allowed orthogonal Engine write; histogram must remain lease-free. Existing legacy RGB first-save, nested-owner unknown-member fail-closed, Stage A repair, and Stage B close-recovery tests remain in place. Unknown nested owner members fail closed; this plan makes no preservation claim.

Observed current-main REDs: second editor open succeeded instead of conflicting (direct process exit 101), and `set_recipe_json` succeeded instead of rejecting (direct process exit 101). The foreign-disk OwnerBaseline control passed (direct exit 0). Raw logs and source freezes are preserved under `tools/orchestrate/wp/UX-03/evidence/develop-exclusive-admission-tests-2026-09-28/`.

## Commands for the next allocated native lane

Run one test filter per preserved attempt, serialized, using the verified external target and two jobs:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated cargo test -p tessera-ffi --lib develop::tests::second_develop_editor_cannot_replace_first_edit -- --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated cargo test -p tessera-ffi --lib develop::tests::direct_recipe_replacement_conflicts_without_publishing_during_develop -- --nocapture --test-threads=1
```

The RED source freeze should include `develop.rs`, `lib.rs`, `recipe_write.rs`, `Cargo.toml`, and `Cargo.lock` hashes, toolchain/environment, command, full output, and direct process exit. Verify each run fails behaviorally at its admission assertion; a harness/compile failure is not a RED. Keep both attempts even if either needs a harness-only correction.

## Required implementation contract after RED review

Implement the lease as an internal capability, not a new UniFFI variant. Public `BridgeError` currently exposes only `Failure { message }` (`crates/tessera-ffi/src/lib.rs:57-63`); use the pinned explicit admission diagnostic unless source review identifies a strong reason to alter the API. Introduce a monotonically allocated owner ID and active owner on the existing destination gate (`crates/tessera-ffi/src/recipe_write.rs:19-60,62-140`). Hold the gate only for atomic admission/validation plus the existing serialized I/O critical section, never across image decode, render, flush, worker join, or waits. Keep the matching lease ID check atomic with Develop writes. `set_selection` remains permitted while leased (`lib.rs:407-426`); `set_recipe_json` must reject before writing (`lib.rs:438-480`).

Acquire the lease in the same gate → catalog/path revalidation critical section as the initial disk snapshot and pre-normalization OwnerBaseline (`develop.rs:1078-1101`). Release the short gate before decode. Avoid rollback self-deadlock: do not drop a lease token that reacquires the gate while still holding that gate guard; construct the returned RAII token only after snapshot success, or clear tentative ownership directly under the held guard on failure. Test both malformed effective recipe failure while the gate is held and a post-snapshot RawImage decode failure, then prove immediate reopen succeeds. Develop save and auxiliary repair must authenticate the *captured* GateState Arc and owner ID, re-run `check_image`/destination-key validation, and reject a stale token; do not reacquire an arbitrary fresh gate using only `gate_for(path)` (`develop.rs:1134-1222`, `recipe_write.rs:120-132`). Keep the current OwnerBaseline comparison, latest-selection/top-level-unknown merge, and Stage A retry behavior. Nested unknown owner members are not retained: the baseline marks them unsupported and publication fails closed (`develop.rs:1241-1295,1153-1161`).

**Worker-final-Arc hazard:** the lease must survive save-worker self-drop. `DevelopSession::drop` calls `stop_writer`; when the final session Arc is dropped on its own save-listener thread, `stop_writer` marks shutdown but deliberately detaches instead of self-joining (`develop.rs:2240-2262`). The writer loop can still finish a due save before exiting (`develop.rs:1640-1688`). The current source checkpoint gives `DevelopSession` and the writer closure separate owning lease references; `Shared` stores only a non-owning captured `(GateState, owner ID)` authority, so render-held Shared Arcs cannot prolong the reservation. Normal close flushes, joins the writer, closes rendering, then takes/drops the session lease even if a closed session Arc remains retained. On writer-thread final Drop, the session reference drops while the worker reference remains through actual loop exit. The worker-exit observer used by the test fires only after that worker-owned reference is dropped. Failed close retains the same reservation for recovery.

## Remaining test-first lifecycle gates

The current unrun source checkpoint adds these focused controls; they still require reviewed execution before claiming Stage C ready:

1. Selection succeeds during the lease and a later Develop flush preserves that selection (`develop.rs:3748-3771`).
2. Read-only histogram succeeds during an editor lease without acquiring/releasing ownership; then a second open still conflicts (`develop.rs:5474+`).
3. Failed open after reservation at two boundaries: malformed effective recipe causes snapshot failure while the gate is held; indexed tiny JPEG removed after successful snapshot causes RawImage decode failure after gate release. Restore each fixture and assert immediate fresh open succeeds, covering rollback deadlock and token cleanup separately.
4. Failed close retains ownership; successful repair/retry drains the worker, releases ownership, and allows reopen (`develop.rs:3991-4049`).
5. Deterministically pause a close from the saved-listener callback after `save_develop` has returned and released the gate. The second open remains rejected while close is waiting; only after close and join can it reopen. This avoids waiting for a contender while holding the destination gate.
6. The callback-final-Arc Drop case holds the last `DevelopSession` strong Arc in a listener-owned test slot, drops it on the save worker, and pauses after `Drop`. A competing open must still conflict while the worker is paused; after a deterministic worker-loop-exit observation, a fresh open succeeds. The gate is free during the callback. The test uses bounded channels, not sleeps.
7. Keep foreign owner-field protection by directly editing sidecar bytes after open. With the Engine setter blocked during the lease, it is the deterministic stand-in for writers outside this in-process gate; assert exact foreign bytes remain after failed flush. Nested unrepresented owner members continue to fail closed.
8. Preserve Stage A post-recipe repair and Stage B failed-close/retry controls; rerun legacy RGB, owner conflict, selection, histogram, recipe gate, and close/drain tests before strict format/lint gates.

The destination key still has a separate unresolved case-variant alias question: on case-insensitive macOS volumes, distinct lexical recipe filenames such as `photo.json` and `PHOTO.json` may resolve to one filesystem destination while the current `destination_key` retains different spellings. The current implementation intentionally preserves that pre-existing key behavior pending root's scope/design decision; no case folding or new path canonicalization is claimed here. A case-variant cross-Engine admission control remains a prerequisite to GREEN if the alias is confirmed in scope.

Current UI recovery/open code handles thrown opener errors with current-generation/library/focus guards and settles the pending-open token (`apps/mac/Sources/Tessera/App/AppModel.swift:2133-2187`). The separate tests-only UI checkpoint `8cc6bc18068f54843d4445e2d0549909623e51aa` covers visible admission rejection, ticket cleanup, successful re-entry, and stale rejection; it is integrated and its focused plus adjacent tests passed. Those controls are not native lease acceptance.

Scope limits: this lease serializes only participating in-process Engine writers. Agent/Cull/import/merge writers and external processes still bypass the gate; no filesystem CAS, batch API, multi-file transaction, or unsupported nested owner-field preservation is claimed. The current merge preserves latest top-level unknown document fields and selection; unsupported nested Develop owner fields fail closed. Main integration remains root-owned.
