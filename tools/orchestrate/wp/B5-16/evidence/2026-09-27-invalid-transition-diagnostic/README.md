# Direct recipe JSON error provenance (diagnostic only)

The original focused Review tests at source `77c47330` and `81a6b761` failed after `recipe serialized` and before `recipe written`, reporting `InvalidTransition { phase: idle, targetPhase: failed(deinit) }`. Their raw logs and manifests remain in the adjacent `2026-09-27-psd-copy-operation/red-json-exception` and `red-json-phase` evidence. Those logs did not capture the Swift dynamic error type or UniFFI call status.

Two later **focused Release diagnostic reruns** used the same preserved FFI archive (SHA-256 `0a9b2de3dee742751da067147805715925ac005e7036d0268d36296dc25168ae`). Both completed with direct exit `0`, one XCTest and zero failures, and logged a caught `TesseraFFI.BridgeError.Failure` with message `conflict: settings do not match history head` when exposure was changed without updating recipe history:

| Diagnostic | Test source | Result | Evidence |
| --- | --- | --- | --- |
| Headless setter control | `372f28dc` from `origin/main` `18f98e06`; generated 96×64 JPEG, unmodified setter control, then invalid settings mutation | 1/0, 32.40 s | [`headless/test.log`](headless/test.log), [`headless/result.json`](headless/result.json), [`headless/manifest.json`](headless/manifest.json) |
| Review context | `2457e9e3` from exact old `81a6b761`; only an import and typed catch around the same post-preview setter call | 1/0, 203.24 s including fresh older-source build | [`review-context/test.log`](review-context/test.log), [`review-context/result.json`](review-context/result.json), [`review-context/manifest.json`](review-context/manifest.json) |

Each evidence directory also includes the direct `exit.txt` and source/archive SHA-256 values. The full tracked `apps/mac/Sources` and `apps/mac/Tests` source bytes for each diagnostic were frozen externally as `source.tar` under `/Volumes/betterSSD/tessera-validation/review-invalid-transition/`; the branch commits name the same test source. The Review-context run used a separate SwiftPM scratch directory so it did not overwrite the earlier validated PSD test bundle.

The settings/history invariant explains the **normal caught BridgeError** in these reruns. It does **not** identify the origin of the earlier phase/deinit report. The original uncaught error may have arisen during reporting or unwind, but that is only a hypothesis. The `InvalidTransition` result remains preserved and unresolved. No product change, broad suite, app acceptance claim, or main merge follows from these diagnostics. Diagnostic source branches are not part of this evidence-only commit.
