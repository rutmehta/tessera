# M2-58 qualified P01 interval source checkpoint

Source-only implementation checkpoint on `codex/m258-current-reconciliation`:

- Analyzer/test contract: `70e48fe5251461dd4a277bc71c7bc9f3ccb11a00`
- App/runner protocol: `5174d90ecec7aea26c073c238ffbd44a0271ddf5`
- Exact readiness-path identity correction: `9bf95afcf5dc16bf3e1b45eba49c9da91d594f7d`

No application launch, termination, GUI access, protected-dialog interaction, Rust build, or Swift build/test was performed for these changes. The prior failed visible capability runs and packaged `f1d13c11` app evidence were not changed. Python tests are allowed and passed at final source: `python3 -m unittest tools/bench/test_app_timing_visible.py` (17 tests); `git diff --check` passed. The pre-existing untracked `target` symlink remains untouched.

## Contract in source

`apps/mac/Sources/Tessera/App/TimingVisibleProtocol.swift:3-73` defines snake-case Codable handshake identity (nonce, PID, bundle ID/URL, launch date, window number, Develop session, CACurrentMediaTime) and a bounded single-start/single-end state machine. Its XCTest source in `apps/mac/Tests/TesseraCoreTests/TimingVisibleProtocolTests.swift` covers successful single interval, stale nonce, wrong PID/window/session, wrong bundle URL/launch date, duplicate start/end, early end, timeout, nonfinite times, and JSON field spelling. These Swift tests are present but unrun.

`apps/mac/Sources/Tessera/App/TimingSelfTest.swift:108-250` waits for a positive same-session baseline presentation and currently active/key/visible/occlusion-visible regular window before atomically publishing `ready.json`. It waits at 50 ms cadence for one matching permit, records `measurement_start`, checks window/session identity for each of 121 scripted Exposure changes, records one app-side visibility check per input, then waits up to 20 seconds for the final input's positive `drawable_presented` record before recording `measurement_end`. Missing identity, focus/window loss, timeout, malformed permit, or missing final presentation records `qualification_failed`; visible trace completion waits for interval end/failure. `AppModel.swift` routes only visible timing self-tests through this qualified path; the prior background audit loop remains.

`tools/bench/VisibleWindowProbe.swift` emits `CACurrentMediaTime` with each WindowServer observation. `tools/bench/app_timing_visible.py` creates a fresh nonce/control directory, validates ready against the uniquely pinned process, requires a stable foreground dwell of at least 500 ms with no probe gap over 250 ms, and atomically publishes the start permit. It collects identity-bound visibility samples through trace publication. `validate_qualified_interval` rejects dropped/failed runs, stale identity, duplicate markers/inputs, wrong-session chains, missing frame keys, nonfinite/nonpositive actual presentation, malformed input sequence, absent final-input presentation, fewer than 100 causal chains, missing app-side per-input checks, unbracketed/out-of-order samples, identity changes, or gaps over 250 ms. Its p50/p95 use the accepted earliest positive actual presentation once per input inside the exact interval only.

`tools/bench/README.md` states that the path is scripted `AppModel.setAdjustment` with `flushPending`, not an OS mouse gesture. It does not accept P11 detail appearance, a performance-threshold result, or physical-panel scanout.

## Remaining verification and limits

This checkpoint is uncompiled Swift source. Root/Astra source review and later authorized focused Swift tests are still required before any rebuild. The shared-clock premise uses `CACurrentMediaTime` in both app and helper and the handshake rejects time-order disagreement; target-machine behavior remains unverified until a permitted run. Visibility samples corroborate foreground/window state but cannot prove every pixel remained unobscured between samples. Actual P11 appearance and paired user mouse gestures remain unaccepted. No performance result is claimed by this source checkpoint.
