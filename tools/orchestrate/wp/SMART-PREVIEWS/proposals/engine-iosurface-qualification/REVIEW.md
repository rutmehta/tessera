# Engine/IOSurface qualification source review

Disposition: source-approved for compilation and bounded qualification; no blocking source defect identified. No builds or workloads were run by this reviewer.

Reviewed candidate SHA256:
- engine-iosurface-qualification.patch: 48f95e6fe727af9a676b8a4ed876d00b530d600cc7ffeccf587633a5b3c135ef
- run.py: 20f818647083369e60cc4133988b48813f73899e76abec0feba737c466e4e4de
- compare.py: 64804c3a8f5019dfc8260e89f00e5caf2716a1131802ddc3dabbe9ceac905e1f

The test uses real Engine sessions and presentation surfaces, waits for the requested generation's final frame, and records selected backend separately from actual resident execution. Forced GPU requires resident receipt, new submissions, nonzero resident dispatches and unchanged pixel-readback bytes. Calibration precedes the per-frame counter snapshot. Pixel inspection and disk writes follow latency and counter capture. Fixture writes target disposable copies, with original bytes checked afterward.

Each route uses the same baseline and sequence of exposure/WB edits. Comparisons assert exact settings identity. Speed ratios and raw CPU/GPU parity gates apply only to equal proxy render dimensions; resized Original/proxy differences are reported as spatial differences without an invented acceptance threshold. Viewport interpolation is an approximation, not compositor identity. Sequential completed interactive edits do not establish continuous-drag or physical input-to-display latency. Fresh children do not establish cold OS/driver caches. Two repetitions do not establish tail latency. These limitations are correctly disclosed.

The SDR 4/255 and EDR 0.002 + 0.002*abs(reference) tolerances are provisional qualification criteria, not independently validated perceptual equivalence. Retain raw metrics/artifacts and inspect them before making quality claims. Compilation, strict checks, actual Metal execution and runtime results remain outstanding.

Optional evidence improvement: write the post-run source manifest before asserting child success, or in a finally block. Currently an unsuccessful child retains its log/exit/command and initial manifest but omits the immediate after manifest. Successful children do verify the tracked and untracked native source freeze.
