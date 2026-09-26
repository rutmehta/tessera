# M2-45 partial implementation, not acceptance-complete

## Allow-list decision required

`crates/tessera-mcp/src/exports.rs:116-120` exhaustively matches
`export::Format::{Jpeg,Png,Tiff}` to select an extension. The CLI depends on
`tessera-mcp`. Extending the existing export format enum with AVIF/JXL/DNG
therefore requires updating that match, but `crates/tessera-mcp/**` is not in
this package's allowed paths. It has not been modified. Please allow at least
`crates/tessera-mcp/src/exports.rs` for codec integration. This is a source-level
constraint found by inspection, not an observed compiler failure from a
completed codec implementation. An alternate second format-override field
could avoid changing this match but would introduce contradictory format
settings instead of extending the existing API cleanly.

## Implemented slice

- JPEG `max_file_bytes`: an upper quality bound, at most eight encodes (full
  quality then at most seven binary-search trials), including ICC and embedded
  XMP. The actual fitting bytes are retained. Unattainable limits fail without
  publishing an image or sidecar. Quality optimality is not promised because
  encoded size is not strictly monotonic across JPEG sampling transitions.
- Deterministic text and PNG-alpha watermarks after resize/output sharpening.
  Explicit font files, relative short-edge size, RGB, opacity, nine anchors,
  inset and text rotation. Premultiplied graphic resampling avoids fringes.
  Asset and raster allocations are bounded. Colors are interpreted in the
  document's encoded space; graphic embedded-profile conversion is not done.
- CLI `--max-file-bytes`, `--watermark <JSON path>` and backward-compatible
  FFI export-options JSON fields. Existing options deserialize to no limit and
  no watermark. No UI changes.
- Tests cover JPEG metadata retention/budget/impossible-output cleanup,
  watermark deterministic alpha/placement/text/rotation/cancellation and the
  actual resized/sharpened export path, plus FFI JSON and CLI flag parsing.

## Not implemented in this attempt

AVIF/JXL codecs, vendored libjxl, DNG export/original copy/embedding, HDR
PQ/HLG/gain maps, expanded metadata policies, sharpening low/standard/high,
post-processing action records, persisted previous settings, and multiple
presets per export. Existing screen/matte/glossy sharpening remains unchanged.
No claim is made that libjxl failed to build: a vendored build was not attempted,
so the permitted lossless-only fallback has not been invoked.

## Verification

### Current invocation

Reconfirmed the restricted downstream exhaustive match at
`crates/tessera-mcp/src/exports.rs:116-120` and direct CLI dependency.
No implementation files were changed in this invocation. The exact chained
gate completed with exit 0, including license checks, regenerated bindings,
and Swift compilation; see `current-gate.log` (`GATE_EXIT=0`). A first foreground
attempt timed out after 420 seconds during compilation; the background retry
completed normally. `git diff --check` passed and the programmatic path check
found no changed/untracked files outside the allow-list.

The gate passing does not complete the missing features listed below. Scope
approval for `crates/tessera-mcp/src/exports.rs` remains required for extending
the existing format enum. No Kanban task ID was provided in this environment,
so the board lifecycle could not be updated (`kanban_show` reported no task ID).

### Retry verification

The retry rechecked the exhaustive match in `tessera-mcp/src/exports.rs`
and the CLI's direct dependency on that crate. The allow-list conflict above
remains. No implementation files were changed during this retry.

The exact full chained gate was rerun successfully: `retry-gate.log` ends
with `GATE_EXIT=0`, `licenses ok`, and the successful Swift build. An initial
foreground invocation hit the tool's 420-second timeout during compilation;
the subsequent background invocation completed. The prior preview starvation
failure did not reproduce in the completed retry. This does not establish its
root cause or constitute a fix. `git diff --check` also passed.

This successful gate verifies only the existing partial implementation, not
the absent codec, DNG, HDR, metadata, sharpening-strength, or workflow features.
Approval to edit `crates/tessera-mcp/src/exports.rs` is needed to extend the
existing format enum without breaking its downstream exhaustive match.

### Previous attempt

The exact requested chained gate was run and failed at
`export_batch_does_not_starve_slider_drag` in `crates/tessera-ffi/tests/develop.rs:1198`.
Both the intermediate and final runs failed its adaptive-preview L2 assertion
(the final run had zero L2 frames). The root cause has not been established.
No test was weakened or skipped. See `gate.log` and `gate-final.log`.

Separately, the remaining gate commands all exited successfully:
`cargo clippy -p export -p tessera-ffi -p tessera-cli --all-targets -- -D warnings`,
`cargo fmt --check`, `cargo deny check licenses`, `apps/mac/build-ffi.sh`, and
`swift build`. The binding regeneration produced no tracked binding changes
because the additions travel inside the existing settings JSON. Swift linked
successfully with an existing deployment-version warning for blake3_neon.o.
See `build-checks.log`.

Focused size-limit and watermark integration tests pass (`focused-final.log`).
The full FFI export integration test target and CLI tests pass, and a real
`tessera export --help` lists both added flags (`bridge-cli.log`). TDD failure
evidence is in `size-limit-red.log` and `watermark-red.log`.
`git diff --check` and a programmatic allow-list check pass. CARGO_TARGET_DIR
remained `/Volumes/betterSSD/tessera-cache/target/M2-45` throughout.

This package must remain FAIL/incomplete even if the partial implementation's
build checks pass. No commit or push was performed.
