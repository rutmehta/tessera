# B5-24 handoff: vector self-test finds the Properties controls again

Scope: self-test only. The one changed source file is `apps/mac/Sources/Tessera/Document/Vector/VectorSelfTest.swift`. No product code changed.

## Root cause
`verifyFixes` (the B5-11b items) looks up `document.shape.stroke.dashOffset` (a `ValueSlider`) and
`document.shape.fill.color` (an `NSColorWell`) in the viewport's own window, and that window lookup was already correct
(it is the SelfTestHost window). The controls were missing because B5-16 (ab293123) added the tabbed inspector:
`ShapeInspector` is now built only while `DocumentWorkspace.inspectorTab == .properties`. The tab defaults to `.stack`
(it persists in `UserDefaults` under `DocumentInspector.tab` and is unset on this machine), and the self-test never
switched tabs. Background hosting and key or main window state played no part. The identifiers are unchanged in
`ShapeViews.swift`, so this is not a product bug.

## Fix
- `show(_ tab:)` sets `ws.inspectorTab`, waits 0.3 s and lays out the content view.
- The test shows Properties before the dash-offset slider (items 3 and 7) and before the fill well (item 6), and Stack
  before the Layers outline and row lookups (items 7 Z and 5).
- A `defer` puts back the workspace tab and the stored `DocumentInspector.tab` default (removed if it was unset).

## Verify
- `tools/orchestrate/wp/B5-selftest-window/run-background-selftest.sh vector` (open -g -n, --nonactivating; the front
  app did not change) printed `vector-selftest: done, 1 failure(s)`.
  - Every 11b check passes. Four of them the failed lookups had been skipping: 11b-3 one node and offset moved,
    11b-7 U and ⇧U, and 11b-6 revert.
  - The one remaining failure is `fill-only drag previews coalesce to frames FAIL 1` (step 372). B5-22 (unmerged) fixes
    it, and it failed the same way on the unchanged baseline, which printed `done, 3 failure(s)`.
  - `DocumentInspector.tab` is still unset after the run.
- `tools/orchestrate/swift-gate.sh` printed SWIFT GATE OK (837 tests, 0 failures).
