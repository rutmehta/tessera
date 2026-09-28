# Develop recovery / Layers admission validation

Candidate source: `5a82058d14e21c5e571c0f21679a5db01f2039ac` (clean review-ownership branch). The tested FFI archive remained SHA-256 `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03` in all attempts. Raw logs, commands, direct exits, source hashes, and status records are in each attempt directory.

## Preserved attempts

- `c4ba2ea6-layers-red/`: pre-fix RED, 4 selected cases, 3 passed and the held-backend status case failed as expected. Direct exit 1. This predates caller-owned `activateDocument: false`.
- `5a82058d-focused58/`: name retained from the planned scope; the actual selected count was **58**, 0 failures, direct exit 0. It covered recovery, flight drain, window guard, output admission, document status/settlement, Layers activation, and the original Layers navigation case.
- `5a82058d-prior7/`: 7 selected Review/open cases, 0 failures, direct exit 0.
- `5a82058d-full597-failed/`: full Release run, **597 executed, 1 skipped, 1 failure**, direct exit 1. Sole failure: `AgentReviewLayoutTests.testReviewEmptyAndRealQueueAtEverySizeAndAppearance`, `XCTUnwrap` at line 141 because the preview cache remained nil after the test's 15-second deadline.

The two focused selections are preserved separately; this report does not assert their counts are a unique combined total.

## Failure investigation note

Read-only source inspection found that `ReviewCurrentPreview.load()` terminates when its recipe-read barrier is non-saved and only starts another load when its identity changes. Thumbnail reservations can remain active until the native preview flight drains. A later view can therefore encounter a blocked reservation and remain without a retry when no identity changes. This is a plausible mechanism consistent with the full-suite failure, not a proven causal trace for this particular occurrence. The failing test and its 15-second deadline are unchanged.

Root verification: raw logs contain 58 and 7 distinct started XCTest identities with no overlap (65 unique tests), both direct exit 0. Root verified external checksums, copied payloads, and unchanged source/archive hash records. Full597 remains failed. Typed Save As is excluded. The original external directory named focused65 retains its planning label; actual count is58. Before publication the packaging verifier was corrected for the full run’s tracked-swift-hashes filenames and clean-status records containing only a Git branch header. No failing check was bypassed.
