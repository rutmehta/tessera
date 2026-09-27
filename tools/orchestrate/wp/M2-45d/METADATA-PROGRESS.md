# M2-45d native metadata progress — uncommitted

Overall WP remains **incomplete**: ISO 21496-1 gain-map JPEG is not delivered.
The existing DNG and PQ/HLG implementation remains in place. No commit or external
publication was made. Parent review and the full required gate are still required.

## Implemented

- Bounded extraction of portable native EXIF, IPTC IIM and embedded XMP from
  JPEG, classic TIFF (including TIFF-based RAW), PNG, AVIF and uncompressed
  metadata boxes in JXL. Both TIFF byte orders are handled.
- Filtered native carriers in JPEG (APP1/APP13), TIFF/DNG (fresh relocated IFDs),
  PNG (eXIf), AVIF (associated Exif item), and JXL (Exif box), alongside XMP.
  Native metadata is included in JPEG byte-budget calculations and HDR carriers.
- All, copyright, copyright/contact, all-except-camera/Camera Raw, and none
  policies; independent person, location and hierarchy switches. Person identities
  in embedded XMP are retained as filtering context even with an external sidecar.
  Native IPTC keywords are merged with XMP hierarchy, including Latin-1 keywords.
  Partial sidecars override explicit properties without discarding absent embedded
  copyright, contact or descriptive fields. Qualified RDF values survive merging.
- CLI, FFI and MCP supply each actual source path to metadata extraction. MCP
  exposes metadata/privacy options through engine types and the schema mirror.
  Batch source metadata is keyed by image sequence, not shared across images.
- Original passthrough still follows its byte-preserving path and rejects filtering.

## Security and support boundaries

Extraction is limited to 2 MiB of aggregate metadata, 1 MiB per XMP packet, four
IFDs, 1024 entries per IFD and 4096 JPEG markers. Bounds, directory cycles,
duplicate tags, misplaced directory pointers and invalid source associations are
rejected. PNG compressed profiles are bounded during decompression. Metadata
source descriptors must refer to regular files. Output offsets are rebuilt.

“All” means portable descriptive metadata, not a byte-for-byte source metadata
blob: source pixel/layout offsets, orientation, previews, opaque MakerNotes and
raw-development structures are deliberately excluded. Person removal drops opaque
EXIF XP fields/UserComment conservatively. Unmarked names in arbitrary prose cannot
be inferred as person keywords. BigTIFF, JPEG Extended XMP and compressed JXL
metadata boxes are rejected rather than silently bypassing privacy filtering.
Non-TIFF proprietary RAW containers such as RAF/CR3 do not yet have native
metadata extraction; the parser currently returns no native fields for unknown
containers. These limitations prevent a claim of universal source preservation.

## Verification

Tests use ExifTool as an independent fixture writer and output reader. The core
matrix covers six formats × five policies × eight combinations of privacy and
hierarchy switches (240 exports). Additional tests cover independent PNG/TIFF
inputs, all-format reimport, big-endian TIFF, AVIF primary-image associations,
Latin-1 person keywords, HDR carriers, JPEG budgets, malformed inputs and aggregate
compressed metadata bounds. Host tests read actual exported native fields,
including two distinct source images in CLI and FFI batches.

Red-phase logs and focused results are retained in this directory. The review
regressions additionally cover misplaced IFD pointers, unsupported Extended XMP
and sidecar preservation. Final verification results are recorded below.
All cargo commands in this run use
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-45d`.

## Gain-map investigation — not shipped

A prototype encoded an SDR sRGB base, grayscale logarithmic gain JPEG, standard
version-0 ISO APP2 metadata and MPF associations. Independent JPEG decoding,
ISO reconstruction against the PQ reference and ExifTool MPImage2 extraction
passed before the ImageIO assertion. On this macOS host, ImageIO recognized
headroom but returned no auxiliary data or decoded pixel buffer. Moving the ISO
segment and removing compatibility XMP did not fix decoding. An attempt to
generate an independent reference using Apple's ISO gain-map encoder also failed,
so platform support could not be established. No interoperability pass is claimed.

The prototype and failing host integration stubs were removed rather than shipped
as working output. `gain-map-core.log`, `gain-map-imageio.log` and the initial
red logs record the investigation. Remaining work is a verified ISO encoder,
actual Apple/independent interoperability, and corresponding FFI/CLI/MCP controls.

## Concurrent writer

During review fixes, another process changed the same source files and test
fixtures. This caused a transient missing merge-method build error captured in
`native-bindings.log`. The duplicate local merge helper was removed in favor of
`with_sidecar_overrides`; source edits were paused and ownership clarification
requested. Subsequent validation must use the final coherent tree.

## Final checks performed in this session

- `native-final-focused.log`: 28 passed, 0 failed, including the 240-export
  policy matrix, review regressions, original-copy, DNG and HDR carrier tests.
- `native-final-sidecar.log`: 10 passed, 0 failed (export policy and keyword/
  namespace merge tests).
- `native-final-clippy-retry.log`: selected five crates, all targets,
  `-D warnings` passed. The first final attempt caught a test-only
  `manual_is_multiple_of` lint, corrected before this rerun. Existing LibRaw
  C/C++ build-script warnings remain; no warning suppression was added.
- `native-final-fmt-retry.log`: workspace formatting check passed. The first
  check caught two allowed-file formatting differences, corrected locally.
- `native-bindings-retry.log`: release FFI build and UniFFI Swift generation
  passed. Generated Swift/header/modulemap are byte-for-byte identical to the
  checked-in files; no generated binding diff is needed for JSON-only options.
- Source/document `git diff --check` and the changed-path allowlist check passed.

Swift build, the full workspace check, license gate and the full test gate were
**not** run here; parent owns that gate. Earlier DNG/HDR report claims are from
those checkpoints, not newly executed verification in this run.

- `native-final-hosts.log`: CLI and MCP native-metadata tests passed (one each).
  The FFI rerun waited on another process's build lock and was interrupted by
  this session before it ran; it is **not** counted as a new pass. The earlier
  FFI native test and all 25 export integration tests passed in
  `native-focused.log`. Final completed focused checks total 40 tests.

At handoff another writer is running `native-full-gate.log` and adding a JPEG
segment-limit regression. Those changes/results are outside this session's
completed verification; this report does not claim their gate passed. The
parent must review and verify the final shared tree after concurrent work ends.
