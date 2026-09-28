# Develop mask retention focused evidence (2026-09-28)

This directory preserves the bounded test-first validation for the two-file mask-retention candidate on integration base `40d3054e0e4f4563bbc5b4ec09eca29e014d1959`.

The test-only baseline was commit `36f5845d03104e60ea55008857819a56120920cd`. Only `DevelopMaskPendingRetentionTests/testRejectedComponentAndUnattemptedGroupsParamsSurvive` was run against it. The test exited 1 with the expected behavioral RED: the rejected component value was lost and the rejected component was not retried. Other baseline tests were intentionally not run: the broader pre-candidate baseline contains a known recursive flush case that can overflow/hang, while later liveness cases require candidate-only APIs.

The candidate was committed as `8c628e63767c2b1b287c53f8054f2d4e4f625d29`. Its only product/test changes are `apps/mac/Sources/TesseraCore/Develop/DevelopController+Masks.swift` and `apps/mac/Tests/TesseraCoreTests/DevelopMaskPendingRetentionTests.swift`. Both files are byte-identical to their counterparts at `origin/codex/mask-pending-retention` commit `639da049f886e7f50533bcca887bdf1d1955b63e` (SHA-256 values are in `final-freeze.txt`). The product candidate did not modify the integrated `DevelopController.swift`, Rust, generated bindings, or local FFI archive.

The final focused Release command selected the 11 `DevelopMaskPendingRetentionTests`, plus adjacent `MaskingTests`, `MasksPanelLayoutTests`, and `TransformLensBlurTests`. Result: 25 XCTest executed (11 + 8 + 1 + 5), 0 failures, direct process exit 0. This was a focused gate, not a full suite. No GUI tests were run. Both runs used the preserved integration FFI archive SHA-256 `8ab43f64cf8bd510ee17c4cb19c5fff58e6e488015905c0b84b2ddfa0dce3a03` and external scratch under `/Volumes/betterSSD/tessera-cache/swift/mask-retention-red-validation/scratch`.

`baseline-red.log` and `final-focused-green.log` are the unedited combined stdout/stderr logs. Their adjacent `.exit` and `.duration-seconds` files record direct process status and elapsed time. Freeze files identify the exact source, archive, generated-binding hashes, commands, and scope for each run.
