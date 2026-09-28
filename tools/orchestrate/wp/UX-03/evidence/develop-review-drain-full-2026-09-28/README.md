# Develop recovery and Review preview-drain validation

Tested recovery candidate HEAD: `634b7e6807e47e217c2649b65de7936f06bd97df` in `codex/develop-recovery-admission-tests-425` (clean before/after the final full run). The source includes Resource's Review barrier drain-wait fix (`7f5c4c49`), exact wait-state diagnostic (`d5ae4ef9`), and the held-flight tests. The FFI archive SHA-256 was `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03`. Full-run source hashes and archive hashes are preserved in `attempts/full-green-599/`.

## Results

- `layers-pre-fix-red/`: 4 selected cases, 3 pass / 1 fail (direct exit 1). This is the earlier caller-owned Layers activation failure, retained as pre-fix evidence.
- `focused-filter-58/`: 58 selected, 0 failures (direct exit 0). The directory name reflects the planned filter, not the actual count.
- `prior-seven-7/`: 7 selected, 0 failures (direct exit 0). Kept separate; this report does not claim the two filter counts form a unique combined total.
- `full-597-red/`: 597 executed, 1 skipped, 1 failure (direct exit 1). The failing Review preview layout timeout was preserved.
- `first-held-flight-nondiscriminating/`: old immediate gate-result baseline passed 1 selected test (direct exit 0). This is explicitly **not** valid RED evidence: the first fixture only waited for gate creation, not completion of the successor evaluation.
- `held-flight-behavior-red/`: after strengthening that handshake, the old immediate-result baseline failed the user-visible replacement-render/cache assertion (1 selected, 1 failure, direct exit 1). Only `ReviewCurrentPreview` was temporarily switched from `resultWaitingForPrecedingDrain()` to `result()`; the checked-in source was restored byte-for-byte afterward. Source and FFI hashes are recorded before/after.
- `review-layout-green-4/`: 4 selected, 0 failures (direct exit 0), including the prior layout timeout and both dedicated held-flight/failed-close tests.
- `adjacent-green-68/`: 68 selected, 0 failures (direct exit 0).
- `full-green-599/`: 599 XCTest executed, 1 skipped, 0 failures; direct exit 0. The 5 Swift Testing tests also passed. Tracked Swift source/header hashes, FFI archive hash, and clean git status match before/after.

The held-flight test controls the non-interruptible first worker, waits for the second Review observation to either suspend on that flight or finish fail-closed, then requires a second render and delivered preview after release. It also cancels one queued observation before mounting a replacement. The companion failed-close case proves Review observation stays blocked and does not automatically retry a failed Develop save.

No GUI validation was performed in this lane; the Release suite is the extent of verification here. The old-result baseline mutation was temporary and is not part of the candidate. Build warnings about the archive's recorded macOS 26.5 deployment target while linking for 15.0 remain visible in raw logs; this evidence does not establish macOS 15 compatibility.
