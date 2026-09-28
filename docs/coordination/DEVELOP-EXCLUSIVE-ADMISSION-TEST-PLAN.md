# Develop exclusive admission: tests-first checkpoint

Status: tests-only preparation on `codex/develop-exclusive-admission`, based on integrated main `f627c4a071b7e4a35c2a114917cb726ab793d458`. The source has not been compiled or run because the native lane is reserved. No lease implementation or UniFFI/API change is included. The branch preserves the earlier histogram branch and BetterSSD target symlink in the separate checkout.

## Current REDs and controls

The focused tests in `crates/tessera-ffi/src/develop.rs` now encode these behaviors:

- `second_develop_editor_cannot_replace_first_edit` becomes the cross-Engine same-destination admission test. It expects the second `open_develop_session` to return the pinned current-API diagnostic `conflict: Develop destination already has an active editor`, and verifies no writer was constructed on the second Engine. After the first close, it verifies reopen succeeds and sees the first editor's saved settings.
- `direct_recipe_replacement_conflicts_without_publishing_during_develop` submits a validated changed Recipe to `set_recipe_json` while the session is active. It pins the same explicit `BridgeError::Failure` diagnostic and compares recipe/XMP bytes plus indexed recipe hash before and after, so an error returned after partial publication is also caught.
- The stale-owner defense test now models a nonparticipating/foreign disk writer by directly writing the recipe and XMP after editor open. It remains separate from Engine admission and proves the saved OwnerBaseline still blocks publication and preserves foreign bytes.
- Existing selection-during-Develop and active-editor saved-recipe histogram tests remain controls. Selection remains an allowed orthogonal Engine write; histogram must remain lease-free. Existing legacy RGB first-save, nested-owner unknown-member fail-closed, Stage A repair, and Stage B close-recovery tests remain in place. Unknown nested owner members fail closed; this plan makes no preservation claim.

Expected current-main REDs are second editor open succeeding and `set_recipe_json` succeeding/publishing. Those are unobserved until the lane is available. No output from a compile/test run is claimed.

## Commands for the next allocated native lane

Run one test filter per preserved attempt, serialized, using the verified external target and two jobs:

```sh
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated cargo test -p tessera-ffi --lib develop::tests::second_develop_editor_cannot_replace_first_edit -- --nocapture --test-threads=1
CARGO_BUILD_JOBS=2 CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/depth-histogram-readonly-77eb68d0-relocated cargo test -p tessera-ffi --lib develop::tests::direct_recipe_replacement_conflicts_without_publishing_during_develop -- --nocapture --test-threads=1
```

The RED source freeze should include `develop.rs`, `lib.rs`, `recipe_write.rs`, `Cargo.toml`, and `Cargo.lock` hashes, toolchain/environment, command, full output, and direct process exit. Verify each run fails behaviorally at its admission assertion; a harness/compile failure is not a RED. Keep both attempts even if either needs a harness-only correction.

## Required implementation contract after RED review

Implement the lease as an internal capability, not a new UniFFI variant. Public `BridgeError` currently exposes only `Failure { message }` (`crates/tessera-ffi/src/lib.rs:31-41`); use the pinned explicit admission diagnostic unless source review identifies a strong reason to alter the API. Introduce a monotonically allocated owner ID and active owner on the existing destination gate (`crates/tessera-ffi/src/recipe_write.rs:19-60,62-140`). Hold the gate only for atomic admission/validation plus the existing serialized I/O critical section, never across image decode, render, flush, worker join, or waits. Keep the matching lease ID check atomic with Develop writes. `set_selection` remains permitted while leased (`lib.rs:407-426`); `set_recipe_json` must reject before writing (`lib.rs:438-480`).

Acquire the lease in the same gate → catalog/path revalidation critical section as the initial disk snapshot and pre-normalization OwnerBaseline (`develop.rs:1078-1101`). Release the short gate before decode. A local RAII lease must release on every open/decode/resource/worker-spawn failure. Develop save and auxiliary repair both validate the live lease and path (`develop.rs:1134-1222`), while keeping the current baseline comparison, latest-selection/unknown merge, and Stage A retry behavior.

**Worker-final-Arc hazard:** the lease must be owned by `Shared` (or an equivalent object held by the save worker), not merely by `DevelopSession`. `DevelopSession::drop` calls `stop_writer`; when the final session Arc is dropped on its own save-listener thread, `stop_writer` marks shutdown but deliberately detaches instead of self-joining (`develop.rs:2216-2241`). A session-only lease field would be dropped before that worker exits, admitting a second editor while the old writer still runs. Keep the token inside `Shared` so the worker Arc retains it through exit. On normal successful explicit close, after flush, worker join, and rendering close, explicitly take/drop the Shared token even if the closed session Arc remains retained (`develop.rs:2991-3031`). On failed close retain it through retry/recovery. On worker-thread final Drop, allow the writer-held Shared Arc to drop the token only after the worker exits.

## Remaining test-first lifecycle gates

After the initial admission RED review and minimal implementation review, add focused controls before claiming Stage C ready:

1. Selection succeeds during the lease and a later Develop flush preserves that selection (`develop.rs:3748-3771`).
2. Read-only histogram succeeds during an editor lease without acquiring/releasing ownership; then a second open still conflicts (`develop.rs:5474+`).
3. Failed open after reservation (e.g. indexed tiny JPEG removed before RawImage open, then restored) releases the lease and allows a fresh open.
4. Failed close retains ownership; successful repair/retry drains the worker, releases ownership, and allows reopen (`develop.rs:3991-4049`).
5. Deterministically pause a close at the existing post-recipe barrier (`develop.rs:4114-4195`): second open remains rejected while close is in flight; only after close and join can it reopen. Assert stale lease identity cannot authorize publication or clear a newer owner.
6. Add the callback-final-Arc Drop case. Hold the `DevelopSession` only from a listener-owned test slot, drop its final strong Arc on the save worker, and pause inside the callback after `Drop` begins. A competing open must still conflict while the worker remains paused; after a deterministic worker-exit/release observation, a fresh open succeeds. This specifically guards the detached-self-join path; no sleep-based inference.
7. Keep foreign owner-field protection by directly editing sidecar bytes after open. With the Engine setter blocked during the lease, it is the deterministic stand-in for writers outside this in-process gate; assert exact foreign bytes remain after failed flush. Nested unrepresented owner members continue to fail closed.
8. Preserve Stage A post-recipe repair and Stage B failed-close/retry controls; rerun legacy RGB, owner conflict, selection, histogram, recipe gate, and close/drain tests before strict format/lint gates.

Current UI recovery/open code handles thrown opener errors with current-generation/library/focus guards and settles the pending-open token (`apps/mac/Sources/Tessera/App/AppModel.swift:2133-2187`). The separate tests-only UI checkpoint `8cc6bc18068f54843d4445e2d0549909623e51aa` covers visible admission rejection, ticket cleanup, successful re-entry, and stale rejection. Root/Astra review of that checkpoint is pending; it is not native runtime acceptance.

Scope limits: this lease serializes only participating in-process Engine writers. Agent/Cull/import/merge writers and external processes still bypass the gate; no filesystem CAS, batch API, multi-file transaction, or full owner-field preservation is claimed. Main integration remains root-owned.
