# M5-32 round 3: partial implementation, not ready to merge

## Gate result

The requested exact chained gate was executed with
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32`.
The final completed run exited 101 in the release tests:

```
crates/tessera-ffi/tests/document_restoration.rs:45
unsupported: neural/photo_restoration
```

Raw output: `round3-final-gate.log`. Earlier runs exposed the adjustment JSON
fixture defaults and a new channel test's Arc move; both were corrected. The
final failure is the missing restoration bridge, not those earlier failures.
No test was ignored or weakened to conceal this blocker.

The later stages were also run independently:

- Full requested all-target Clippy command: PASS (`round3-clippy-final.log`).
- `cargo fmt --check`: PASS (`round3-fmt-final.log`).
- `cargo check --workspace`: PASS (`round3-check.log`).
- `(cd apps/mac && ./build-ffi.sh && swift build)`: PASS (`round3-mac.log`).
  Swift bindings regenerated and expose `selectedAreas` and `photoRestoration`.
- FFI channel-display regression: PASS, directly executing the release test
  binary compiled by the final gate (`round3-channel-direct.log`).
- Adjustment fixture regression: 2 PASS after fixture update
  (`round3-ffi-direct.log`).
- Focused CAF, ROI, adjustment serde, PSD and GPU tests: PASS when directly
  executing their compiled release binaries (`round3-focused-direct.log`).
- Channel contract/compositor/brush/MCP release test binaries: PASS
  (`round3-channel-stacks-direct.log`).
- Both 18 MP ignored Remove benchmarks explicitly executed: PASS, 757.424 ms
  CPU and 807.665 ms Auto fallback; texture MAE 0.000000. These are synthetic
  periodic-texture fixtures, not real photographic quality measurements.
  See `remove-performance.md` and `round3-perf-direct.log`.

A queued duplicate focused Cargo test was terminated after the full gate
returned 101 to avoid deployment-target rebuild churn. The channel test was
then run from its already compiled release binary. Full-gate results above
come from the completed gate, not the interrupted extra check.

## What changed in this round

1. PSD Color Lookup filename and dither descriptor mapping, missing adjustment
   constructor fields, tests, and serialized FFI fixture defaults. GPU now
   explicitly rejects enabled lookup dither and Match Color neutralization
   rather than silently ignoring their CPU semantics.
2. Minimal marked AlphaDisplay integration into FFI channel records, including
   selected-area polarity and legacy alpha identity handling. Native/PSD
   persistence and engine summaries were already implemented in round 1.
3. Added engine-api 1.6 contract changelog and compositor documentation for
   channel painting and new adjustment metadata. Existing DocOp/brush/MCP
   channel destination and undo tests pass.
4. Bounded CPU PatchMatch Remove to a tile-aligned stroke ROI plus 128px donor
   margin/dilation and explicit donor areas. Added conservative SSD rejection,
   translation-row fast path, and donor/edge/coverage/tile-coordinate regressions.
5. Minimal marked PhotoRestoration variant and dispatch ID in FFI, actionable
   unloaded-DRUNet error in ml-filters, and missing-weights atomicity regression.
   This is only partial: the shared adapter still rejects the identifier.

## Remaining blockers and exact ownership requests

- `crates/filters/src/compositor_adapter.rs` is outside the allowed paths. Its
  neural parameter decoder, evaluator, and model loader recognize only skin,
  colorize and JPEG removal. The owner must add `neural/photo_restoration`,
  denoise-only parameters, a pinned PhotoRestoration model slot/explicit loader,
  and clean missing-weights errors. Do not route around this shared adapter or
  disguise Photo Restoration as JPEG removal. This omission causes the final
  gate failure.
- `crates/compositor/src/resident/adjustments.wgsl` is excluded and owned by
  M5-31. Its shared adjustment evaluator must implement spatial lookup dither
  and Match Color neutralization, with matching payload packing in the allowed
  program/specialize files. Current rejection is honest but does not satisfy
  the requested GPU behavior. Tests verify rejection, not parity for enabled
  options. Existing default-option GPU behavior still passes.
- Exact Photoshop neutralization equivalence remains unestablished; current
  CPU implementation is native gray-world Lab chroma removal.
- The strict real-photo 18 MP benchmark requirement remains unverified; the
  measured benchmark is an 18 MP synthetic texture with a 300x300 hole.

## Scope and hygiene

All changed/untracked paths were checked against the supplied allowlist:
no out-of-scope paths. No commits or pushes. The pre-existing brief edit is
preserved. FFI integration hunks are marked `// M5-32`. Generated UniFFI Swift
retains generator-standard trailing whitespace (git diff --check reports five
added generated lines); Rust formatting and the Swift build pass.

No Kanban task ID was available: initial kanban_show returned `task_id is
required`. Consequently no board completion/block transition was possible.

RESULT: FAIL restoration adapter and GPU shader integration require excluded paths; all five acceptance items are not complete.
