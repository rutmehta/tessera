# EXP-45 A versus qualified explicit-reference source audit (read-only, 2026-09-28)

## Observed boundary

The same ImageIO run decoded the qualified explicit-rational Google16 control with ISO auxiliary present, HDR peak 13.934, and reported headroom 16; A's retained four-stop file had ISO auxiliary present but HDR peak 7.9838 and reported headroom 8. Core Image decoded the A file near 16. These observations demonstrate host capability above 8 but do not establish why ImageIO treats A differently. See `denominator-explicit/NATIVE-SUMMARY.json` in the pinned `d52a0d13814ca399fc8a07e23de1d2c63f0e8404` reference directory.

## Smallest concrete wire discrepancy

A's `crates/export/src/gain_map.rs:139-157` writes ISO 21496-1 version 0, flags `0x40`, seven explicit BE32 rational pairs. For a four-stop gain, alternateHeadroom and gainMax are each `4,000,000/1,000,000`. The qualified Google16 auxiliary has the same version and flags, and corresponding pairs are each `4/1`. Both encode **4 log2 stops**, which corresponds to a 16× linear gain. A's independent reconstruction confirms values `[0,4,0,4,1,0,0]`; Google's `denominator-explicit/static-16-explicit.json` reports identical semantic values. A's `headroom` is checked finite and >1 in `gain_map.rs:24-28`, then converted to stops by `headroom.log2()` at line 37. This is a representation delta, not evidence of different declared gain.

The fixed-size two-way control would create *copies* of these two already-qualified files and change only the two pairs:

- A retained file SHA-256 `bb08f44d...` at auxiliary ISO namespace offset 1460, flag offset 1492: change pair offsets **1501** and **1517** from BE32 `(4,000,000,1,000,000)` to `(4,1)`.
- Qualified Google16 SHA-256 `cadfffea...` at auxiliary ISO namespace offset 1485, flag offset 1517: change pair offsets **1526** and **1542** from `(4,1)` to `(4,000,000,1,000,000)`.

Each edit replaces 16 bytes at two existing fixed-width positions (four BE32 integers total). Numerator 4,000,000 and denominator 1,000,000 are both in the unsigned 32-bit range, denominator stays positive, ratio remains exactly 4 in binary floating-point either way. The JPEG APP2 size, MPF entry sizes/offsets, compressed base and gain pixels, ICC/XMP, and all other ISO fields must remain byte-identical. A general experiment script should find and validate the APP2 namespace, version/flags, pair values, and frozen input SHA-256, rather than rely only on literal offsets. Verify a byte diff with exactly these 16 changed byte positions or the precise two 8-byte fields; zero-valued bytes within fields may themselves remain unchanged.

Requalify each altered copy structurally and with the independent reference decoder; then perform same-host ImageIO and Core Image decode against both untouched originals and copies in one run. If A's peak rises and Google's drops under opposite rewrites, rational representation is strongly implicated. A one-way change or no change cannot isolate the cause; profile, base/gain JPEG encoding, segment order, and MPF primary attribute still differ between A and Google. The representative-bit-only control was negative, while changing Google's common-denominator `0x48` to explicit `0x40` enabled native recognition; neither result establishes this within-explicit numeric hypothesis.

## Scope and limits

This is a narrow diagnostic, not a proposed product patch. The A serializer's fixed million denominator supports fractional stop values; replacing it generally with denominator 1 would change meaning for nonintegral stops. The controlled four-stop rational fits safely. Do not infer an 8× host cap, waive the original A ImageIO failure, or change output policy from this source comparison alone.
