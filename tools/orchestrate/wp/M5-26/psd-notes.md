# M5-26 PSD ColorLookup notes

## Genuine format references
Downloaded reference-only sources alongside this file:
- ag-psd-additionalInfo.ts: https://raw.githubusercontent.com/Agamnentzar/ag-psd/master/src/additionalInfo.ts (`clrL` handler and enum definitions).
- ag-psd-descriptor.ts: https://raw.githubusercontent.com/Agamnentzar/ag-psd/master/src/descriptor.ts (LUT3DFileData is raw `tdta`; LUT3DFileName is TEXT).
- psd-tools-adjustments.py: https://raw.githubusercontent.com/psd-tools/psd-tools/main/src/psd_tools/psd/adjustments.py (`ColorLookup` reads/writes `HI` version framing).

Implemented u16 version 1 + u32 descriptor version 16, null-class Adobe Action Descriptor; lookupType=colorLookupType.3DLUT, LUTFormat=LUTFormatType.LUTFormatCUBE, raw embedded CUBE bytes. No native JSON under Adobe keys.

## Supported and limitations
- Embedded unit-domain 3D CUBE, size 2..256, finite RGB samples; native red-fastest order preserved without transpose. Export uses lossless f32 decimal text, RGB order, no dithering.
- Other LUT formats, external-only references, ICC abstract/device-link profiles, BGR order, dithering, 1D/shaper LUT directives and non-unit CUBE domains stay opaque on import. ICC conversion intentionally not added.
- Malformed framing, truncated descriptors/raw data, invalid UTF-8, incomplete cubes and nonfinite samples return errors, not silent native fallbacks.
- Unsupported CUBE feature preflight does not validate the entire unsupported format; its original bytes are retained rather than interpreted.
- Photoshop application rendering was not exercised. Validation is against genuine upstream layout, independently constructed descriptor fixtures, and serialized PSD/PSB roundtrips.

## Verification
- RED: nonidentity CUBE roundtrip failed with the previous native-only export error.
- RED: unsupported 1D CUBE fixture failed before adding opaque feature handling.
- `cargo test -q -p compositor --release --test m5_26_psd`: 11 passed.
- Serialized PSD and PSB nonidentity asymmetric CUBE roundtrip, Adobe field/type checks, independent descriptor import, every-byte truncation, malformed CUBE errors, opaque unsupported retention covered.
- Full compositor release tests passed with only `unsupported_features_are_reported_without_losing_opaque_data` explicitly skipped; output in clrl-release-tests.log. Cache: /Volumes/betterSSD/tessera-cache/target/M5-26.
- **Parent action required:** unowned crates/compositor/tests/psd.rs uses random four-byte clrL data in `unsupported_features_are_reported_without_losing_opaque_data`; it now correctly fails `invalid clrL version`. Replace with a valid unsupported clrL descriptor (e.g. LOOK/profile) or another opaque adjustment key. This file was not changed.
- Formatted only owned Rust files with rustfmt --edition 2024 --config skip_children=true. No commits.
