# Original + XMP checkpoint (partial item 2)

M2-45c is still **FAIL / incomplete**. This slice does not complete any of the
three numbered acceptance items. The inherited XMP-only filtering work remains
staged. No numbered item is being committed as green.

## Implemented in this retry

- `export::export_original(source, destination, recipe, packet, cancel)` copies
  non-DNG originals byte-for-byte and writes `<destination>.xmp`. It never
  decodes or transforms sample data. Ordinary source documents can also be
  copied this way; it does not claim to validate opaque RAW sample payloads.
- Classic little-/big-endian TIFF DNG receives updated embedded XMP. A new IFD0
  is appended with tag 700 replaced; all other directory entries, existing
  payload offsets, next-IFD pointer and sample bytes are retained. Only the
  TIFF header's IFD0 pointer is changed in the original byte range. This is not
  a DNG version upgrade or a DNG 1.6 tag audit. BigTIFF is explicitly rejected.
- XMP is bounded to 1 MiB. Malformed/truncated IFD0, duplicate tags, invalid XMP
  offsets/types, and missing DNGVersion fail without publishing the copy.
  This is not a full validator of every existing DNG directory or payload.
- Supplied XMP is authoritative (typically the external sidecar). Otherwise
  embedded DNG XMP is used. An explicit recipe updates development/selection
  while preserving unknown properties in that selected packet. Supplied and
  embedded packets are not generically union-merged. Unreferenced original
  packet bytes remain in the copied DNG; this mode is not metadata sanitization.
- `recipe=None` retains the selected packet's existing edits. Both hosts use
  this when neither an external recipe nor XMP exists, avoiding replacement of
  embedded-only development and ratings with a synthetic default recipe.
- Source is read-only; copying uses a 1 MiB buffer with cancellation checks.
  Publication reuses the exporter's synced temporary-file / no-clobber commit
  and sidecar rollback. Existing destination or appended sidecar is an error.
- CLI: `--format original`, retaining the original suffix. Raw `{date}` naming
  reads metadata through LibRaw without unpacking samples, only when requested.
  Ordinary-document date behavior matches the existing CLI (empty date).
- FFI: `ExportOptions` JSON `"format":"original"`. Uses existing naming,
  conflict handling, reports, progress, cancellation, presets, Previous,
  multi-preset workflow and post-actions. No new UI or UniFFI binary record
  layout. Original mode bypasses source decoding/rendering/model loading.
- Hosts reject metadata reduction/privacy flags, hierarchy removal, resize,
  sharpening, watermark, upscaling and byte limits in original mode, rather
  than silently pretending to apply them to a byte-preserving copy.
- MCP has not gained a new original mode in this slice.

## Verification

- `original-red.log`: missing core API, before implementation.
- `original-host-red.log`: CLI rejects missing original format.
- `original-ffi-red.log`: FFI rejects missing original JSON variant.
- `original-green.log`: core copy/merge/cancel/collision and developed sample
  preservation tests passed before host integration.
- Read-only review found two host bugs: embedded-only DNG recipe reset and
  dropped RAW capture dates. Neither review nor these tests establishes the
  broader M2-45c acceptance criteria.
- `original-embedded-red.log`: real FFI export reset exposure 1.25 to 0.
- `original-date-red.log`: CLI erroneously succeeded with `-source.nef` instead
  of reading the date from RAW metadata. Fixed before final gate.
- `original-focused-green.log`: CLI/FFI original tests passed after those fixes.
- `original_tiff.rs`: both TIFF byte orders, opaque tag/payload/next-IFD
  preservation, malformed/truncated input, BigTIFF refusal and sidecar conflict.
- `original-full-gate.log`: tests passed, Clippy found constant `chunks_exact`
  in a new test. Replaced with `as_chunks::<12>()`, no lint suppression.
- `original-verified-gate.log`: the exact full user gate passed, exit 0:
  389 Rust tests passed, 0 failed, 16 ignored, followed by Clippy (`-D warnings`),
  formatting, license check, binding regeneration and Swift build (13.76s).
  Generated bindings have no tracked diff because options still use JSON.
  Existing native LibRaw compiler warnings and the blake3_neon macOS 26.5/15.0
  deployment-target linker warning remain.
- `CARGO_TARGET_DIR` stayed `/Volumes/betterSSD/tessera-cache/target/M2-45c`.
  `git diff --check` passed. No dependencies or licensing changes.

## Remaining for M2-45c

1. Native EXIF/IPTC extraction, preservation, filtering and carriers in all
   requested formats, with independent carrier-aware read-back for every policy.
   The inherited policies remain XMP-only.
2. Developed DNG OriginalRawFileData toggle, DNG 1.6 tag audit, independent LibRaw
   RGB comparison against the engine render, plus corresponding host controls.
   The original-copy subfeature above alone does not finish item 2.
3. All requested HDR output: PNG16 cICP, AVIF10/12 PQ/HLG Rec.2020 with recipe
   headroom, interoperable ISO 21496-1 gain-map JPEG, reconstruction/ImageIO
   tests, and the corresponding CLI/FFI controls.
