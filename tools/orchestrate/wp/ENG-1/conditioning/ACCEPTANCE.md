# Accepted synthetic references

The pre-fix references were committed in `927c94f1`. Running the fixed renderer
against them failed all three stored-reference profile tests (release mode):
sRGB maximum 0.20692861, Display P3 0.19836733, Adobe RGB 0.19964510.

The replacement files have been verified against the `after_sha256` values in
[report.json](report.json). The before files at `927c94f1` match its
`before_sha256` values. Recomputing every changed-pixel predicate directly
between that commit and the accepted files reproduces the report exactly.

| Profile | Opacity | Changed / 4,403 | Max encoded delta | Outside radius 11 |
| --- | ---: | ---: | ---: | ---: |
| sRGB | 1 | 64 | 0.2069286108 | 0 |
| sRGB | 0.35 | 63 | 0.0724250674 | 0 |
| sRGB | 0 | 0 | 0 | 0 |
| Display P3 | 1 | 79 | 0.1983673275 | 0 |
| Display P3 | 0.35 | 79 | 0.0694285631 | 0 |
| Display P3 | 0 | 0 | 0 | 0 |
| Adobe RGB | 1 | 87 | 0.1996451020 | 0 |
| Adobe RGB | 0.35 | 85 | 0.0698757768 | 0 |
| Adobe RGB | 0 | 0 | 0 | 0 |

All 457 changed pixels satisfy the predicate. The measured maximum distance
is 8; the justified bound remains 11. The seed is index 2101, coordinate
(29,8), with pre-fix L = 0.0004458632320165634 (sRGB),
0.0004458688199520111 (P3), or 0.0004458613693714142 (Adobe RGB).
Global dehaze airlight and confidence are bit-identical between runs.
See [README.md](README.md) for the operator proof and cached-opacity provenance.

All five photographic RAW PNGs remain unchanged. Both builds passed their
strict stored-PNG comparison, and their before/after encoded RGB8 captures
are byte-identical. [raw-report.json](raw-report.json) records each result.
