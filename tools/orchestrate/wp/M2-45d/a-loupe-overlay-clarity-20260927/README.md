# Loupe overlay clarity validation

Source checkpoint: `codex/loupe-overlay-clarity`, based on `9f62d8738b829656fba447117cc7c09e82907998`. The focused gate was run against the four modified source/test files listed in `source-manifest.json`; no source edits followed the gate or GUI run.

## Focused source gate

`logs/focused-gate.log` and `logs/focused-gate.exit` preserve the direct run. Result: 14 passed, 0 failed (10 `KeyFocusTests`, 3 `LoupeOverlayPresentationTests`, 1 `ThemeLintTests`), process exit 0. Command:

```sh
MACOSX_DEPLOYMENT_TARGET=15.0 TESSERA_APP_DIR=/tmp/tessera-loupe-keylock-fix/onexit-test-app-support \
swift test --jobs 2 --package-path apps/mac \
  --scratch-path /Volumes/betterSSD/tessera-cache/swift/loupe-overlay-9f62d87 \
  -c release -Xswiftc -enable-testing \
  --filter 'KeyFocusTests|LoupeOverlayPresentationTests|ThemeLintTests'
```

## Isolated GUI run

The latest focused-test executable was packaged at `/Volumes/betterSSD/tessera-validation/loupe-overlay/escape-callback-20260927/Tessera-LoupeOverlay-EscapeCallback.app` (bundle ID `dev.tessera.validation.loupeoverlay.9f62d87.escapecallback`, executable `TesseraLoupeEscapeCallback`, SHA-256 `16e9f8432cac5138a423878a4592295fd6605447a113d4d03f4ee1d03b7022b7`). The bundle was signed and strictly verified. Its six generated JPEG fixture images and app-support directory were kept under `/tmp/tessera-loupe-escape-callback/`; this fixture set contains no RAW files.

Observed in the running app:

- The full long filename was present in Display info at both the initial window size and the narrowest resizable window reached (about 960×650). The passive image-top label ellipsized at the narrow width; Display info retained the complete name.
- With Display info open, Right, D, and X left the current photo, Loupe mode, and Undecided decision unchanged.
- Escape dismissed Display info while leaving Loupe selected. Escape dismissed Shortcuts while leaving Loupe selected. A subsequent Escape selected Grid.
- Command-Q was not verified as a successful app quit: the synthetic key action changed from the fixture window to the blank app window while the process remained alive. The app was then quit using its explicit app-menu Quit item; the exact validation executable process was confirmed gone.
- No RAW was available to exercise mask/picker pointer pass-through or live soft-proof ready/pending/unavailable states. No claim is made for those paths.

Prior GUI-red packages, logs, and startup diagnostics remain preserved (not overwritten) under `/Volumes/betterSSD/tessera-validation/loupe-overlay/9f62d87/`, `/Volumes/betterSSD/tessera-validation/loupe-overlay/9f62d87-keylock-fix/`, and `/Volumes/betterSSD/tessera-validation/loupe-overlay/exit-command-20260927/`. In particular, the earlier real Escape-routing failure and the `.onExitCommand` delivery failure remain recorded there. The external app bundles and logs are retained on betterSSD; this directory carries the portable source gate and result summary.
