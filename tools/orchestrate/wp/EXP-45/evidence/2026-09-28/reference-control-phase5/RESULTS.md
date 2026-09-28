# EXP-45 phase 5: reciprocal primary ICC profile swap

Date 2026-09-28; A host macOS 26.6.2 (25G83), arm64. This is a **scratch-only** profile-path diagnostic using the unchanged retained A four-stop JPEG and the phase-4 admitted exact-white Google16 explicit-denominator JPEG. It follows the read-only `source-audit.md`. Original fixtures, phase-1–4 evidence, Tessera product source and the unchanged original ImageIO gate remain untouched.

## Exact source, byte changes, and pre-native qualification

`make_icc_swap.py` requires the original SHA-256 values (`A bb08f44d…`, `Google255 61824f8e…`) and parses the primary JPEG markers. Each has exactly one ICC APP2 segment: length field 604, total marker+segment 606 bytes, prefix `ICC_PROFILE\0` with identical sequence/total chunk identifiers `01/01`, and 588 bytes of profile data. It reciprocally swaps **only those 588 profile bytes** in two copies. The complete original profile bytes are retained as `A.icc` SHA-256 `9917273a11ee1e244854a427a69099558670b195637332b5bd012efba0059f44` and `Google255.icc` SHA-256 `be1eccdf7ba1b4c2b13dae337d96b87040fc9206d2d388f7fbed8187cf2c5a6c`. Both source/output lengths are identical and every byte outside the ICC data ranges is asserted equal. Exact data ranges, segment ranges, source/output hashes and 299 differing-byte counts are in `A-swap.json` and `Google255-swap.json`. The transformed copies are **not unmodified source files**.

Output SHA-256: `A-with-Google255-ICC.jpg` `dd79b76d38536ed7a4b4766363a80619e14a40c4f9897ec97f2102e8fb1904e5`; `Google255-with-A-ICC.jpg` `c250df6ed9dc29e55c4e918499eb829f898ec42d69253408ed91b5a0493546f3`. `static_validate.py` independently confirms unchanged file size, MPF entry attributes/offset/auxiliary bytes, ISO v0 metadata/rational values, and byte-exact ExifTool auxiliary extraction. A primary MPF attribute stays `0x20030000`; Google stays `0x00030000`. JPEG compressed base/gain data, marker order and ISO APP2 bytes are necessarily identical by the outside-ICC byte check; no extra patch was made. The source audit documents the profiles' D50/sRGB similarities and differences (A LittleCMS v4.4/perceptual/TRC type3 versus Google v4.3/relative/TRC type4, A extra chad/chrm); this whole-profile swap does not isolate any one subfield.

Before native comparison, the pinned Google libultrahdr reference decoder was run on all four files with documented linear half-float RGBA and, separately, SDR sRGB U8 output. Each direct exit was 0. For A original versus A+GoogleICC, the HDR output is byte-identical (SHA-256 `61cf27b4757daa75048dc3c4d44e4520838872ee7096e23df001442a91547aa5`, white center16) and the SDR output is byte-identical (SHA-256 `cd7ea07100f775c17d4b0385428ed2a750442ce93b7358d032e6f0c43422d205`, white center255). For Google255 original versus Google255+AICC, HDR byte-identical SHA-256 `f8ed048c2b04f259ff1a65d77a3e35c17556d355c0322dd279a9d59d3240a19e` (white center16) and SDR byte-identical SHA-256 `bb0c4719ebc01d19663d6382bcd149db079c8c766775e523604eafe34c04d975` (white center255). Full command arrays, metadata, pixel samples and exits are in the eight `*.hdr-linear-half.json`/`*.sdr-srgb-u8.json` records. These decoder byte identities are a qualification control, not proof that Apple's color path is invariant to ICC.

## Same-host native result

The phase-1 scratch ImageIO and software Core Image binaries were reused without recompilation, with original/copy pairs in one invocation per probe. Raw `native/imageio.stdout` begins with four `too few samples` lines before valid JSON; the prefix and parsed JSON are separately retained. Both direct exits were 0, ISO auxiliary presence was true, and all decoded pixels finite/opaque. The ImageIO input CGColorSpace ICC hashes changed in the reciprocal copies as expected (`native/NATIVE-SUMMARY.json`).

| File | SDR peak | ImageIO HDR peak | ImageIO headroom | Core Image HDR peak |
|---|---:|---:|---:|---:|
| A original | 1 | 7.9837622643 | 8 | 16.000003815 |
| A with Google255 ICC | 1 | 7.9837627411 | 8 | 16.000003815 |
| Google255 original | 1 | 15.9515762329 | 16 | 16.000003815 |
| Google255 with A ICC | 1 | 15.9515762329 | 16 | 16.000003815 |

**Outcome: negative for the whole ICC profile as the cause of A's half-headroom.** Swapping the full profile did not transfer either file's ImageIO headroom/result; A's ~4.8×10⁻⁷ absolute pixel shift is immaterial to its retained 4% assertion. This result does not isolate or independently exonerate profile subfields, nor does it establish a broader encoder/host claim. The original A core gate remains 4 pass/1 fail; no tolerance or product source changed. No additional runtime probe was performed in this phase.
