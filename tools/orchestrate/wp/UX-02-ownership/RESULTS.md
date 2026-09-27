# Review ownership safety prerequisite

Status: focused release verification passed; full Swift suite remains pending the coordinator’s combined INT45 + current-main integration gate. No main merge or push from this worktree.

## Behavior

Retained review rows now capture the owning EngineLibrary, stable photo identity and queue generation. Accept, Revert, Redo and Show reject foreign or stale targets. A captured run may finish in its original library after a folder switch, but its completion cannot refresh positional items, publish status/toasts or request review presentation in the new library. Within-owner queue ordering and merge/history behavior are preserved. Runs wait for all review mutations to finish, so a different photo’s run cannot invalidate an in-flight review completion. Successful current-owner Accept refreshes inspector provenance even when the retained queue belongs elsewhere.

Owner-scoped AppModel barriers capture both already-closing and pending-opening Develop sessions before any action suspends. Review actions recheck owner/generation after their barrier; captured runs retain their original target. A cancelled matching open clears its loading state; an unrelated new library’s open is preserved. DevelopController close callers await one actual backend close. Coalesced settings and mask patches flush before the controller becomes closed, fixing the previous closed-guard ordering that discarded them.

The AppModel dependency was authored separately by the UI owner: original commits `4b52933`, `35f8590`, `6fd51b2`, cherry-picked as `ba58dae`, `5863519`, `3686813`. This branch includes UX-01 main base `4925677`; the Agent/Core changes were kept separate from navigation work.

## Evidence

- `swift-red.log`: baseline at `1995f75`, before production edits, compiled successfully and ran four new real-engine tests. All four failed on six intended assertions: wrong-folder style learning, old recipe/review mutation, foreign status publication, and foreign refresh/review presentation. No unexpected or setup failures. The test file was subsequently expanded and adapted to the explicit target API.
- `swift-focused-green.log`: release compile succeeded without a repair cycle, then 18 ownership/save-ordering tests plus all 3 existing AgentReviewQueue tests passed (21 total, zero failures, 9.955 seconds). Source bytes are frozen in `focused-source.json`.
- The tests use actual tiny JPEGs and two indexed folders sharing a temporary catalog, with the deterministic scripted planner. Save tests assert persisted settings, masks and agent provenance. A test-only wrapper gates the real session close so concurrent-close waiting does not depend on storage speed. Immediate pending-open tests also cover cancellation and preserving an unrelated owner’s loading state.
- `git diff --check` passed. `commands.json` records exact invocations. No Rust regeneration was performed; `ffi-cache-provenance.json` records the source-compatible archive and generated binding hashes.

Existing warnings remain in untouched Document code/tests (Sendable captures, switch-pattern `where`, unused selftest variable) and the reused archive’s blake3 object deployment target (26.5 versus linked 15.0). No new ownership/Core-close warning appeared.

## Limits

This is an ownership/save-order prerequisite, not durable Review navigation or persistence. Owner matching deliberately uses the exact EngineLibrary instance: reopening a folder does not automatically reattach an older retained queue. Accept’s completion callback returns true only for an engine success whose owner/generation is still current; navigation integration can use that result without inferring success from a status string. Full-suite and interactive Review destination acceptance remain with the coordinator’s combined gate and UX-02a.
