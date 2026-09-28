# B5-16 current-base inspector source review

Candidate `ddb2810196d5492c9258840af9a431e706d9318f`, source374ea0ede8ef9fc8ca0206e8d33dbefafefffcdf, pinned base1e45baaead70b1bcd208afba800bcacd92213ee8. Source-only review; no builds, tests, app/window actions, checkout edits, or historical acceptance promotion. Applied SwiftUI Expert review guidance to state/layout/API use. All B authored tests remain UNRUN on this combination.

## Verdict

No blocking product source defect found in this bounded reconciliation. Suitable for clean integration candidate and fresh compiler/focused/full gates, preserving the handoff's explicit interaction limits. The restored app runner is not ready for blind/default execution; two concrete execution caveats below must be addressed or bypassed with A's isolated procedure before using it as an acceptance harness.

## Exact scope and preservation

Independently hashed all16 files from CURRENT-MAIN-INSPECTOR-SOURCE.sha256; every digest matches Git candidate bytes. The exhaustive base-to-candidate diff contains exactly those16 source/test/harness files plus the handoff and manifest. Machine-readable hashes: /tmp/tessera-b516-ddb28101-source-review.json.

No changes to Rust/Cargo, generated Swift/header, checked-save backend/commit implementations, DocumentSavePresenter/DocumentSheets, AppModel, Smart Preview APIs/routing, or resource cancellation/cache/viewport/outline implementations. DocumentWorkspace diff is only eight inspector-tab preference lines; current checked collision result, destination intent, async admission/save/close/status logic remains intact. AdjustmentEditors retains the exact current synchronous inline MainActor Sendable checkbox setter. ToolsPalette/DocumentView preserve existing Transform tool attachment/options call sites rather than overwriting from the old full branch. B5-16a models are unchanged.

## Product reasoning

- Inspector tab choice belongs to observable DocumentWorkspace and persists through its existing lifetime; invalid stored values default to Stack. History requested height is persisted separately and clamped only at layout/drag, preserving requested height during ordinary window resizing. Budget reserves header/separator/tab minimum and limits History allocation. History states/snapshots share a scroller, while New Snapshot/memory and Layers/Channels footers remain outside their list scrollers. Actual SwiftUI measurement still needs the restored layout matrix; pure arithmetic is not proof of geometry.
- Document tabs cap visible count, always include selected index and preserve order; overflow selects through existing workspace admission. Width and long-title behavior require fresh geometry/interaction gates.
- Auto UI now edits independent persisted percent clip fields, and mode/reanalysis preserves those values; fresh Auto creation uses the model's explicit0.1percent default. LUT filename/dither travel through model fields, loading captures basename, reset retains dither, and cancelled file picker makes no set call. Existing sampling/analysis mechanisms remain unchanged.
- Neutralize helper reanalyses only disabling the legacy zero-chroma/no-flag representation; modern flag changes retain frozen statistics and do not evaluate target autoclosure. Missing legacy source returns nil with explicit unchanged-setting explanation. Three restored regressions exercise real native save/reopen/undo, missing-source refusal, and modern no-target-read. No analysis-on-load is introduced.

## Tests and harness

Five pure layout tests and three inspector-specific shell cases are present. Whole-shell matrix retains current dynamic case count/Masks checks and strengthens Document overflow from expected failure to ordinary failure. The populated per-tab matrix checks region ordering/minima/footer bounds and presence, plus compact eight-document strip and window key equivalents. It does not prove drag/reset/relaunch/VoiceOver or focus-specific shortcut behavior; those remain honestly UNRUN in the handoff. History requested-height isolation is not explicitly varied in this matrix, so the interaction matrix must still exercise persisted large heights and actual dragging.

Seven Python regressions cover exact completion, hidden prerequisite failures, malformed/missing summaries, timeout/exit handling, root routing and direct-child-only termination. No Python tests were run by this reviewer. The log validator correctly rejects the original false-success conditions. Broad app suite limitations:

1. selftest_runner.py TESTS['transform'] still passes --new-document, the exact setup previously diagnosed as racing the intended Transform fixture (B takeover72d8756). The test only asserts Transform inclusion, not correct setup. Before running Transform through this runner, remove/adapt that argument and add a regression pinning its fixture arguments; a tiny isolated procedure is an acceptable alternative. The handoff already warns broader selftest setup was not transplanted; this must not be lost during integration.
2. Restored runner lacks the later explicit resource-hold admission check and accepts a default nine-test heavyweight run. --nonactivating is only a request; app self-tests may raise windows. Do not invoke run-selftests.sh/default suite while the hold is in force or treat it as desktop-isolated. Root explicitly excluded broad active-window harness runs. Keep it out of runtime gates until A supplies bounded admission/isolation, or test only the pure/Popen-fake Python regressions.

History resize handle remains a pointer DragGesture/double-click target with identifier/help, not an explicit accessibility adjustable action. No keyboard/VoiceOver resize acceptance follows; evaluate that required interaction separately and add an accessible action if resizing is otherwise undiscoverable. This is an outstanding acceptance question rather than an observed runtime defect.

## Required next gates

Use exact candidate artifacts and fresh optimized strict compiler, focused inspector/analysis/JSON tests, adjacent checked-save/resource/Transform-key tests, pure runner regressions, then full Swift. Do not carry base703 or old424 success forward as candidate acceptance. Preserve failures rather than restoring the overflow expected failure. Actual bounded small-window History/scroll/footer/tab/shortcut/VoiceOver and checked-save compatibility require isolated GUI work. No heavy full app self-test, performance, Transform-completion or complete B5-16 acceptance is granted by this source review.
