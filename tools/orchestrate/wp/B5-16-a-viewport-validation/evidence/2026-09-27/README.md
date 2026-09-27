# A-side viewport ownership validation

The tested source was clean commit `fb4d7df8701b1e6df7417904534342f1218b1539`
on the isolated `codex/document-viewport-validation` branch. It follows the
separately validated and pushed outline checkpoint `398acb76`. Commits
`984da95b` and `fb4d7df8` import only `DocumentViewport.swift` and
`DocumentViewportOwnershipTests.swift` from B commits `63a06a2` and `b2f8d85`.
The final two files match B `b2f8d85` byte-for-byte; `source-manifest.json`
records their hashes, all adjacent source hashes, ABI header, retained FFI
archive, and imported patch hashes.

Both commands used release Swift tests, `-Xswiftc -enable-testing`, `--jobs 2`,
the external `export-integration` scratch directory, and a bounded subprocess
watchdog. The focused filter
`DocumentViewportOwnershipTests|DocumentOutlineLifecycleTests|LatestRequestBufferTests|DocumentOverlayAnimationTests`
exited 0: 16/16 selected tests passed (4 ownership, 3 outline lifecycle,
4 request buffer, 5 overlay animation). The adjacent filter
`DocumentToolsTests|DocumentViewportMathTests|DocumentOutlineTests` exited 0:
27/27 passed (15 tools, 4 viewport math, 8 document outline). Exact logs,
exit codes and elapsed seconds are preserved beside this report.

Ownership tests constructed offscreen Metal-backed views and retained one
1×1 IOSurface to check weak-controller teardown. The tools suite included a
tiny 256×128 engine adapter fixture. There was no GUI window, user catalog,
large canvas, or benchmark. The retained FFI archive
`19f9f5487752e2588c6febe99d126905cad7d9986285d2aebf633dfaeba2b427`
predates recent Rust resource fixes, so these results validate the scoped
Swift lifecycle behavior, not current Rust memory or whole-app behavior.
