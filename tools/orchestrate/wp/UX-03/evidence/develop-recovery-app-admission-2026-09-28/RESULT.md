# Develop recovery and admission focused gate

The final focused gate used checkout `df1f1bc409614bf0e704baff85b74b381b7cbf69` with the existing FFI archive SHA-256 `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03` and generated header SHA-256 `e188c20e48663ce3c722a3ae9ee25f66b8412ddff711e55cd897063fef65e703`.

`attempt-3-full-focused` ran Release with `--jobs 2` and the filter `DevelopRecovery|ThumbnailFlightDrain|RecoveryWindowCloseGuard|OutputAdmissionCancellation`. It executed 31 XCTest cases with 0 failures, direct exit 0. `attempt-3-window-focused` separately ran the five NSWindow close-guard cases; all five passed, direct exit 0. The before/after source manifests are identical for both runs.

The 31 cases were: 5 AppModel recovery/navigation scenarios, 2 source-alias admission cases, 2 existing AppModel recovery-admission cases, 9 recovery coordinator state cases, 2 export/print cancellation cases, 5 NSWindow close-guard cases, and 6 thumbnail-flight drain cases. Swift Testing discovered 0 tests.

Two earlier failures are preserved without overwriting their outputs. Attempt 1 stopped at compile time because the original flight-drain test awaited a semaphore wait from an async context (direct exit 1). Attempt 2 compiled and ran the AppModel, coordinator, and output tests successfully, then XCTest crashed during teardown of the third NSWindow test (direct exit 1; the crash report is included). Resource corrected only test fixtures: the wait now uses a synchronous utility-queue worker, and test windows set `isReleasedWhenClosed = false`. The isolated window rerun and final focused gate passed after those corrections.

The final build emitted existing Swift warnings and an ld deployment-target warning for the archive (`26.5` object linked into a `15.0` target). These did not prevent the focused tests from passing. This is a focused gate, not full-suite acceptance.
