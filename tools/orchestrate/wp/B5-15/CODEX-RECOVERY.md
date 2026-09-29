# B5-15 export recovery — 2026-09-27

**Export stall fixed and verified; B5-15 remains NOT READY for full acceptance.**
Machine A retains sole merge ownership. Source commits: 34a394d (export process
activity, strict self-test failures, Theme tokens and Clippy fixture cleanup),
22f416e (isolated export acceptance mode). Integrated main 503bc46 in 1389f6c.

## Cause and fix

The inherited asynchronous app run timed out twice after 900 seconds on the
styled 4608 × 3072 export. An unchanged-binary reproduction showed runnable
export workers throttled to macOS priority 4T while the covered app made little
CPU progress. Stack samples showed rendering work rather than a deadlock.
A detached user-initiated Swift task alone did not prevent App Nap.

A launch-only control (`-NSAppSleepDisabled YES`, no persistent defaults changed)
on the unchanged binary completed both styled exports in 100.12 and 99.37 seconds.
The fix scopes a Foundation `userInitiatedAllowingIdleSystemSleep` activity to the
export worker. `defer` ends it on success, error and cancellation, before the
completion callback. It does not prevent the Mac from sleeping.

The app self-test now samples the OS suppression flag during export, requires
actual samples, counts prerequisite failures, cancels timed-out work, and keeps
timeouts as failures even if cleanup finishes successfully.

## Fixed app run, normal App Nap settings

Release bundle provenance and signature verified against 22f416e. Run in an
isolated app directory with no competing builds/tests, on this M4 Max:

```sh
open -g -n apps/mac/build/Tessera.app \
  --env TESSERA_FILTER_PERF=1 --env TESSERA_FILTER_PERF_EXPORT_ONLY=1 \
  --env TESSERA_DOC_PERF_LOG=1 \
  --stderr /tmp/tessera-b515-export-only/stderr.log \
  --stdout /tmp/tessera-b515-export-only/stdout.log \
  --args --nonactivating --timing-grid-only \
  --folder /tmp/tessera-b515-fixed/photos \
  --app-dir /tmp/tessera-b515-export-only/appdir \
  --filter-selftest /tmp/tessera-b515-export-only/out
```

The photos directory contains a copy of sample.dng. Without `--timing-output`,
`--timing-grid-only` provides the existing nonactivating background panel without
starting a competing timing test. Export-only mode skips filter-drag/viewport
changes while retaining both export pairs and cancellation. No App Nap override.

| Check | Result |
| --- | --- |
| 18 MP smart-filter exports | 2.06 s / 1.83 s |
| Styled 14 MP exports | 81.94 s / 78.40 s |
| App Nap suppression checks | All four passed |
| Styled main-thread p95 | 0.06 ms / 0.07 ms |
| Styled main-thread maximum | 35.93 ms / 58.00 ms — exceeds 8 ms target |
| Cancel at 31% | Finished in 4293 ms |
| Destination preservation | Exact previous bytes retained; no temporary file |
| Exported PNG metadata | Both 4608 × 3072, sRGB built-in |
| Completion | Exact done line, zero failures; app exited |

Control and fixed runs use different background viewport configurations; these
measurements demonstrate restored export completion, not a precise speedup ratio.
The final app footprint was 14824 MiB. Full raw logs and build provenance are in
`recovery-evidence/`; exported images remain in the isolated local output folder.

## Gates

- Rust release (`filters`, `compositor`, `tessera-ffi`): 701 passed, zero failed,
  37 ignored across 114 groups. No Rust production code changed in this recovery.
- Smart-filter render/edit/save round-trip: 20/20 consecutive passes under
  compiler load; each run records an active Swift compiler PID.
- Strict all-target tessera-ffi Clippy and workspace fmt passed after equivalent
  parity-fixture cleanup. The changed GPU test target passed again, 4/4.
- Final-source strict Swift gate: 402 XCTest cases, one skipped, zero failures;
  all five Swift Testing tests passed. Debug build and Xcode Debug build passed.
- FFI regeneration left tracked bindings unchanged. Release build, signing and
  provenance validation passed for 22f416e.

The initial Swift run caught inherited raw spacing/system-ink usage in the export
bar. Existing Theme tokens now supply the same spacing and appropriate secondary
ink. The full suite was rerun after correction and again on the final harness.

## Outstanding acceptance and independent failure

The strict <8 ms main-thread maximum remains unmet. The inherited harness uses
14 MP styled and 18 MP smart-filter images, not the brief's 20 MP fixtures. No
full P16/P19 or memory acceptance is claimed; compositor optimization belongs to A.

The first current-source full filter run (34a394d) reached the second drag and
then terminated with an AppKit layout exception before any export. Its exception
summary and preceding log are preserved. The isolated export run does not resolve
that layout failure or establish P19 drag acceptance. Its viewport was also not
4K, so the inherited label must not be interpreted as verified 4K coverage.

Older Claude results remain historical. The control-run full filter trace showed
GPU Gaussian stages without CPU fallback, but current full filter/viewport
acceptance still needs investigation and a clean rerun. Do not merge solely on
the strength of the export-stall fix or passing unit suites.
