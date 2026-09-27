# M2-45d DNG checkpoint

Overall work package: **FAIL / incomplete**. This run implements the DNG slice
(item 2), not native EXIF/IPTC policy or HDR output. Item 2 is verified below.
No metadata/HDR completion is implied by a green gate.

## Implementation

- Developed export finalization writes DNGVersion 1.6.0.0 and preserves
  DNGBackwardVersion 1.4.0.0. The existing merge writer's float32 LinearRaw samples,
  Rec.2020/D65 ColorMatrix1, unity AsShotNeutral and 64 Mi-pixel limit are retained.
  The appended active IFD reuses existing sample offsets. Merge outputs outside
  the export path remain unchanged.
- Optional OriginalRawFileName (50827) and OriginalRawFileData (50828) embed the
  original data fork. Independent 64 KiB zlib blocks use the DNG big-endian
  length/offset envelope. Empty resource/THM fields are encoded as zero lengths.
  No resource fork or THM companion is included. Sources are nonempty regular
  files, at most 1 GiB, with ASCII basenames. The core stores opaque bytes rather
  than claiming to validate RAW sample contents.
- Compression reads one block at a time, checks cancellation between blocks,
  and never writes to the original. It uses the exporter's existing synced
  temporary-file/no-clobber publication and rollback. A nonblocking open and
  descriptor metadata check prevent FIFO replacement races from hanging export.
- Core `ExportSettings::original_raw` is an optional source path. CLI exposes
  `--embed-original-raw` with `--format dng --bit-depth 32`. FFI/preset and MCP
  export JSON expose `embed_original_raw`, default false. Each host selects the
  current image's source, not a shared batch source. CLI uses single-image waves
  when embedding. FFI remains JSON; there is no new Swift UI or binary layout.
- Embedding refuses reduced metadata/privacy options: the copied original
  necessarily retains private camera, person and location metadata. MCP refuses
  `embed_metadata=false` in combination with embedding.
- The bounded linear-DNG reader accepts the implemented DNG 1.4–1.6 subset,
  rejects unsupported backward versions, and skips OriginalRawFileData without
  allocating it against the 2 MiB tag budget. Type/range/duplicate validation
  still applies to the skipped tag.
- `flate2` and `libc` become direct export dependencies. Both were already in
  Cargo.lock. No codec SDK, ExifTool implementation, or new GPL dependency is
  bundled.

## Tests and review

- `dng.rs`: exact tag IDs/types/counts, version/backward-version, float sample
  representation, white level and illuminant; exact engine sample round-trip.
- `dng_libraw.rs`: independent processing through the vendored LibRaw C API into
  linear 16-bit sRGB, compared to the engine render transformed from Rec.2020.
  The non-neutral ramp fixture stays inside sRGB gamut; maximum absolute channel
  error must be below 0.003. No empirical exposure/white-balance fit is applied.
  An initial fixture included negative sRGB red and failed due to LibRaw's
  clipping. That was a test-fixture issue, not an engine bug; the final fixture
  explicitly asserts in-gamut expected values rather than loosening tolerance.
- `dng_original.rs`: independent envelope decompression at 64 KiB boundaries and
  with a >2 MiB incompressible original; source bytes preserved; privacy/wrong
  format rejected. ExifTool extraction is also compared byte-for-byte when the
  executable is installed (it is installed in this environment).
- `dng_safety.rs`: FIFO nonblocking rejection, backward-version rejection, and
  malformed/duplicate/out-of-range OriginalRawFileData rejection.
- CLI, FFI and MCP tests exercise the new control and actual outputs. The CLI
  test exports two distinct sources to guard against a batch-wide source path.
- Read-only review identified backward-version and FIFO issues. Regression
  tests reproduced both before their fixes. A second independent review returned
  `passed=true`, no security concerns and no logic errors. Static added-line
  security scan found no issues. One suggested malformed-tag regression was
  added; host tests still check the carrier filename while byte-exact extraction
  is covered by the core integration test.

Red evidence: `dng-version-red.log`, `dng-original-red.log`,
`dng-large-red.log`, `dng-host-red.log`, `dng-cli-red.log`,
`dng-mcp-red.log`, `dng-safety-red.log`.

## Final gate

The exact requested command passed, exit 0, in `dng-full-gate.log`:

```
cargo test -p export -p tessera-ffi -p tessera-cli -p tessera-mcp -p sidecar --release && cargo clippy -p export -p tessera-ffi -p tessera-cli -p tessera-mcp -p sidecar --all-targets -- -D warnings && cargo fmt --check && cargo deny check licenses && cargo check --workspace && (cd apps/mac && ./build-ffi.sh && swift build)
```

- Full gate: 521 Rust tests passed, 0 failed, 18 ignored; Clippy with warnings
  denied, formatting, licenses and workspace check passed. Bindings regenerated
  without a tracked diff. Swift build passed (47.15s).
- `dng-final-focused.log`: all 7 DNG tests passed, including the malformed-tag
  regression added after the full gate had already compiled that test binary.
  This is a supplementary run, not 7 additional unique tests on top of 521.
- Final `cargo fmt --check` passed. Source diffs passed `git diff --check`
  before adding the verbatim diagnostic logs; the logs retain tool-emitted
  trailing whitespace and blank lines. Scope verification found no modified
  tracked/untracked paths outside the user's allowlist.
- Existing native LibRaw warnings, Swift unused-value warnings in unrelated
  document self-tests, and the blake3_neon deployment-target linker warning
  remain; no warning suppression was added.

CARGO_TARGET_DIR remains `/Volumes/betterSSD/tessera-cache/target/M2-45d`.
An earlier baseline attempt was interrupted during ongoing work; it is not
claimed as a successful final gate.

## Remaining work

1. Native EXIF/IPTC extraction, preservation, policy filtering and format
   carriers in JPEG/TIFF/PNG/AVIF/JXL/DNG, with independent per-policy/per-format
   read-back tests. The inherited metadata filtering remains XMP-only.
2. HDR PNG16 cICP and AVIF10/12 PQ/HLG Rec.2020 with recipe headroom, ISO 21496-1
   gain-map JPEG, carrier/tag/ImageIO/reconstruction tests, and corresponding
   host controls. No HDR implementation was added in this run.

Format-envelope reference used during implementation:
https://github.com/exiftool/exiftool/blob/master/lib/Image/ExifTool/DNG.pm
(`ProcessOriginalRaw`). The downloaded reference was removed, not vendored.
