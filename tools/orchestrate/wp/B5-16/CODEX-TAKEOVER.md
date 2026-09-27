# Machine B coordinator recovery — 2026-09-27

Codex recovered the local Claude coordinator and verifier transcripts at the user's request.
Machine A remains the sole main-branch integrator. This laptop is Machine B.

## Recovered queue

| Package | Local branch/head at recovery | State |
| --- | --- | --- |
| B5-16a | wp/B5-16a bcd0e79 | Already pushed. Small M5-32 Swift adjustment round-trip fix; not in origin/main 4a4bd71. Prior Claude gate log reports 385 tests, zero failures. |
| B5-16 | wp/B5-16 3f81f55 | Committed tabbed inspector and persisted adjustment editors. Prior complete gate recovered. Codex merged origin/main 4a4bd71; fresh Swift/FFI, fixture and Xcode gates pass (see below). Contains the equivalent of B5-16a's model fix. |
| B5-13 | wp/B5-13 afc0258 + dirty edits | Liquify and Content-Aware Move/Extend. Three modified Swift files and untracked evidence. Last inherited self-test finished at takeover with zero reported failures. Full current integration/gate and independent UI verification remain. Upstream incorrectly points to origin/wp/B5-10c; do not push implicitly. |
| B5-12b | wp/B5-12b 7af2eff + dirty edits | Transform fixes and native-stack workaround removal. Inherited self-test logs Warp did not start after step 395, then reports zero failures. This is an incomplete run, not acceptance. Fix failure accounting and investigate early exit. |
| B5-15 | wp/B5-15 73e8ae0 | Export/filter performance changes committed. Inherited measurement process still running at recovery; styled 14 MP export 0 timed out at 900.14 seconds and recorded FAIL. Investigate before handoff. |
| B5-harness | wp/B5-harness 14f60ef | Published computer-use harness; preserve. |
| verifier | detached 108ed76 | B5-12 report plus six untracked screenshots preserved. Historical build predates M2-56. Drag-dependent checks remain blocked/unrun, not passes. |

## Coordination

Recovered Machine A peer: Claude bridge session `session_01Qow9JxrrmvDwUexto9SQsf`, titled
“Multi-model execution plan (fork)”. The old coordinator used Claude SendMessage. That bridge is
not callable from this Codex session. The user has now started Machine A in Codex as “resume multi model execution plan”.
Codex lists only the local host here; the other laptop chat is not discoverable. Direct receipt
of this handoff by Machine A is unconfirmed.
Use explicit wp/B5-* pushes and READY entries; only Machine A merges main.

Pending cross-machine requests from the recovered transcript:
- Merge B5-16a to restore the adjustment JSON tests; B5-16 follows.
- Rerun the B5-14 document performance bench after M2-57 scheduling changes.
- Machine A's verifier handles the uncovered-canvas drag checks for B5-11, B5-12 and B5-13.
- B5-15 owns investigation of the smart-filter bake race and 20-run stress verification.

## Preservation and UI boundaries

Recovery copies of tracked diffs and untracked files for B5-12b, B5-13 and verify:
`/Users/rutmehta/.cache/tessera-recovery/2026-09-27-codex-takeover/`.
Original worktrees and evidence remain intact. Build artifacts and Claude scratch logs have not
been deleted. Preserve active inherited measurement processes until their results are collected.

Keep background verification nonactivating. Do not raise, uncover, resize, zoom or fullscreen
windows through computer use. Covered-canvas drag checks stay blocked for Machine A's verifier.
Do not count a self-test done line as complete if prerequisite failures skipped later steps.

## Git handoff protocol for the resumed coordinators

Machine A: fetch `origin/wp/B5-16` and read this file with `git show`; do not merge solely
because the branch is published. B5-16 now has a current READY entry for coordinator review, with the validation limits below. Respond on your coordinator branch with a handoff note, or commit a
Machine A coordination note on main. Keep each machine writing its own note and package
branches. Machine B will fetch and inspect coordination changes before allocating new work.

Fresh B5-16 integration head before this note: `91b3fe6`, incorporating main `4a4bd71`.
The inherited complete gate was on `3f81f55`; the fresh Swift/FFI gate passed on the same code tree as `91b3fe6`.
Its log on Machine B is `/tmp/tessera-codex-b516-swift-gate.log`.

## Fresh Codex validation — 2026-09-27

Code tree: `91b3fe6` (main `4a4bd71` integrated); subsequent changes are handoff documentation only.

- `tools/orchestrate/swift-gate.sh`: FFI generation and Swift build passed; 405 XCTest tests,
  one skipped, zero failures; all five Swift Testing tests passed. `SWIFT GATE OK`.
- `cargo test --locked --release -p tessera-ffi --test adjustment_json`: 3 passed, zero failures.
- `cargo fmt --all -- --check` and `git diff --check`: passed.
- `xcodebuild -scheme Tessera -configuration Debug -destination 'platform=macOS'
  -derivedDataPath "$HOME/.cache/tessera-derived-data-B5-16" -jobs 2 build`: BUILD SUCCEEDED.

Validation limits: Codex did not rerun the full Rust/clippy suites or all app self-tests in this
recovery pass. The earlier complete Claude gate and on-screen/self-test evidence remain historical
at their documented commits. No new independent computer-use acceptance pass was performed.
Machine A still owns integration review and main merge; no merge or peer receipt is claimed.

Next Machine B work: investigate B5-15 styled-export timeout; correct B5-12b's false-success
prerequisite accounting and early exit; integrate and gate B5-13. Rerun the document performance
bench after M2-57 as previously requested by Machine A.
