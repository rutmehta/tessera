# B5-43 — P19 live filter resize reproduction

P19 remains **unreproduced/open**. Two distinct live-document configurations completed
four resize operations each without an AppKit exception. No production layout fix was
made. Branch `wp/B5-43`, base `445131fc786900e111d4e7ff9fb010ba2dfc2cf5` (the local
`origin/main` at start). Local commits only.

## Reproduction conditions and observations

Both attempts used the real `SelfTestHost` / `ContentView`, engine-backed documents,
Gaussian Blur sheet attached, inspector Stack tab visible, expanded History, and preview
updates immediately before resizing. The host was ordered behind other windows, never
key/main or activated. No screen capture or real Lightroom catalog access.

`NSSetUncaughtExceptionHandler` logs exception name, reason and call stack. The probe
sets `NSViewLayoutFeedbackLoopDebugging` and
`NSConstraintBasedLayoutLogUnsatisfiable`; the app launch also passes both defaults
on the command line. There was no exception reason to capture. This is not proof that
the historical exception cannot recur, nor that this handler intercepts every internally
caught AppKit exception.

| Attempt | Document | Initial host points | Requested host points / observed | Exact device-pixel check |
|---|---|---|---|---|
| Hosted XCTest | New 512×512 u8 engine document, one exposure history entry | 1440×984 | 3840×2160 → 3840×1289, then back | Host 2444×1157 → viewport 1920×1080 pt @2× = 3840×2160 pixels, then back |
| Background filter self-test | `sample.dng` opened via Edit in Layers, 5212×3468 u16, Metal (Apple M4 Max) | 1440×984 | 3840×2160 → 3840×1289, then back | Host 2444×1157 → viewport 1920×1080 pt @2× = 3840×2160 pixels, then back |

Both returned to a **916×907-point viewport**. The literal oversized host request was
clamped in height. It did not throw an exception. The first oversized viewport measured
3316×1212 points in XCTest and 3316×1179 points in the app; the exact 4K viewport case
matched in both. Logs include every requested/actual frame, backing scale, sheet attachment,
key/main flags, and inspector region rectangles. History body was 296×168 points at each
measured size. The two distinct live attempts finished their full resize sequences.

The initial probe treated the literal host clamp as an assertion failure: **one failure
in each attempt**, not a clean overall pass. These original results are preserved in
`evidence/reproduction.log`. The committed diagnostic logs that oversized point request
as an observation, while still failing missing live state, a detached sheet, key/main
activation, a clamped exact-4K target, or failure to restore the initial size. It does not
change window constraints or override AppKit's frame handling.

Setup failures (not counted as completed live attempts):

- An unordered XCTest host rendered the document but did not attach the filter sheet;
  it failed its precondition before any resize. The test now uses the same below-all-windows
  ordering as the background host and waits one second for content attachment.
- The first background launch used a symlinked RAW fixture; library loading timed out before
  a document opened. A fresh isolated app directory with a physical fixture copy loaded
  successfully. Both setup failure logs are retained in the compact evidence.

After the two completed attempts, reproduction exploration stopped. There is no failing
P19 crash regression and no speculative production constraint fix. The retained
`test(B5-43):` commit adds a diagnostic and future regression coverage. A subsequent
`fix(B5-43):` commit corrects test-host disposal exposed by the gate, described below;
it is not a fix or closure of P19.

## Re-run

Use `PATH="$HOME/.cargo/bin:$PATH"` and
`CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-43"`.

1. `tools/orchestrate/wp/B5-43/run-build.sh` builds FFI and the selected release hosted test.
   It waits for already-running build processes before starting; B5-43 runs its build steps
   sequentially. Other jobs may start independently on this shared host.
2. `tools/orchestrate/wp/B5-43/package-probe.sh` wraps the exact release executable from that
   build, adds Sparkle, and signs ad hoc. It does not install an app. The two SHA-256 hashes
   in `evidence/package.log` describe the measured executable before/after rpath/signing.
3. `tools/orchestrate/wp/B5-43/run-background.sh` uses only
   `open -g -n ... --nonactivating`, `TESSERA_FILTER_LAYOUT_REPRO=1`, fresh task-local
   fixture/app/output directories, and the normal `--filter-selftest` entry point.
   Preserve/rename previous run directories before repeating to retain old logs.

The packaged binary used for the measured attempt predates only the diagnostic assertion
reclassification above; all resize operations were identical. No second background run
was made just to turn that recorded failure into a green summary. This targeted P19 mode
does not run B5-15's unrelated export performance scenarios.

## Verification

The first required gate ran **892 XCTest tests, 3 skipped, 1 failure**, plus **5 Swift
Testing passes**. `SelfTestHostTests` passed, including P19. The only failing test was
`ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`, reporting
26 region violations with stale coordinates from the reproduction host (1440×984).
The new harness closed its window without detaching its hosting tree. This let later
inspector geometry callbacks pollute the following suite's global probe.

The test now waits for sheet dismissal and calls the existing
`LayoutProbeHarness.dispose` helper to detach the hosting tree. The combined targeted
sequence then passed: **5 SelfTestHost tests + 1 inspector test, 6 total, zero failures**.
No assertion in the existing inspector test was changed or suppressed. This is a harness
isolation correction after a real RED full-gate result, not a product resize change.

The session was locked (`CGSSessionScreenIsLocked=true`), but the first gate's failure
was **not** a window-capture failure. Do not attribute it to the locked screen.

The final required gate **PASSED**, exit 0, and printed **SWIFT GATE OK**:
**892 XCTest tests, 3 skipped, zero failures** in 185.918 seconds; **5 Swift Testing
tests passed**. Both the P19 diagnostic and the following inspector test passed in this
full run. `evidence/verification.log` preserves the relevant RED/GREEN lines and final
summary. The complete raw logs remain local and ignored.

Required command executed twice (serially):

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-43"
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

No Rust source changes: crate-specific Rust test/clippy/fmt gates do not apply.
`board.json` and `Cargo.lock` are untouched.

## Local commits

- `4640e52d` — `test(B5-43): probe live filter sheet layout during background resize`.
- `c8557994` — `fix(B5-43): dispose the live reproduction host after sheet dismissal`
  (test isolation only).
- The docs commit contains this handoff and compact evidence.

All commit messages end with the requested co-author trailer. Machine A owns integration.

## B5-43b — display-aware viewport target (2026-10-01)

Machine A's 3840×2160 non-Retina display has a 3840×2130-point visible frame.
The original exact-4K diagnostic incorrectly failed when that frame could not
accommodate 2160 viewport pixels plus chrome. This follow-up corrects the
diagnostic only; **P19 remains unreproduced/open**.

The shared reproduction now measures the attached host frame minus the live
viewport bounds to obtain chrome/panel overhead, subtracts it from the host
screen's visible frame, and converts the remainder with the backing scale.
It requests the per-axis minimum of 3840×2160 and those achievable pixels.
Whole-pixel capacity is rounded down. It checks exact achieved viewport pixels
and exact restoration of both the original host and viewport sizes. The literal
oversized host-points probe remains observational.

An unreachable 4K target produces an informational result note:
`N/A: 4K not reachable on this display (achievable WxH)`.
The background filter self-test includes this note in its successful check label
because its existing check helper omits detail text on success. Notes do not
count as failures. Exception-handler installation/restoration and layout-logging
arming are unchanged.

### Tests-first and verification

- `effeb150` — `test(B5-43b):` extracts the existing fixed target unchanged and
  adds injected geometry tests. The 3840×2130 @1× case with measured 524×77-point
  overhead failed as expected: actual 3840×2160 versus expected 3316×2053.
  A sufficiently large @2× display retains the exact 3840×2160 target.
- `ff54555c` — `fix(B5-43b):` implements achievable sizing, exact checks, and
  successful N/A result reporting.
- The required FFI + Swift gate command passed cleanly: **SWIFT GATE OK**,
  **894 XCTest tests, 3 skipped, 0 failures** (191.307 seconds), plus
  **5 Swift Testing tests passed**.
- Final-source standalone `SelfTestHostTests` ran twice: **7 tests, 0 failures**
  on each run. Earlier standalone runs also passed twice.
- This host's measured achievable viewport was **3064×2424 pixels**; it requested
  and reached **3064×2160 pixels**, then restored the **1440×984-point host** and
  **916×907-point viewport**. The result contained the explicit N/A note.
  Every observed resize kept the sheet attached and key/main false.
- One earlier gate invocation was invalidated by an edit during its debug build
  (`input file was modified during the build`). Its own process tree was stopped;
  the clean full command above was rerun with source fixed. Other jobs were untouched.

Compact RED/GREEN evidence is in `evidence/B5-43b-verification.log`. Raw logs are
local under `/tmp/B5-43b-*.log`. The physical Machine A display was simulated by
injected geometry, not exercised here. No packaged background app rerun was
performed; its shared diagnostic is exercised by hosted XCTest, and its result
label path compiled in the gate.

These commits are local on `wp/B5-43`, directly above `41da8502`, without rebase.
No Rust source, `board.json`, or `Cargo.lock` changes. No foreground GUI launch.
All follow-up commit messages end with the requested co-author trailer.
