# B5-16 review-fix evidence, 2026-09-27

Product commit: `8184da1`; resolved baseline merge: `ba1eaa7`, incorporating
origin/main `9efa76f`. No Rust production or generated FFI changes in the fixes.
Machine A subsequently published newer export/Review integration on main; that
newer source is not included in this gate and requires A integration validation.

- `legacy-red.log`: initial PNG fixture setup failed; not a product regression.
- `legacy-red-corrected.log`: corrected warm CGImage fixture; old behavior fails
  three intended assertions (neutralize stays enabled, chroma stays zero,
  saved/reopened model remains neutralized). No setup failures.
- `match-green.log`: compatibility fix, 40 analysis/JSON tests pass. Includes
  native save/reopen, explicit disable, undo, save/reopen persistence, missing
  source behavior and modern frozen-stat toggles without pixel reads.
- `full-swift.log`: `MACOSX_DEPLOYMENT_TARGET=15.0 swift test -c release
  -Xswiftc -enable-testing --package-path apps/mac`; 424 XCTest cases, one existing
  skip, zero failures, plus five Swift Testing tests. Strict Shell layout and
  Document inspector checks run without an expected-failure waiver.
- `runner-tests.log`: `python3 -B -m unittest discover -s
  tools/orchestrate/wp/B5-16 -p test_selftest_runner.py`; eight pass. Covers exact
  completion, prerequisite failure, missing/duplicate/nonzero completion,
  timeout, child crash, unrelated-process survival, default transform inclusion,
  another checkout, aggregated failure exit, and Text/Transform capture request acknowledgement.
- `runner-default-red.log`: test expectation initially omitted macOS `/private`
  symlink resolution; corrected fixture expectation, not a product regression.
- `ffi.log`: successful FFI generation before the focused/full Swift gates.

Full app self-tests and interactive acceptance remain separate. Unit success
must not be interpreted as completed UI acceptance.

- `xcode-review.log`: Xcode Debug build succeeded on the same product source.
- `package-review.log` and `Tessera.app.provenance.json`: fresh release package,
  ad-hoc signatures and provenance verified at `2dc358d` (product `8184da1`).
  Subsequent runner-only changes do not alter the app source or binary.
- Runner now consumes Text/Transform `.req` capture requests, copies a successful
  owned-window PNG to the requested output path and rejects `(no watcher)`.
  A capture failure never acknowledges successful evidence.
