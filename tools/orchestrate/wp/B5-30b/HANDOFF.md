# B5-30b — document colour follow-ups

Base: `wp/B5-30` at `69728b71`. Swift-only follow-up; engine pixels and Rust are unchanged.

## Behaviour

- The Channels component overlay, RGB/component channel thumbnails and layer thumbnails carry the document's `DocumentDisplayColor.space`, just like the detail preview panes. The async layer thumbnail loader carries the space into image creation; pixel bytes are not converted. Alpha/spot channel masks remain scalar grayscale, and layer masks retain their existing display path.
- An unsupported embedded ICC profile still logs the sRGB fallback. The controller retains the warning until a status callback exists, then delivers it on the next main-actor turn so the workspace's “Opened …” status cannot immediately overwrite it. Delivery consumes the pending warning once.
- The P3 canvas regression explicitly asserts `wantsExtendedDynamicRangeContent == false` and that the layer colour space is the document profile. The sRGB path retains its existing EDR setting.

## Encoding boundaries inherited from B5-30

Files embedding HP or Apple sRGB profiles take the tagged document-profile path: they are not byte-identical to the engine's built-in sRGB profile. Opaque pixels are identical in colour; transparency blends in the profile's encoding, rather than in the linear sRGB canvas path used for untagged/built-in sRGB documents.

Zoomed-out filtering for non-sRGB documents happens in the document encoding. Tagging does not add linear-light resampling or change engine mip generation / Metal sampling.

When Assign / Convert to Profile exists, key re-tagging and dependent image-cache invalidation on the **profile digest**, not the display name. Different profiles can share a name. The current controller's name-based refresh trigger is inherited from B5-30 and is not a substitute for that future invalidation contract.

## Validation

RED commit: `5c72c345`. Targeted `DocumentDisplayColorTests` execution: 6 tests, 10 assertion failures, including the missing async P3 tag, overlay/layer/component thumbnail colour-space mismatches, and the undelivered opening diagnostic. The existing canvas P3 test (including the new EDR-off assertion) passed. The XCTest summary said “0 unexpected” despite those failures.

Fix commit: `87d8d02d`. The same 6 targeted tests passed with 0 failures after the fix (`swift test -c release -Xswiftc -enable-testing --filter DocumentDisplayColorTests`).

`build-ffi.sh` succeeded. The full required command was attempted serially, but **did not print SWIFT GATE OK**:

- First complete run: 867 tests, 3 skipped, 14 assertions across `DocumentHistoryKeyboardTraversalTests.testDocumentSwitchKeepsGlobalHistoryPreferencesAndShowsSelectedDocument`, `testInspectorTabSwitchKeepsHeightControlIdentityFocusAndValue`, and `ShellLayoutTests.testShellContainedAtEverySizeStateAndAppearance`.
- Isolated rerun: shell layout and document switching passed; the tab-switch test instead saw the shared preference `properties` when it expected `stack`.
- Second complete run: 867 tests, 3 skipped, one different failure: `DocumentHistoryKeyboardTraversalTests.testCollapseWithFocusedHeightButtonSettlesFocusOnLiveView` read `216 pt` instead of `192 pt`. Concurrent worktree XCTest processes were observed; preference interference is suspected, not proven.
- Third run passed those History checks, then failed `MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth` while opening a generated PNG. Investigation found `ShellLayoutHarness.swift:160` invokes `/usr/sbin/screencapture`, conflicting with this package's explicit no-screen-capture rule. The full runs had already reached that helper before this was discovered. The third XCTest process was terminated once the conflict was identified. Its gate reported failure (also listing `ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`); it is not a completed qualification run.

No manual GUI launch or capture was performed. The existing full-gate tests themselves invoke window capture, so the requested full gate and the no-capture restriction cannot both be honored as currently implemented. Do not describe this package as full-gate qualified. No tests were excluded or weakened, and gate scripts were not modified.

Rust, `Cargo.lock`, and `board.json` are unchanged. All commits are local. The full gate remains a qualification blocker requiring an approved non-capture test path or an explicit resolution of the conflicting requirements.
