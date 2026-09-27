# M2-45 round 2 checkpoint

## Implemented: priority 1, AVIF

- `export::Format::Avif(AvifOptions)` uses rav1e 0.8.1 (BSD-2-Clause)
  directly, with full-range identity GBR, 4:4:4, 8/10/12-bit quantization
  from float, quality 1–100 and speed 1–10. Quality 100 is not advertised
  as lossless. No external executable, assembly toolchain or GPL codec.
- HEIF still-image container with primary-image property associations,
  ICC `prof`, truthful CICP `nclx`, XMP MIME item plus `cdsc` association,
  optional straight-alpha AV1 auxiliary image plus `auxl` association.
  ProPhoto CICP primaries/transfer are unspecified. Rec.2020 transfer is
  unspecified because the built-in ICC uses gamma 2.4, not the video OETF.
- Public `encode_avif` accepts document-encoded RGBA floats with independent
  alpha coverage. The existing develop renderer is RGB-only, so normal
  CLI/FFI/MCP developed-photo exports remain opaque, exactly like existing
  PNG/TIFF exports. Source-image transparency preservation in those hosts
  is not implemented. Watermark transparency is composited before encoding.
- CLI: `--format avif --bit-depth 8|10|12 --quality 1..100
  --avif-speed 1..10`. The bit-depth flag also exposes existing TIFF16.
- FFI: backward-compatible `format: "avif"`, `bit_depth`, `quality`,
  `avif_speed` in the existing export/preset settings JSON. Default speed 6.
  Existing settings JSON remains accepted. No UI changes or ABI record change
  is needed because these settings already cross UniFFI as JSON.
- MCP's existing AVIF quality record now maps to the same encoder with
  default 8-bit/speed 6; it does not expose AVIF depth/speed yet.
- Centralized public `Format::extension()` removes duplicated CLI and MCP
  extension matches, so later format additions will not repeat that blocker.
- Existing resize, sharpen, watermark, orientation, metadata policy,
  cancellation and no-clobber publication paths are reused. Invalid AVIF
  settings and JPEG-only byte-budget combinations are rejected.
- Samples must be finite; dimensions, pixels and XMP size are bounded.
  Cancellation is checked between encoder calls and rows; a rav1e frame
  encode itself is not interruptible. Encoding runs one rav1e worker per
  admitted export, avoiding nested encoder thread pools.
- AVIF DPI/EXIF density is not written. XMP is embedded and also written as
  the existing sidecar. This slice adds no new EXIF metadata writer.

## Verification

Final exact requested gate: **exit 0**. All four release test suites, all-target
Clippy with warnings denied, workspace formatting, license check, macOS FFI
build/binding regeneration and Swift build passed. Verbatim selected output is
committed in `round2-verification.txt`; full local logs listed below are ignored
to avoid committing repeated third-party compiler diagnostics. Bindings were
regenerated with no tracked output change because the settings use JSON.
The CLI's actual `export --help` output is in `round2-cli-help.txt`.
`CARGO_TARGET_DIR` remained `/Volumes/betterSSD/tessera-cache/target/M2-45`.
The existing LibRaw compiler warnings and blake3 deployment-target linker
warning did not prevent the gate from passing.

Two intermediate gate issues were resolved:
- License check included rav1e's cfg(fuzzing)-only libfuzzer-sys dependency.
  Its version-specific permissive NCSA exception is documented in
  `docs/13-licensing.md`; normal/build app dependency trees exclude it.
- Existing print test regenerated its expected ICC profile. The failure's
  arrays differed only at byte 35 (creation-time seconds, 45 versus 46).
  It now compares against the exact fixture bytes written, retaining full
  byte equality instead of depending on two calls occurring in one second.
  No production printing behavior or assertion strength was changed.

- Initial FFI red: `avif-red.log`, unknown `avif` format before implementation.
- Codec red: `avif-codec-red.log`, missing codec module.
- MCP red: `avif-mcp-red.log`, explicit unsupported-format response before wiring.
- Focused cross-crate green: `avif-green.log`.
- `avif-precision.log`: macOS ImageIO independently decodes every bit depth
  and all four profiles, verifies RGB under partial alpha and alpha coverage,
  tiny/odd sizes, and a 12-bit pair that collapses to one value at 8 bits.
- Tests parse BMFF boxes and verify depth, CICP, ICC color transforms and
  alpha associations. The FFI test runs actual batch exports at every depth.
  A developed CFA orientation test exercises AVIF's shared orientation path.
- Read-only independent review found no blocking codec defect. Its identified
  precision/partial-alpha test gaps were subsequently covered. Host alpha
  preservation and MCP depth/speed remain explicit limits above.
- Full gate result is recorded separately in `round2-gate.log`; only a final
  `GATE_EXIT=0` establishes success. The first unlogged full-gate attempt
  passed tests then failed one new-test Clippy lint (`chunks_exact`); fixed
  using `as_chunks` without suppressing the lint or weakening assertions.

## Not implemented; overall work package remains FAIL

2. JPEG XL lossless via zune-jpegxl, format/CLI/FFI wiring and ICC/round trips.
   Vendored libjxl was not attempted. There is no claim that it failed to
   build, and no lossy JXL implementation or libjxl-ffi crate.
3. DNG linear export, original+XMP copy, original embedding, DNG 1.6 tags
   and actual LibRaw round-trip validation.
4. Expanded metadata policies and privacy filters, hierarchical keywords,
   output-sharpening low/standard/high. Existing screen/matte/glossy output
   sharpening remains wired, without a new intensity setting.
5. HDR PNG/AVIF/JXL PQ/HLG and ISO/Adobe gain-map JPEG reconstruction.
6. Structured host post-actions, persisted Export with Previous, multi-preset
   execution. Existing presets and Finder hint remain unchanged.

Only AVIF was added to `export::Format` in this checkpoint. Unsupported JXL
and DNG variants were not added as stubs. Stop after this verified priority
item rather than committing incomplete later codecs. Prior round's JPEG
size limit and watermarks remain intact.

## DNG investigation for the next slice

The public `merge::dng::write` float writer is reusable, but hardcodes DNG
version/backward version 1.4 and lacks extensible tags/original embedding.
The current managed export render is output-encoded; it must not be tagged
as scene-linear DNG. `pipeline_cpu::render_linear_scaled` and a Rec.2020
synthetic-camera matrix are the relevant starting point for baked linear
pixels. Existing resize/sharpen clamps highlights, and AI masks currently
return tone-mapped pixels: address these explicitly rather than silently
clipping HDR into a purported raw file.

The existing custom linear-DNG reader accepts version 1.4 and float32 only.
The current LibRaw wrapper exposes the u16 CFA raw_image pointer, not float
RGB. Open/unpack success alone is not a pixel round trip. A real 1.6 DNG
validation strategy must be worked out within the allow-list or request a
narrow decoder/FFI extension; do not claim the existing merge tests provide
that validation.
