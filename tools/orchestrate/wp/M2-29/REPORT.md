# M2-29 implementation handoff

Status: round 2 incomplete / FAIL. Decoder, FFI regression, and metadata-adapter changes are verified; PNG/HEIC/HEIF catalog admission requires an additional allow-list change.

## Latest retry verification

- Current attempt independently reproduced the exact HEIC Console regression with `RUST_TEST_THREADS=1 cargo test -p tessera-mcp --release --test console heic_console_describe_edit_and_export -- --exact`, then ran the full user-required chained gate. The chain exited 101 at the same HEIC admission failure; current evidence is `current-gate.log`. Clippy/fmt/Swift were not reached in this attempt. JPEG develop/edit persistence and the unchanged slider-starvation regression passed. Re-inspection confirmed the scanner predicate is still outside the explicit allowed paths. No implementation changes were made, and permission for `crates/index/**` is still required. The runtime has no task ID, so `kanban_show()` could not resolve a board task.
- Re-read the scanner and Console admission path. `Core::scan` skips PNG/HEIC/HEIF at `crates/index/src/lib.rs:202`, because `is_image` at line 797 omits their extensions, before either metadata adapter can run. `Console::open_image` uses that scanner and fails at `crates/tessera-mcp/src/console.rs:67`. The current permitted paths still exclude `crates/index/**`.
- Ran the exact requested full command again with the existing external `CARGO_TARGET_DIR`. It exited 101 at `heic_console_describe_edit_and_export` (`tests/console.rs:27`), with `Unsupported { what: "image format is not indexable" }`. See `latest-retry-gate.log`. Clippy, fmt and Swift stages were not reached by this retry's short-circuiting chain; their previous independent results below are historical, not new runs.
- JPEG develop/render/edit persistence, PNG/HEIC metadata adapters, direct HEIC preview/export decoding, ICC/orientation parity, and resident RGB tone parity passed in this run. `export_batch_does_not_starve_slider_drag` also passed unchanged.
- No source changes or test weakening in this retry. Permission to edit `crates/index/**` remains necessary to fix the scanner and add scanner regression coverage. No task ID was present for a kanban blocked transition.

## Round 2 implementation and verification

- Replaced only `jpeg_images_are_refused` in the FFI integration suite with `jpeg_opens_renders_nonblack_and_persists_edits`. It renders through attached surfaces, checks non-black histogram and exposure response, commits/flushes, checks `source_kind`, and reopens the engine/session to verify persistence. Other existing develop tests are unchanged.
- MCP export source loading and the cached CPU preview path now decode through `image_core::RgbSource`. The shared extension predicate recognizes HEIC/HEIF. ICC and EXIF are consumed once before preview downsampling, linear histograms, critic metrics and export. Scores use cached source dimensions instead of the image crate's HEIC-incompatible header reader. MCP saves record source_kind while preserving other unknown recipe members.
- Added synthetic AdobeRGB/EXIF JPEG parity tests for export-source pixels, display and scene-linear output. Added direct ImageIO HEIC preview/export-source tests. Added a public Console HEIC describe/edit/export regression, intentionally left enabled: it exposes the out-of-scope scanner blocker below.
- FFI EmbeddedMetadata now classifies rendered formats with RgbSource::recognizes instead of routing PNG/HEIC through LibRaw. PNG and HEIC metadata tests failed with LibRaw error -2 before the change and pass afterward. Metadata remains header-only; it does not decode full pixels during scans.
- **Remaining blocker:** `crates/index/src/lib.rs:797-816` (`is_image`) excludes PNG, HEIC and HEIF. `Core::scan` rejects them before metadata hooks run (`:202`). `Console::open_image` therefore returns `Unsupported { what: "image format is not indexable" }`; FFI indexing likewise cannot discover these originals. No public single-image admission API exists to use instead. Fix requires permission for `crates/index/**` and scanner regression tests there. No SQL bypass, renamed copy, ignored test, or out-of-scope edit was introduced.
- The exact required gate was run twice with the exported `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-29`. First run stopped at the pre-existing 3-second preview callback timeout in `missing_jpeg_returns_pending_then_callback_and_cached_bytes`. That test passed alone and in the second full gate. Second gate passed image-core, pipeline-gpu, export and tessera-ffi tests, then failed on the new Console HEIC admission regression. Logs: `round2-gate.log`, `round2-gate-retry.log`, `round2-fallback-retry.log`.
- `export_batch_does_not_starve_slider_drag` passed in both full gate runs. It was not modified, skipped or weakened.
- MCP's full suite was also run with `--no-fail-fast`: only the HEIC Console admission regression fails; all other executed tests pass (`round2-mcp-full.log`). Direct HEIC decoding and AdobeRGB/orientation parity pass.
- Required clippy command with `--all-targets -- -D warnings` passes after moving the catalog test module to the end of the file. `cargo fmt --check` passes. `(cd apps/mac && ./build-ffi.sh && swift build)` passes independently, because the failed test chain short-circuits before those stages. Logs: `round2-clippy.log`, `round2-fmt.log`, `round2-swift.log`. Generated bindings contained unrelated CFA-denoise API updates and were restored to keep this patch focused.
- No manual GUI/screenshot acceptance was performed. Engine-api remains unchanged. The typed source_kind / relative-WB contract observations below still apply.
- No commits or pushes. All retained changes are within the round-2 allowlist. No kanban task ID was provided by the runtime, so no board lifecycle transition was available.

## Round 1 historical handoff

The sections below describe round 1 and its then-applicable path restrictions; items 1-3 are superseded by the round-2 findings above.

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
