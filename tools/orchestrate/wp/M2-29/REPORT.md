# M2-29 implementation handoff

Status: incomplete / FAIL because required changes cross the explicit path allowlist.

## Implemented in this worktree

- `image-core::RgbSource` decodes JPEG, PNG, 8/16-bit TIFF and float TIFF to upright planar f32 linear Rec.2020. Embedded RGB ICC is authoritative, absent ICC assumes sRGB. Invalid ICC fails rather than silently guessing. EXIF orientation is consumed exactly once.
- Default feature `imageio` enables macOS HEIC/HEIF through an ImageIO-to-lossless-TIFF bridge in color-mgmt. Builds without that feature/platform return Unsupported. This path handles the primary image, not HDR auxiliary gain maps.
- The historical `RawImage` source wrapper now owns either shared CFA or shared RGB, without a fabricated CFA plane. RGB seeds the Demosaic checkpoint in host and resident GPU paths, bypassing raw decode/linearization/demosaic/denoise. RGB source pages retain f32 precision. Existing working-space CAT white balance operates relative to D65, not camera WB. Existing Kelvin-valued settings/UI remain unchanged; this is not a new Lightroom-style zero-centred temperature slider contract.
- Develop sessions accept RGB. A JPEG session regression verifies non-black rendering and persistence of an exposure edit plus top-level `source_kind: "rgb"`.
- FFI export/print decoding shares RgbSource rather than assuming sRGB. RGB export uses the existing CPU export path; CFA-specific streaming GPU export bands explicitly decline RGB. Resident viewport rendering is supported and tested against pipeline-cpu for repeated tone edits, tolerance <= 3/255 per channel.
- Recipe source_kind uses the existing flattened unknown-members extension. Develop saves own only that member and preserve other unknown members. Engine-api is unchanged.
- ACCEPTANCE.md includes manual JPEG loupe/edit/reopen/export and ICC/orientation steps. Actual GUI screenshot acceptance has not been run.

## Scope blockers / required follow-up

1. `crates/tessera-ffi/tests/develop.rs:489` has `jpeg_images_are_refused`, which explicitly unwraps an error from opening a JPEG. That behavior is the bug this package removes. This test must become a success/render test, but the test path is outside the allowlist. It was not changed or disabled. The exact requested validation chain fails here.
2. `crates/tessera-mcp/src/pixels.rs` has a separate RGB decoder that ignores ICC and EXIF, and omits HEIC. `crates/tessera-mcp/src/preview.rs` dispatches its own RGB CPU path. Agent uses this Console path. These callers need to adopt RgbSource and avoid applying orientation twice. They are outside the allowlist, so agent/MCP parity cannot honestly be claimed.
3. `crates/tessera-ffi/src/catalog.rs:89` sends PNG/HEIC metadata reads to RawSource. Index/import behavior for those formats needs review outside this scope. The JPEG develop test indexes and opens successfully.
4. For typed API support, engine-api needs a backward-compatible `Recipe.source_kind` enum (`raw`, `rgb`, with a legacy/unknown inference policy) and an explicit WB units/relative-offset contract if Lightroom-style relative sliders are desired. Current implementation uses the extension field and existing Kelvin controls, not a schema change.

## Verification

- Required exact command was executed with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-29`. It fails only at the obsolete JPEG refusal test in the run observed. See verification.log.
- An earlier diagnostic run excluding only `jpeg_images_are_refused` passed the requested four-crate release suite (293 passed, 24 ignored). The latest diagnostic run, serialized with `--test-threads=1`, reached FFI fallback tests but hit the three-second callback timeout in `missing_jpeg_returns_pending_then_callback_and_cached_bytes`. That test passes when rerun alone (fallback-retry.log). The current tests-excluding-obsolete.log retains the timeout, not the earlier green run. Neither run is presented as passing the exact required command.
- pipeline-cpu release suite: 135 passed, 1 ignored. See cpu.log.
- image-core RGB decoder unit tests: 9 passed, including synthetic AdobeRGB JPEG ICC, JPEG/TIFF orientation, adjacent 16-bit TIFF samples, float HDR TIFF and actual HEIC decode.
- Native ImageIO unit tests: 2 passed (HEIC transcode, invalid input rejection).
- Clippy with the exact requested crate/target flags and -D warnings passes. cargo fmt --check passes.
- `(cd apps/mac && ./build-ffi.sh && swift build)` passes. Build-generated tracked FFI binding changes were restored because their paths are outside the allowlist. The build emitted a pre-existing deployment-target linker warning for blake3_neon.o.
- No commits or pushes. Only allowed source/report paths remain modified. Cargo.lock additionally resolved the already-declared vector workspace member's dependencies while adding TIFF support.
- Repeated verification exposed an order-dependent export cancellation test: it checked that exactly one result succeeded, then assumed result zero was that result. Updated that test, within the allowed export path, to inspect the successful result without weakening cancellation, file-count, ICC or XMP assertions. The final required run passes export and fails at the obsolete JPEG refusal assertion.
- Final required-chain rerun used `RUST_TEST_THREADS=1` to avoid concurrent GPU test contention, retaining the exact requested command/flags. It exits 101 at `jpeg_images_are_refused`. Clippy and formatting were independently rerun successfully after the final code edits.

The HEIC fixture is a synthetic 32x24 solid-colour image generated from heic-input.png using macOS `sips -s format heic`; it contains no personal photo data.
