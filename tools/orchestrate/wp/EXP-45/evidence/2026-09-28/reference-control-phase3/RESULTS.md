# EXP-45 phase 3: reciprocal fixed-width ISO rational test

Date: 2026-09-28; same A host macOS 26.6.2 (25G83), arm64. The current experiment changed **copies only** of the retained A four-stop JPEG and the previously admitted explicit-denominator Google 16× control. No APP2 segment length, MPF entry, ICC profile, JPEG base/gain image bytes, production source or test assertion changed. Phase-1 and phase-2 reports/manifests/fixtures remain immutable. The source and frozen byte provenance of `crates/export/src/gain_map.rs` is in `SOURCE-PROVENANCE.json` plus `gain_map.rs.frozen`; the read-only audit is `/tmp/tessera-exp45-a-vs-qualified-source-audit.md` (copied into the portable bundle).

## Before native work: exact semantic qualification

`make_rational_swap.py` checks the original source SHA-256, parses the ISO v0 auxiliary and MPF IFD, requires an explicit-denominator `0x40` flag, and updates only two BE32 numerator/denominator pairs: AlternateHeadroom and GainMapMax. A's original pairs `4000000/1000000` become `4/1` at byte ranges `[1501,1509)` and `[1517,1525)`; Google explicit16's original `4/1` pairs become `4000000/1000000` at `[1526,1534)` and `[1542,1550)`. There is no file-length change; the script asserts every byte outside those two ranges remains identical. Exact word before/after hex, actual differing byte offsets, source/output SHA-256, MPF association and rational equality are in `A-to-reduced.json` and `Google-to-million.json`.

Output hashes: `A-to-reduced.jpg` SHA-256 `7056601607020d82d96e22c47220c8bae20dff6c8ace0624801437e33686af3d`; `Google-to-million.jpg` SHA-256 `19127b58eccbfff947f6c4410fe331cd639657e7d02c8d485e144b28c4daa3f9`. `static_validate.py` independently re-parses all four JPEG/MPF/ISO associations and verifies that the seven rational numeric values, MPF offset/size/attributes, and ExifTool ICC hashes are equal within each original/copy pair. It confirms ImageIO-extractable auxiliary bytes at the unchanged offsets; direct exit 0. The 4-stop values are `0,4,0,4,1,0,0` for both representations in both pairs. The generated fixture payloads are unchanged apart from the recorded 16-byte field ranges.

The pinned reference decoder requested linear transfer (`UHDR_CT_LINEAR=0`) and half-float RGBA (`UHDR_IMG_FMT_64bppRGBAHalfFloat=4`) for **all four** files. It exited 0 each time; original A and A-to-reduced produced byte-identical 10,240-byte RGBA outputs, SHA-256 `61cf27b4757daa75048dc3c4d44e4520838872ee7096e23df001442a91547aa5`, bright patch 16.0. Google-explicit and Google-to-million likewise produced byte-identical outputs, SHA-256 `bbddc6f6788ca1d72f79b14329812622f24ee2ac20e62f45dd252078705baa4a`, bright patch 13.9453125. Decoder command arrays, metadata and direct exits are in the four `*.decode.json` files. Numeric equivalence and decoded-byte equivalence were established **before** running ImageIO/Core Image.

## Same-host native result

The unchanged phase-1 scratch probe binaries were run on the four original/copy files in one ImageIO invocation and one software Core Image invocation. Raw ImageIO stdout (`native/imageio.stdout`) begins with four `too few samples` lines before the JSON array; this prefix is preserved separately in `native/imageio-prefix.txt`, with a parsed copy in `native/imageio-parsed.json`. Both direct process exits were 0. All four ImageIO ISO auxiliary lookups succeeded, and finite opaque pixels were decoded.

| File | SDR peak | ImageIO HDR peak | ImageIO headroom | Core Image gain / peak |
|---|---:|---:|---:|---:|
| Retained A 4-stop original | 1 | 7.9837622643 | 8 | yes / 16.000003815 |
| A-to-reduced copy | 1 | 7.9837622643 | 8 | yes / 16.000003815 |
| Admitted explicit Google16 original | 0.871337890625 | 13.9340171814 | 16 | yes / 13.941894531 |
| Google-to-million copy | 0.871337890625 | 13.9340171814 | 16 | yes / 13.941894531 |

**Outcome: negative for the proposed A cause.** Reducing A's two numerically equal rationals does not lift its ImageIO ~8× readback; inflating the known-admitted Google control's two rationals does not reduce its ImageIO ~16× readback. The representation **form** (phase-2 `0x48` common versus `0x40` explicit) affected admission of the unmodified Google file, but the magnitude of these equal explicit rational pairs is not the cause of A's separate ~8× result. A's unchanged original frozen core gate remains 4 pass/1 fail. No encoder acceptance, assertion waiver, or general host-support claim follows. No further metadata mutations or runtime probes were performed.
