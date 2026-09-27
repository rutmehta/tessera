# UX-02b validation evidence

## Full current-FFI gate and isolated relaunch

The source freeze for the full gate is commit `16cb6ce5fd3c7459a5165eea07453e4836ebcada`, recorded in `evidence/ux02b-full-gate-source.sha256`. The latest current-FFI archive SHA-256 is `99ba90171889d8b4566377e9939a444f21a953f6cb51667ea791e59736eb628e`; the archive checksum record is `evidence/ux02b-current-ffi-archive.sha256`. The full release Swift suite exited 0 with 494 XCTest cases, 1 skipped, 0 failures, plus 5 Swift Testing cases. The raw log and direct exit record are `evidence/ux02b-full-swift-current-ffi.log` and `.exit`. The gate used a fresh scratch path and the archive was isolated from the historical 19f archive. Generated FFI Swift/header inputs matched the tracked sources after the build.

The exact tested executable was copied to a unique validation app at `/Volumes/betterSSD/tessera-validation/loupe-review-candidate/Tessera-UX02b-16cb6ce5.app`. Its copied executable received the validation-only Sparkle rpath and ad-hoc signature; it is not a release artifact. The app launched with a two-photo generated JPEG library and dedicated app-support path under `/tmp/tessera-ux02b-relaunch/`, using the scripted fake planner. It created the Review record in `ReviewRuns`; no user library or normal Tessera app-support location was used.

GUI sequence and result: the scripted run completed for both fixture photos; `01 - Accepted candidate.jpg` was accepted and `02 - Reverted candidate.jpg` was reverted. After selecting the second row, the app was quit and relaunched with the same arguments. Startup returned to Library with no Review modal and no planner replay. Opening Review restored both terminal states and selected the second, reverted row. The final saved Review record is `evidence/ux02b-gui-review-record.json` (completed revision 7); it stores queue membership and selection, not per-target review statuses. The corresponding accepted and reverted XMP recipe sidecars are preserved as `evidence/ux02b-gui-accepted-photo.xmp` and `evidence/ux02b-gui-reverted-photo.xmp`; status labels were visually observed in the Review UI before quit and after relaunch, not captured into a raw AX transcript. The generated fixtures' hashes are in `evidence/ux02b-gui-fixtures.sha256`. Both GUI screenshots were inspected through the computer-use display during the run; this tool did not provide a persistent screenshot file path, so the portable evidence records the saved queue and recipe state instead.

This verifies deterministic fake-planner relaunch behavior for a tiny generated JPEG library. It does not cover RAW decoding, network providers, or larger libraries.

The final focused gate ran on the source state represented by `evidence/ux02b-group-validation-freeze.sha256`:

```sh
swift test --package-path apps/mac --scratch-path /Volumes/betterSSD/tessera-ux02b-review-resume --jobs 2 --filter 'ReviewResumeStoreTests|AgentReviewQueueTests|AgentReviewNavigationStateTests|AgentReviewOwnershipTests|AssistTests'
```

The test process exited 0: 47 selected tests, 0 failures. The preserved transcript is `evidence/ux02b-group-validation.log`. The hash manifest freezes the Swift product/test inputs immediately before that gate. The broader preceding controller gate and its manifest are retained separately.

Earlier evidence is intentionally retained without rewriting: the test-first compile RED (`ux02b-review-resume-store-red.log`), the first store GREEN runtime failure (`ux02b-store-green.log`), the subsequent recursion crash (`ux02b-store-repaired.log`), and the later controller gate (`ux02b-controller-gate.log`). These show the progression and are not acceptance evidence. The final group-validation gate is the current passing result.

The older focused gates below remain useful implementation history; they are not substitutes for the full current-FFI gate and GUI sequence documented above.

The Review empty-state sentence was updated after the focused gate to say queues resume when reopening the library; this copy-only adjustment was not part of the gate. `evidence/final-source.sha256` records the final candidate bytes, while `ux02b-group-validation-freeze.sha256` remains the exact pre-gate test freeze.
