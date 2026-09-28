# Core Develop close result validation — 2026-09-28

The Core-only result-bearing close implementation is source commit `dbb740bb5f558dffa3ddf2e98195aaf98971c4a7`. It returns host settings/mask and native close errors, leaves a failed session retryable, shares one in-flight result with joining callers, and rejects mutating controller calls while closing. It does **not** implement AppModel recovery or convert its existing callers to inspect the result; the UX-03 plan lists that follow-up. It does not enable the Develop writer lease.

All Swift gates used the byte-identical FFI archive SHA-256 `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03`, two build jobs, Release configuration, and the external scratch path recorded in each command file. The three tracked generated binding inputs were unchanged and match current main at preparation: `TesseraFFI.swift` `b78ca980fa0ed7997ad3f65c17a7ea3b509758ebf466679f130702d83f25bca4`, `CTesseraFFI.h` `e188c20e48663ce3c722a3ae9ee25f66b8412ddff711e55cd897063fef65e703`, and `module.modulemap` `efda206de8cf8eb6c092c29fd32f286b9a46d9d7c4c150e6e9b66941dc43d6bd`.

| Stage | Source | Direct exit | Result |
| --- | --- | ---: | --- |
| First RED setup | `e96781ac` | 1 | Swift 6 test-harness compile error: semaphore wait called directly in an async closure. No behavioral claim. |
| Behavioral RED | `7cc7d39d` | 1 | 2 selected tests compiled; 10 state/persistence assertions failed against current `Void` close. |
| First implementation | `25281354` | 1 | 12 of 13 focused tests passed, then `testUnencodableSettingsBlockCloseAndRemainPending` aborted with signal 6. No green claim. |
| Focused final | `dbb740bb` | 0 | 13 focused Core close tests, 0 failures. |
| Adjacent setup | `dbb740bb` | 1 | 36 Agent Review + 11 mask-retention + 2 generated-JPEG Develop tests passed; one RAW Develop test had two setup issues because this checkout lacked `fixtures/raw`. |
| Adjacent final | `dbb740bb` | 0 | 50 adjacent tests, 0 failures, using the existing repository RAW fixture via a read-only symlink. |
| Full final | `dbb740bb` | 0 | 540 XCTest tests executed, 1 existing skip, 0 failures; 5 Swift Testing tests also passed. The suite includes its existing RAW and GPU paths. |

Each stage directory retains the exact command, direct exit, elapsed time, raw log, and input manifest. `source-snapshots/` retains the baseline, first candidate, and final source bytes named by commit. The initial checkout's ignored FFI symlink resolved to the older archive SHA-256 `19f9f5487752e2588c6febe99d126905cad7d9986285d2aebf633dfaeba2b427`; its target was never overwritten. A byte-identical backup remains at `/Volumes/betterSSD/tessera-validation/develop-close-core/e967-red/historical-19f9-ffi.a` (omitted here because it is 284 MiB).

The first candidate's crash report is `252-candidate1/xctest-2026-09-27-225556.ips`. The isolated `diagnostic/` source, command, log, and direct exits show the cause: `JSONSerialization.isValidJSONObject` returns false for NaN, while calling the Foundation JSON writer directly raises an uncaught `NSInvalidArgumentException` (`Invalid number value (NaN) in JSON write`); the finite control exits zero. Final source checks the normalized JSON value before entering that writer and returns a typed close failure while retaining the pending patch.

The initial adjacent setup failure and final 50-test success are both preserved. The worktree's previously absent `fixtures/raw` was linked only to `/Users/rutmehta/Developer/tessera/fixtures/raw`, the same source used by the existing test checkouts. The original fixture files were not changed; their filenames and hashes are in `dbb-candidate2/raw-fixture-sha256.txt`. Tests copied needed photos into scratch directories.
