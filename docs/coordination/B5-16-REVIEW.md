# B5-16 independent integration review

Reviewed 2026-09-27, read-only, against main `c0d4535` and `origin/wp/B5-16` `3bb9117` (code tree from `91b3fe6`). No checkout, source edits, builds, UI interaction, merge, or push. Tracked working tree remains clean. Read Machine A coordination, B takeover, brief, implementation status, READY, source and tests. Applied the SwiftUI review guidance to state/layout/sheet changes.

## Recommendation

Hold the merge for the Match Color compatibility fix below and mechanical conflict resolution. The layout implementation is a substantial improvement, and the fresh 405-XCTest gate provides useful coverage; it is not full UI acceptance. After the fix and a fresh resolved-tree Swift gate, the code can be reconsidered separately from the outstanding interactive acceptance. Do not mark B5-16 fully accepted until the required app/UI checks are recorded.

## Findings

### P2 — Legacy Match Color Neutralize can no longer be turned off

**Changed site:** `apps/mac/Sources/Tessera/Document/AdjustmentEditors.swift:318–322` on B5-16; also the removed compatibility accessor at `apps/mac/Sources/TesseraCore/Document/DocumentAdjustmentModels.swift:203` on main.

B5-16 binds the checkbox directly to `m.neutralize` and its action only writes that flag. Pre-M5-32 Tessera encoded an enabled Neutralize by storing `sourceMean = [L, 0, 0]`, without a flag (`a6e6361^:.../AdjustmentAnalysis.swift:147–156`). Those documents now decode `neutralize` as false. Thus a previously neutralized adjustment opens with its checkbox off. Toggling on and off cannot restore the source's chroma: the renderer adds `source_mean` and, when neutralized, subtracts that same mean (`crates/compositor/src/adjust.rs:797–803`); subtracting zero changes nothing. This is an editing regression even though the frozen image initially renders identically.

Current main deliberately retains `MatchColorModel.neutralized` (legacy-zeroed-stat detection) and the old editor reanalyzes when the user disables Neutralize, recovering the source mean when the source layer exists. B5-16 removes both behaviors. The existing tests cover newly analyzed nonzero statistics and new flags, not this legacy representation.

**Fix:** preserve a compatibility path for the old zeroed-stat representation, including correct checkbox state and reanalysis when disabling it with an available source, while retaining direct persisted-flag toggling for new models. Merely retaining the unused computed accessor does not fix the editor. Add a regression case using a warm existing source and a legacy saved Match Color object; verify reopening indicates enabled Neutralize, disabling restores nonzero source chroma, and undo/reopen behave correctly. Avoid silently reanalyzing on load because frozen rendering must remain unchanged.

### P2 — The new self-test runner returns success even after a timeout/failure

**Changed site:** `tools/orchestrate/wp/B5-16/run-selftests.sh:39–47,50–63`.

On timeout/crash it prints `NO DONE LINE`, kills the app, and continues. `run()` ends with `grep ... | head -20`; without `pipefail`, that pipeline reports the successful `head` status whether a failure line is found or not. The outer loop aggregates no failure status. A run with `done, N failure(s)` for N > 0 likewise exits successfully. This is a verification-tool bug, not evidence that the logged historical runs failed. Do not use this script's exit status as an acceptance gate until it requires each exact completion line with zero failures and also rejects prerequisite FAIL lines and missing completion.

The runner additionally hardcodes `/Users/rutmehta/Developer/lightroom/.worktrees/B5-16` at line 8; it does not run the invoking checkout on Machine A's `/Users/rutmehta/Developer/tessera`. Resolve the worktree from the script location or an explicit parameter before reusing it for current-tree verification. Keep timeout handling and termination scoped to the process actually launched.

## B5-16a and preservation of main

No additional defect found in B5-16a's merged model/analysis/JSON changes. `DocumentAdjustments.swift`, `AdjustmentAnalysis.swift`, `StubCompositor.swift`, `DocumentAdjustmentAnalysisTests.swift`, the Rust adjustment JSON test and fixture are identical between current main and B5-16. The only remaining model-file difference is the legacy accessor removal described above. Main's already completed 397-XCTest/5-Swift-Testing and Rust adjustment_json 3/3 gates do not need duplication just for this review.

`git merge-tree --write-tree main origin/wp/B5-16` reports exactly three conflicts:

1. `AdjustmentEditors.swift`: Color Lookup switch dispatch. The branch's four-value dispatch is needed for its new four-argument editor.
2. `DocumentAdjustmentModels.swift`: main's legacy `neutralized` compatibility accessor versus branch removal. Resolve in conjunction with the actual compatibility fix, not by blindly choosing theirs.
3. `tools/orchestrate/wp/READY.md`: keep the B5-16a merged record and B5-16's truthful review/UI-pending status.

The automatic merge preserves Machine A's coordination documents and its fixture preflight in `tools/orchestrate/swift-gate.sh`. The only differences from the B branch in the merge-tree result are these three conflicted files plus those main-only documents/preflight. There are no remaining Rust production changes from B5-16. This is a normal merge candidate after review fixes; wholesale replacement of main with B's tree would lose main's checkpoint/preflight and must be avoided.

## Coverage and outstanding acceptance (not additional proven product bugs)

- New tests remove the document overflow expected-failure exception. They exercise 4 sizes × 3 inspector tabs × History open/closed, root and region containment, footer placement and Stack sliders. Three shell tests plus five pure layout tests explain the 397 → 405 XCTest increase. Background-safe tests and current Xcode success are reported on B; this review did not rerun them.
- The matrix uses existing/persisted History height and programmatic tab selection. It does not simulate History dragging, reset-by-double-click, relaunch persistence, scroll reachability, VoiceOver, or shortcut routing with each required first responder. Treat acceptance steps 423, 425, 427–429, 431–437 as requiring their actual behavioral checks, not as proven by the matrix alone.
- Particularly check the History resize handle at its min/max, double-click, resize after a persisted large height, tab switches, and reopening; selected slider edits across tab switches; eight-document overflow selection/closing at compact width; and header/footer reachability in the sheets with long errors/content.
- The status note's claim that this branch has no `--transform-selftest` is stale: `apps/mac/Sources/Tessera/Document/Transforms/TransformSelfTest.swift` is present in `3bb9117` after the main merges. The brief explicitly requires it. The runner's list omits transform and no B5-16 transform evidence is supplied. This is outstanding acceptance, not a reason to discard the otherwise useful historical eight self-tests. The known B5-12b false-success/early-exit issue means an exact done line alone is insufficient if prerequisites skip later checks.
- The B takeover/READY correctly acknowledges no fresh independent computer-use pass and no full Rust/clippy rerun. Remaining B5-16 code is Swift/UI, so I see no change-based need for another full expensive Rust suite merely for this merge; follow any mandatory project gate separately. A fresh focused JSON check is cheap only if the conflict fix touches serialization.

## Focused validation before reconsidering merge

1. Add and run a meaningful legacy Match Color edit regression, plus `DocumentAdjustmentAnalysisTests` and `DocumentAdjustmentJSONTests`.
2. Fix runner failure accounting; exercise success, failure and missing-done/timeout paths without launching expensive app work, and ensure it targets the intended checkout.
3. On the resolved integration tree, run the strict Swift gate once (including `ShellLayoutTests` / `DocumentInspectorTabsTests`) and confirm tracked FFI output remains unchanged; a build on the unresolved B head does not prove conflict resolutions.
4. Complete current app self-tests required by the brief, including transform, with process ownership, background boundaries and prerequisite accounting maintained. Run focused interactive acceptance for the uncovered behavior above; record historical evidence as historical and unavailable drag tests as pending.

Minor nonblocking hygiene: `git diff main origin/wp/B5-16 --check` reports a new blank line at EOF in `IMPLEMENTATION-STATUS.md:182`; no source whitespace errors. This does not affect the merge decision.
