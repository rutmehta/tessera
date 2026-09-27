# Tessera Editing Preview validation

## Build and source

- Candidate source: `65fa6a33bb0f64e4db329c2e261275e495da07ba` (`Fix baked export metadata on reimport with verified regression evidence`), branch `codex/editing-export-preview`; product worktree clean after build.
- Aggregate tracked source SHA-256: `d841381e7ef82a2a4f81295d7bab524c658cd0f13e8bb74cc53a4429542ef91d`.
- FFI build: `build-ffi.log`, direct exit `0`. Archive SHA-256 `2f241fe71c0216118ede2e1f1de70e6b1b64e68c8d5dcf8fb368de2426dca211`; generated Rust library SHA-256 `2b2e4bea41c73525c0a6171ee7ba3d6d1324e40c8e1b06628180c2c4d3042490`. Swift binding, C header and modulemap were unchanged; hashes are in `preview-validation-manifest.json`.
- Full fresh-scratch Swift suite: `swift-test.log`, direct exit `0`; 504 XCTest, 1 skipped, 0 failures, plus 5 Swift Testing checks passing.
- Linker emitted a BLAKE3 NEON object deployment-target warning (object built for macOS 26.5, linked target 15.0). This A-host preview does not establish compatibility on macOS 15.

## Preview packages

- User-facing, no fixture arguments: `/Volumes/betterSSD/tessera-validation/editing-export-preview/Tessera-Editing-Preview-65fa6a33.app`; bundle ID `dev.tessera.preview.editing.65fa6a33`. Its `TESSERA_APP_DIR` is `/Users/rutmehta/Library/Application Support/Tessera Editing Preview`. It has no document-handler registration, automatic update checks disabled, and passes deep strict code-signature verification. This no-argument package was not launched during fixture tests; the support path was confirmed absent before the normal launch handoff.
- Disposable test package: `/Volumes/betterSSD/tessera-validation/editing-export-preview/Tessera-Editing-Preview-Validation-65fa6a33.app`, bundle ID `dev.tessera.validation.editingpreview.65fa6a33`; tested with explicit disposable `--folder` and `--app-dir` arguments. It is closed after testing. Its tested executable came from the same fresh Swift suite build; only package rpath/signing differed.
- Both package executable hashes and package metadata are in the manifest. The earlier Review validation app/archive and historical archive were preserved separately before the build.

## GUI checks

Disposable catalog and support data live under `/tmp/tessera-editing-preview-validation-20260927/`.

- JPEG input: generated 320×240 checkerboard `jpeg-input/Fixture long name - display proof test.jpg` (SHA-256 `7ea9ae26e75949e577d8a441b555e2ee4364a014ec034e7598e2a841d282b86e`). I edited exposure to +0.10 EV, saw the recipe sidecar record 0.1, and exported with the Full-size JPEG preset to `jpeg-output/`. Export toast reported 0.2 s; this is a single UI observation, not a performance measurement. Output is 320×240, SHA-256 `d3b81a974b608955683b2f181fd25eba7858cac77234a2701a7eff0f171ae245`. Embedded XMP and adjacent XMP sidecar contain no develop/private edit properties. Reimported in Library and opened the editor: Exposure was +0.00 and the photo showed Unedited. This validates neutral controls/stripped edit metadata; no pixel-identity comparison was performed.
- RAW input: copied Sony NEX-6 `sony-arw.ARW`, 4928×3276 (16.2 MP), 16,646,144 bytes, SHA-256 `bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8`. I changed exposure from +0.00 to +0.10 EV and temperature from 4632 K to 5132 K. Recipe `.edits/sony-arw.json` and XMP sidecar were written. After quitting and relaunching the validation bundle against the same disposable support path, Library marked it Edited; opening the editor restored +0.10 EV and 5132 K. The original was not modified; only this `/tmp` copy and its sidecars were written.
- During the combined disposable library session, a process sample showed 1,708,560 KB RSS with eight catalog images including this RAW. It is a single point-in-time observation, not peak memory or a performance claim.

## Scope limits

These checks cover one small JPEG and one copied 16.2 MP Sony RAW with manual Develop edits and one ordinary JPEG export. They do not validate PSD, large catalogs, batch work, GPU throughput, macOS 15 compatibility, broad camera coverage, or pixel-level equality after reimport. No user photo library or existing Tessera app was opened or modified.

## Normal launch handoff

The no-argument user-facing bundle was opened once after validation. Its process command line was only the bundle executable (no fixture paths, fake planner, or other test arguments), and the empty Library displayed “No folder open” / “No images.” LaunchServices created only `Preferences.plist` in the dedicated support directory. The app is left open for the user on that empty Library. The empty view also currently exposes a “Load 20,000 Stub Items” button; it was not activated and is a visible development affordance to address separately.
