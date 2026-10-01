# B5-46 — Lightroom import report UX

Branch: `wp/B5-46`. Base: `445131fc`. Local commits only.

- Tests: `a123cc57` — `test(B5-46): cover import report paths, ETA and fidelity failures`
- Fix: `985432be` — `fix(B5-46): complete Lightroom report grouping, ETA and fidelity diagnostics`

## Changes

- The FFI summary keeps B5-29c grouping and counts, preserves per-image entries for both duplicate-ID notes and “imported as unedited”, and exposes full catalog paths for develop-warning examples. Example lists are capped at five. Existing sheet rows and AX report warnings consume the same summary.
- Already-grouped CLI records retain only their first image ID, so those groups show that one representative path with the full image count. No attempt is made to guess other affected paths.
- The existing progress strip shows a step ETA from processed/total and monotonic elapsed time. Writing and indexing reset the estimate independently; empty, unstarted and complete steps show no ETA. Publication is limited to once per 500 ms on the main actor, including phase changes. Every import/resume starts a fresh estimator.
- `document.import.report.fidelity` now also exposes failures before samples exist. Existing sample diagnostics remain verbatim, including renderer failure reasons. Markdown retains sample failure reasons even when no sample could be compared. No new UI chrome.

## Test-first evidence

- `a123cc57` adds the regression tests before the fix.
- Rust red: the fixture summary returned 3 examples where 5 were required.
- Swift red: the ETA model was absent at compile time. After adding it, the 13 import tests exercised the two remaining failures: missing fidelity AX field before samples exist, and lost renderer reason in an all-failed Markdown report. The ETA test passed.
- Fixtures cover a 7-image warning group capped at 5 paths; two separate entries for each duplicate-ID reason and unedited imports; grouped AX counts and five example paths; 25/100 processed in 10 seconds giving a 30-second ETA; the 500 ms throttle boundary and phase reset; renderer errors with and without samples.

## Validation

- `cargo test --release -p tessera-ffi`: **563 passed, 0 failed, 29 ignored**, across 51 test-binary/doc-test results.
- `cargo clippy --all-targets -p tessera-ffi -- -D warnings`: passed.
- `cargo fmt --all -- --check`: passed.
- `cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK**, exit 0. XCTest: **894 executed, 3 skipped, 0 failures** (216.613 seconds). Swift Testing: **5 passed** in 2 suites. All **13 Lightroom import tests passed**, including all 3 AX tests. No window-capture failure.
- All commands used `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-46`; builds were run serially.

No real Lightroom catalog was opened. No manual GUI launch or screen capture was performed; accessibility checks use fixture reports in the background test harness. `board.json` and `Cargo.lock` were not modified.
