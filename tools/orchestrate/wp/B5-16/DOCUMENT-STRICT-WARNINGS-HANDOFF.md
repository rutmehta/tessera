# Document strict-warning source handoff

Request a4ac739c-5640-48a9-a971-f1a47007a09b validated for exact B chat and accepted before edits. Supplement de6dcb0f corrects the callback site to checkbox Binding, not DocSlider. Separate branch `codex/document-strict-warnings` starts at preserved Smart Preview775f39b0. A owns AppModel FolderHandle import, offline alias fixture oracle correction, compiler/generated bindings/main. Do not cherry-pick the old Smart Preview base over A integration: only the commits below are new to this request.

- Tests: `17354fb9`, `beda8605` (three new methods in DocumentTransformTests, UNRUN).
- Product: `d6332c5a` (immutable render step, transform keys, self-test binding); `d7c86b65` (checkbox isolation).
- No AppModel, generated binding, Document save/load or Smart Preview test edits in this diff.

## Corrections and behavior

`TesseraCore/Document/StubDocumentBackend.swift`, viewport render: compute the same power-of-two sampling stride into a temporary variable, then capture immutable `let step` in concurrent row bands. Identical budget/rounding/bands/pixel loop and lifetime; no new unsafe/sendability suppression or synchronization changes. Existing unsafe raw surface pointer is unchanged.

`Tessera/Document/AdjustmentEditors.swift`, checkbox helper: callback is explicitly `@MainActor @Sendable (Bool) -> Void`. The installed SDK SwiftUICore Binding initializer accepts `@_inheritActorContext @isolated(any) @Sendable` get/set closures (arm64e-apple-macos.swiftinterface, Binding declaration near11570). The Toggle remains a synchronous MainActor edit; no Task, hop, assumeIsolated or unchecked wrapper. Existing per-toggle model capture/final=true behavior and DocSlider are unchanged. A must confirm against its actual SDK/compiler; no B compiler was invoked.

`Tessera/Document/Transforms/DocumentTransforms.swift`, keyboard routing: source inspection showed these warnings concern key patterns, not axis/geometry. Former `case36,76 where...` and `case51,117 where...` applied guards only to the final pattern. Extract a small KeyAction mapping used by actual handleKey, with explicit guard return per paired branch. Both Return/Enter apply only unmodified or Command; both Backspace/ForwardDelete request pin removal only unmodified. Escape remains unmodified. NumericPad/function/capsLock are still stripped. This deliberately corrects accidental Shift/Option/Control Return and modified Backspace interception; do not call that old accidental behavior unchanged. Session/activity and Puppet+selectedPin checks remain at dispatch; geometry math and operations are untouched.

Three tests cover both codes, allowed/disallowed modifier combinations, Escape/unknown-key negatives and incidental flag stripping. These exercise the mapping called by handleKey, not live NSEvent dispatch/backend geometry. Existing DocumentTransformTests retain geometry/session coverage for A to rerun; this handoff does not claim it passed.

`Tessera/Document/Transforms/TransformSelfTest.swift`: unused `if let id = doc?.primary?.id` becomes the same optional existence condition; position-lock action sequence and waits unchanged.

## Validation and limits

Source inspection and `git diff --check` only. All authored tests **UNRUN on B**. No builds/tests/apps/benchmarks/heartbeat restart or writer replacement. A should cherry-pick new commits onto its current integration, run strict warnings-as-errors compilation and DocumentTransformTests/adjacent adjustment tests, then retain original full-suite gates. No runtime acceptance, concurrency proof from execution, GUI or performance claim.

A additionally reports source SmartPreview initial20+6 router tests passed after its import fix; alias test failed seven assertions because Foundation path normalization differed from native POSIX canonical identity. A owns that fixture correction and retains failures. This branch intentionally leaves that test unchanged from775f39b0; it is not a newly accepted whole-app baseline.
