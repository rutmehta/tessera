# B5-17a handoff: Photo Restoration in Neural Filters

Branch `wp/B5-17a`, based on origin/main `aec3738e` (M5-32 merged as 39d598e8). No rebase needed at push time.

## Commits

- `81eba5e4` test (RED): `crates/tessera-ffi/tests/document_restoration_ui.rs` (new), catalogue count in
  `document_retouch_ui.rs`, Swift `DocumentRetouchTests` (NeuralKind, model id, catalogue, re-edit). RED was a
  compile failure on the missing `NeuralFilterKind::PhotoRestoration`.
- `9d7c6fb8` feat: implementation, regenerated `apps/mac/Sources/TesseraFFI/TesseraFFI.swift`, two test fixes
  (`@MainActor` on the new Swift test; `testTheStubNeedsTheEngine` now expects every `NeuralKind`).
- This handoff and `ACCEPTANCE-STEPS.md` (steps 480–484, for the integrator to paste under
  `## B5-17. Photo Restoration, painting into channels and alpha display`).

## What changed

- `retouch.rs`: `NeuralFilterKind::PhotoRestoration` (`neural/photo_restoration`, "Photo Restoration", backend
  `DRUNet (denoise only)`). `neural_filters()` matches catalogue entries to kinds by name, so the `zip` no longer
  drops the fourth entry. `neural_filter()` checks params against the catalogue before any model load: known keys
  only, plus Skin Smoothing's `faces`, and finite values in range. It then loads DRUNet through `load_cached`
  (weights first) and dispatches to `RasterFilterOperation::PhotoRestoration`. New layer and smart-from-pixels go
  through `neural_id`. The DRUNet `used_by` is now `JPEG Artifact Removal, Photo Restoration (DRUNet)`. M5-32 hunks
  in `filters.rs` were not touched.
- Swift: `NeuralKind.photoRestoration` with its `filterId`, the FFI mapping both ways, and
  `RetouchModelDownloads.modelId(for:)` → `enhance/drunet-color`. The sheet's list chip and `missingModel` now use
  that shared lookup; the two duplicate switches are gone. The sheet itself needed no change: the slider and the
  limitation caption come from the catalogue. Re-opening from a smart filter row already works through
  `NeuralKind(filterId:)`. The stub backend gets the engine catalogue automatically.
- `RetouchSelfTest` step 7 (`restoration`): checks the listing, the one control, the limitation and the DRUNet
  requirement. Without DRUNet, every output fails with the missing-weights message and changes nothing: no history
  node, no new layer and no new smart filter, first on the pixel layer and then after Convert for Smart Filters.

## Gates (worktree root, CARGO_TARGET_DIR=~/.cache/tessera-target/B5-17a)

- `cargo test --locked --release -p tessera-ffi`: all 38 test binaries ok. The new `document_restoration_ui` passed
  4/4, and `document_retouch_ui` (9) and `document_restoration` (1) passed.
- `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: clean.
- `cargo fmt --all -- --check`: clean.
- `tools/orchestrate/swift-gate.sh`: **SWIFT GATE OK** (704 tests, 3 skipped, 0 failures).

## Not run / remaining

- The `xcodebuild` app build and the `--retouch-selftest` run (`open -g -n … --new-document
  --retouch-selftest=<dir>`) were not run here because the machine is loaded. Expect the new
  `check Photo Restoration …` and `check restoration on … ok` lines.
- On-screen checks 480–484, which need a person or a screen-capture pass: the list row and limitation caption, and
  the downloads-off message with Settings ▸ AI. With DRUNet installed: one node per output with undo, re-opening the
  smart filter with its value, and smart objects. With DRUNet installed, the self-test skips the missing-weights
  checks and logs that it did.
- Behaviour change to note: params are now validated before the weights check for all neural filters. Unknown keys or
  out-of-range values fail with `<Filter>: <key> must be a number in a..b` or `<Filter>: unknown control <key>`
  instead of the missing-weights error. The engine rejected the same values at evaluation already.
