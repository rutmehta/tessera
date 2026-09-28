# Terminal Save As probe ownership — source-only follow-up

Request: 776e287a-0fa0-4f1c-b7b6-cdecc54e8448. Exact B target and unexpired request validated, accepted before editing.

Tests (all UNRUN): b1e62cb0, a7e0294c, a2c80ec3. Six new tests, including explicit hidden-window capture after the parent-end event. Product commit is the parent of this handoff commit; its subject is `fix: settle unmaterialized Save As claims through probe teardown`.

## Terminal ownership rule

The SwiftUI content boundary still claims the request. Each materialized NSView probe registers a UUID lease. Reuse releases the former owner before registering the new one. dismantleNSView reports any final window, clears callbacks, and releases that exact lease once. SwiftUI onDismiss seals the request against new leases. Native completion for an actually observed attachment remains supported independently.

The additional terminal route requires all three: this generation's SwiftUI dismissal, no outstanding probe leases, and the captured parent's attachedSheet being nil. Missing window capture alone remains insufficient. A claim whose probe never materialized has no leases; onDismiss plus an actually clear parent can retire it. For end-before-probe/late-capture ordering, both teardown and parent didEnd re-evaluate the terminal rule, so no second native event is required. An unrelated still-attached sheet prevents the fallback, and is never forcibly ended. Parent-loss cleanup remains available but is no longer required for normal preattachment cancellation recovery.

The internal saveSheetParentIsClear override provides deterministic attachment snapshots in tests; production reads the captured NSWindow. No public save/load API changed. Existing native observer injection is retained. Old generations, duplicate dismantles, and unknown lease IDs cannot retire a successor.

## Evidence and limits

Apple documents dismantleNSView as cleanup in anticipation of view removal, not a native sheet-detachment guarantee: https://developer.apple.com/documentation/swiftui/nsviewrepresentable/dismantlensview(_:coordinator:)-21agq
Apple describes sheet onDismiss as the callback when dismissing: https://developer.apple.com/documentation/swiftui/view/sheet(item:ondismiss:content:)
Neither API establishes the ordering formerly assumed. The implementation therefore joins explicit view ownership retirement, the request's dismissal, and a current native attachment check; it does not equate either callback alone with detachment. No timing/polling or framework-order impossibility claim.

Six regressions cover preattachment cancellation then successor, native attachment still present at teardown/dismissal, end-before-late-window-capture without a second end event, a claim with no materialized probe, multiple leases/stale-generation teardown, and the actual probe dismantle callback's exactly-once cleanup. Prior tests and evidence are preserved.

Only source review and git diff --check performed on B. Tests/compilation/actual SwiftUI reuse and teardown ordering remain A validation gates. B resource hold, existing writer, and paused heartbeat remain intact. A owns all compiler/GUI/main integration. Apply the new test/product commits on top of 2cc48419 and its tests; do not restore B's old diagnostic instrumentation already reverted on A.
