# M2-09 embedded-correction API handoff

## Integration

- `raw_decode::RawMetadata::opcode_lists: [Option<Vec<u8>>; 3]` is owned OpcodeList1, OpcodeList2, OpcodeList3 data, in that order. It is copied from LibRaw 0.22.2's `color.dng_levels.rawopcodes` for the **selected raw IFD**, not merged with preview IFD metadata. The same field exists on `libraw_ffi::Metadata`. `None` means unavailable, not identity calibration. LibRaw only retains lists smaller than 4 MiB. Its TIFF reader does not independently prove complete reads; use the checked extractor below for untrusted files/strict validation.
- `raw_decode::dng::extract_dng_opcode_lists(&mut impl Read + Seek) -> io::Result<Vec<DngOpcodeLists>>` independently reads classic TIFF/DNG with checked offsets, lengths, budgets and cycle detection. Each result has `ifd_offset: u32` and `lists: [Option<Vec<u8>>; 3]`. No raw/preview selection is guessed; callers must select the appropriate image IFD. Reads from stream start regardless of initial position; final position is unspecified. Non-TIFF/non-DNG returns empty. Truncated/malformed TIFF is an error, not silently empty. DNGVersion presence is required, but this is not a full DNG validator.
- Add `pub mod opcodes;` in the lens crate's lib.rs (owned by the lens worker). `crates/lens/src/opcodes.rs` is std-only and has no dependencies on other lens modules.
- `opcodes::parse_opcode_list(&[u8]) -> Result<Vec<ParsedOpcode>, OpcodeError>` returns supported corrections in original order. `ParsedOpcode` carries `minimum_version: u32`, `flags: u32`, and `correction: CorrectionOpcode`.
- `CorrectionOpcode::WarpRectilinear(WarpRectilinear)` exposes `coefficients: Vec<[f64; 6]>` in `[k0,k1,k2,k3,p1,p2]` order per plane, plus normalized `center: [f64; 2]` (`cx,cy`). Supports 1–3 planes. Three planes preserve channel-specific warps; do not average them into a single distortion curve.
- `CorrectionOpcode::FixVignetteRadial(FixVignetteRadial)` exposes `coefficients: [f64; 5]` for the gain polynomial's r²…r¹⁰ terms and `center: [f64; 2]`. The constant is 1; coefficients are not lensfun coefficients.

All opcode scalars are **big-endian even in little-endian TIFF**. Parsing retains values without applying any correction. The consumer must implement DNG radius/coordinate normalization, stage placement, version/flag policy, and model conversion explicitly. Bad-pixel opcodes 4/5 and every other unsupported opcode are skipped after validating their framing; this is a correction extractor, not a full required-opcode validator. Invalid supported payloads fail the whole list. Finite coefficients, normalized centers, exact payload sizes, counts, and truncation are checked.

## Actual coverage / limitations

Bundled source inspected: `libraw_types.h` (public maker notes/lens structures and raw opcode buffers), `src/metadata/tiff.cpp` (tags 51008, 51009, 51022), and `src/metadata/identify.cpp` (selected-IFD transfer). Public LibRaw 0.22.2 structs do **not** expose parsed proprietary geometric distortion, lateral-CA, or vignetting calibration coefficients. No vendor calibration fields are fabricated from camera/lens identity, crop, white balance, or output-processing settings. Native Sony ARW, Fuji RAF, Canon CR3, Nikon NEF proprietary correction calibration remains unsupported. DNG opcodes from those cameras are supported when actually present.

The checked extractor supports II/MM classic TIFF, next-IFD chains, SubIFDs (LONG/IFD), inline/out-of-line UNDEFINED opcode tags. It rejects BigTIFF, duplicate tags, cyclic/aliased IFD graphs, >256 IFDs, >4096 entries/IFD, >4 MiB/list or >12 MiB total list bytes. It does not traverse EXIF/maker-note-private IFDs, decode image strips, parse GainMap, or apply bad-pixel corrections. Selected-IFD metadata does not automatically substitute arbitrary lists from the checked all-IFD extractor.

## Tests

Handcrafted II/MM TIFFs cover all three tags, SubIFDs and per-image grouping, truncation, bad types, empty/oversized lengths, out-of-file pointers, cycles and non-DNG input. Handcrafted opcode tests cover warp coefficients/centers, three-plane values, vignette, ignored unknown/badpixel opcodes, headers, all truncated prefixes, invalid plane counts, nonfinite numbers and trailing bytes. FFI ownership and raw metadata transfer are tested. `raw-decode` tests include the standalone opcode module by path so it is exercised before lens workspace integration.

Run with `CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M2-09 cargo test -p raw-decode -p libraw-ffi`.
