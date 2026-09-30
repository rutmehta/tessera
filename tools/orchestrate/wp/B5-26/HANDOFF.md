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
  20 to 40 mm. It asserts: no tick takes ≥ 40 ms; the tracer never runs on the main thread (bound loosened to 80 ms in the review follow-up); ≤ 3 traces; the last trace is for 40 mm;
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

## Review follow-ups (Machine A review of `1ad514fc`)
Commits on top, no rebase.
- RED `51c3b89e`: new tests with a scripted tracer (per-call delays, optional failure, start/finish counts):
  - `testOKSurfacesAFailedFinalTrace` (RED): OK while a re-trace is pending and the final trace fails. RED failures:
    - `:382` "OK committed a constraint whose final trace failed"
    - `:383` `m.error` was nil, expected "Constraint 1: outside the camera's field of view"
    - `:384` history 1 ≠ 0 ("nothing committed")
  - `testOKDuringAPendingRetraceCommitsTheFinalFocal`: smart object, 21 ticks 20 to 40 mm with a 150 ms tracer, OK at once;
    the stored smart filter recipe has 40 mm and the curves of a direct 40 mm trace.
  - `testCancelDropsALateRetrace`: Cancel with a 300 ms trace in flight and one queued: no curves applied, revision and
    preview count unchanged, the queued trace never runs.
  - `testCloseAfterOKDropsALateRetrace`: OK (fast trace in the job) closes the sheet while a 500 ms slider trace is in
    flight: OK does not wait for it, and its late result changes nothing.
  - `testLinesEditedDuringARetraceMatchByEnds`: with a trace in flight, line 1 is removed and a new line drawn; the
    surviving line gets its new-camera curve, the new line keeps its own, no error.
  - The per-tick main-thread bound in `testFocalSliderRetracesOffTheMainThreadLatestWins` is 80 ms (was 40).
- GREEN `42bbb515`: in `ok()`, a failed final trace throws `DocumentError.invalid("Constraint n: …")` from the
  job body, so OK ends as a normal failure with the drag's message and nothing is committed. `applyTrace` schedules no
  preview while OK is committing (`busy != nil`); it still updates the draft.
- Gates: `apps/mac/build-ffi.sh` OK; AWA suite 18/18 in 5 repeated runs; `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK
  (861 tests, 3 skipped, 0 failures).
