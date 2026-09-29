# Inspector AX bridge qualification stopped

Exact114273ea focused01 compiled and executed47 XCTest cases:46passed,1failed with2assertions. Directexit1; all source/HEAD/fourFFI/Sony/runner/imported-runner/oracle/baseline freezes equal. No skips. Existing unrelated compiler warnings retained.

Failure: InspectorFocusAXBridgeTests.testNativeHeterogeneousChildrenKeepSnapshotUnknownAndReleaseObjects, lines107/108: weak NSAccessibilityElement root and child both remain nonnil immediately after lexical do scope. Earlier snapshot unknown/incomplete assertions did not fail. All four other new native bridge tests passed, as did the42 existing tests.

Possible AppKit autorelease lifetime was explicitly called out in source review, but this result alone does not distinguish autorelease ownership from another retain path. No assumption of product leak or test defect is established. Requires bounded diagnosis before any change; do not weaken lifetime assertions. Strict and GUI NOT RUN. No source changes or retries; runtime finished and released to root. Historical c5 crash and earlier evidence untouched.
