# B5-36 — Lightroom Report and Fidelity accessibility

Branch: `wp/B5-36`. Local commits only.

## Changes

- `ReportStep` takes report data directly, allowing a hosted fixture without exposing the controller's private setters.
- The existing report count grid exposes `document.import.report.summary`, with all eight displayed counts in its AX value.
- `document.import.report.warnings` exposes skipped file names/reasons and unsupported issue categories, reasons, counts, and examples. Unsupported issues are also visible in the sheet.
- `document.import.report.fidelity` exposes photo names, comparison metrics/status, and renderer diagnostic messages in both the Fidelity step and the final Report step when fidelity results exist. The Fidelity step value includes all samples, independent of lazy card realization and the visual filter.
- The existing markdown disclosure starts expanded and contains a selectable, scrollable, read-only native `NSTextView`, identified as `document.import.report.markdown`. It exposes the original markdown as AXValue with the AXTextArea role.
- The native text area uses the matching `Theme.NSFonts.captionMono` token and theme text color.
- Summary, warning, and fidelity groups have explicit static-text AX traits: a generic SwiftUI group accepted identifiers but did not expose its assigned value on this macOS runtime.

## Tests and verification

Two hosted SwiftUI tests cover the report's eight counts, warning details, retained fidelity diagnostics, markdown contents, AXTextArea role, read-only AX setter policy, and failed-render diagnostics in the actual import sheet's Fidelity step.

The fixture uses background windows under the existing prohibited activation policy. It materializes SwiftUI's AX tree through an app-local enhanced-accessibility request and restores the prior value. It traverses Objective-C AX getters because SwiftUI virtual nodes do not formally conform to NSAccessibilityProtocol. The new tests do not capture the screen or activate a foreground app.

- Red: original rendering with the final harness, 2 tests / 2 expected failures (missing summary and fidelity identifiers); no setup failures.
- Green: `swift test -c release -Xswiftc -enable-testing --filter LightroomImport`, 10 tests / 0 failures (2 new hosted AX tests + 8 existing LightroomImport tests).
- First full gate: 891 XCTest tests, 3 skipped, 1 failure in `ThemeLintTests.testViewsUseThemeTokensOnly`; 5 Swift Testing tests passed. Fixed the ad-hoc native font by adding the matching Theme token.
- Final required serial gate: **SWIFT GATE OK** (exit 0). FFI build succeeded; debug Swift build succeeded; 891 XCTest tests, 3 skipped, 0 failures in 178.320 seconds; 5 Swift Testing tests in 2 suites passed. No window-capture failure.

Commands use `PATH="$HOME/.cargo/bin:$PATH"` and `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-36`. Required serial sequence: `cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh`.

No Rust changes; Rust test/clippy/fmt gates do not apply. Neither Cargo.lock nor board.json was changed. Generated FFI bindings have no tracked diff.

## Limits

Hosted AppKit/SwiftUI AX behavior is verified; no installed-app external automation session or visual screenshot review was performed. Existing markdown generation is unchanged, including its omission of individual fidelity rows when no sample could be compared; the sheet's separate fidelity AX content exposes those diagnostics.

## Commits

- `2ec2b737` — test(B5-36): cover hosted Lightroom report and fidelity accessibility
- `1acdacbf` — fix(B5-36): expose Lightroom report and fidelity content to accessibility
- This document is delivered by the subsequent `docs(B5-36):` commit.
