# B5-26 handoff: Adaptive Wide Angle constraint re-trace off the main thread

Branch `wp/B5-26` from `origin/main` `031eaa56`. Swift only (`apps/mac/Sources/Tessera/Document/AdaptiveWideAngle/DocumentAdaptiveWideAngle.swift`
and its test). No Rust, FFI, format or board change.

## Problem (Machine A review of B5-20)
Every focal-length slider tick (and projection change) re-traced every constraint synchronously on the main actor via
`adaptive_wide_angle_curve`. With B5-20b's adaptive segment counts that is up to 1024 segments per line per tick.

## Approach (minimal: latest-wins, no timer)
- `AdaptiveWideAngleWorkspaceModel.tracer`: injectable `@Sendable (recipeJson, from, to) throws -> [CGPoint]`, defaulting
  to `backend.adaptiveWideAngleCurve`.
- A camera change (`setFocal`, `projection`) calls `cameraChanged()`. The slider value itself lands on the draft at once.
  If the curve key changed and there are lines, the draft snapshot goes into a second `LatestRequestBuffer`
  (`traceGate`, the same primitive the preview uses): one re-trace runs in `Task.detached`, and later ticks replace the
  pending one. A drag of N ticks costs about two traces, however slow the tracer is.
- On completion, the result applies only if its generation is still current and its curve key equals the draft's.
  Curves are matched to the current lines by their ends, so lines drawn meanwhile keep their own trace (already done
  under the new camera) and deleted lines are skipped. Then the preview is scheduled. With no lines, or no change to the
  curves, the preview is scheduled immediately, as before.
- OK while a re-trace is still pending traces the final camera inside the render job (off the main thread) before the
  commit, so the committed recipe always matches the released value. Trace errors keep the old per-line behaviour
  (`Constraint n: …` message, line kept).
- Close/Cancel invalidates `traceGate`.
- Unchanged: drawing a new line still traces that one line synchronously (a single trace per pointer-up), and scale,
  orientation and removal previews behave as before.

## Tests
- RED `6ef08b10`: `DocumentAdaptiveWideAngleTests.testFocalSliderRetracesOffTheMainThreadLatestWins`. It uses a real
  engine document with one fisheye constraint, a tracer that sleeps 80 ms and records its thread, and 41 ticks from
  20 to 40 mm. It asserts: no tick takes ≥ 40 ms; the tracer never runs on the main thread; ≤ 3 traces; the last trace is for 40 mm;
  the curves end equal to a direct trace at 40 mm; no curves other than the initial and final ones are ever observed,
  including during 0.4 s of settling afterwards; and a preview lands. RED failures:
  - `:312` tick took 0.0905 s ("a slider tick waited for the tracer on the main actor")
  - `:334` "the tracer ran on the main thread"
  - `:336` "41 ticks traced 41 times: not coalesced"
- `testWorkspaceDrawsTracedLinesPreviewsAndAppliesOnce` now waits for the re-trace after `setFocal(30)` instead of
  reading it synchronously.
- GREEN `038df061`: suite 13/13, passed 5 repeated runs.

## Gates
`apps/mac/build-ffi.sh` OK; `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK (856 tests, 3 skipped, 0 failures). Rust untouched, so no cargo gates.
