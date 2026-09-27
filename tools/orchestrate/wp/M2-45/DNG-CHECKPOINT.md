# M2-45 checkpoint: developed float DNG core

Overall WP result: FAIL (minimum milestones 1–4 are not all complete).

## Existing green milestones retained

- AVIF: existing commit 331cca8, 8/10/12-bit SDR, ICC/CICP, auxiliary alpha.
- JPEG XL: existing commit 6356474, lossless 8/16-bit sRGB through zune-jpegxl. Lossy libjxl and non-sRGB JXL remain deliberately deferred, with the precise rationale in docs/13-licensing.md.
- Round-one JPEG size budget and watermark work retained.

## Added in this checkpoint

- `export::Format::Dng`, using the existing `merge::dng::write` float32 LinearRaw writer, not an independently duplicated DNG implementation.
- Developed full-resolution CPU render in linear Rec.2020 D65, followed by the existing orientation/resize/sharpen pipeline. The selected document-encoded ICC space and render-scale hint are not applied to DNG.
- XYZ-to-Rec.2020 ColorMatrix1, unity AsShotNeutral, no integer quantization in the DNG codec.
- Rebuilt descriptive XMP, removing foreign development settings to avoid double-applying baked edits. Ordinary metadata policy remains applied to embedded and sidecar XMP.
- Explicit rejection of DNG watermarks until watermark colour conversion to the linear output is implemented.
- CLI `--format dng --bit-depth 32`, FFI JSON `{"format":"dng","bit_depth":32}`, and MCP's existing DNG format routed to the shared engine.
- Tests observed failing before implementation: missing Format::Dng, CLI format rejection, FFI JSON format rejection, MCP unsupported format, preserved Camera Raw exposure, and missing watermark rejection.

## Verification

Ran the full user-requested command with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45`:

    cargo test -p export -p tessera-ffi -p tessera-cli -p tessera-mcp --release && cargo clippy -p export -p tessera-ffi -p tessera-cli -p tessera-mcp --all-targets -- -D warnings && cargo fmt --check && cargo deny check licenses && (cd apps/mac && ./build-ffi.sh && swift build)

Final command exit: 0. Evidence: `tools/orchestrate/wp/M2-45/dng-gate.log` (local ignored log).
Parsed Rust results: 260 passed, 0 failed, 12 ignored across 56 test-result summaries. Ignored fixture/benchmark tests were not claimed as executed.
Clippy, formatting and license checks passed. UniFFI generation and Swift build succeeded. Existing native LibRaw compiler warnings and the macOS deployment-target warning from blake3_neon remain visible in the log.

Also ran `cargo run -p tessera-cli --release -- export --help`: exit 0, DNG and float32 help text present. Evidence: `export-help.txt` and `export-help-build.log` beside this report.

DNG verification checks exact decoded float samples against the developed linear render through `raw_decode::linear_dng`, and independently checks LibRaw open/unpack acceptance. The LibRaw Rust wrapper does not expose linear RGB sample buffers. This is NOT claimed as an independent LibRaw pixel comparison. The synthetic DNG codec regression uses an RGB render source, not a real-camera fixture.

## Still required

Milestone 3 is only partial:
- Original + XMP copy mode.
- Embedded-original toggle/payload.
- DNG 1.6 tags (the reused writer currently declares DNG 1.4).
- LibRaw RGB pixel comparison and additional real-raw/orientation coverage.
- Optional 16-bit/lossy DNG are not implemented; only float32 is exposed.

Milestone 4 is not complete:
- Copyright + contact / all except camera policies.
- Person/location removal and keyword hierarchy controls across output metadata.
- Low/standard/high sharpening strength wiring. Existing screen/matte/glossy sharpening remains wired at its existing fixed strengths.

Milestones 5 and 6 remain:
- PQ/HLG HDR output and interoperable JPEG gain maps with reconstruction tests.
- Structured host post-export actions, persisted Export with Previous, and multi-preset export orchestration.

No UI was added. Binding regeneration produced no tracked API diff because export configuration travels as JSON. No changes outside the requested tracked-file allow-list were made.
