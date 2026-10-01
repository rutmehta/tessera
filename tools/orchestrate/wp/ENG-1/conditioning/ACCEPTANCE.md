# Accepted synthetic references — ENG-1d

The continuous gain ruling supersedes ENG-1b's attenuating divisor floor.
The historical pre-conditioning files at `927c94f1` still match every
`before_sha256` in [report.json](report.json). The six accepted nonzero-opacity
files match its `after_sha256`; zero-opacity files were not changed.
Comparisons use exact final encoded RGBA f32 bits, with no attribution tolerance.

| Golden | Changed pixels / 4,403 | Max encoded delta | Max distance | Outside predicate |
| --- | ---: | ---: | ---: | ---: |
| sRGB, amount 1 | 38 | 0.000294983387 | 8 | 0 |
| sRGB, amount 0.35 | 38 | 0.0001032352448 | 8 | 0 |
| sRGB, amount 0 | 0 | 0 | n/a | 0 |
| Display P3, amount 1 | 57 | 0.0002828836441 | 8 | 0 |
| Display P3, amount 0.35 | 55 | 9.900331497e-05 | 8 | 0 |
| Display P3, amount 0 | 0 | 0 | n/a | 0 |
| Adobe RGB, amount 1 | 44 | 0.0002887845039 | 8 | 0 |
| Adobe RGB, amount 0.35 | 42 | 0.0001010894775 | 8 | 0 |
| Adobe RGB, amount 0 | 0 | 0 | n/a | 0 |

All 274 changed pixels satisfy the original active-pre-fix-seed predicate.
The maximum Chebyshev distance is 8, within the unchanged radius 11.
The sole seed remains index 2101 (29,8), with the same pre-fix luminance bits.
Global dehaze airlight/confidence bits remain identical. See [README.md](README.md)
for the support proof, seed predicate, and cached-opacity provenance.

All five photographic RAW PNGs remain untouched. Both builds passed their
strict stored-PNG comparison, and before/after RGB8 captures are byte-identical.
[raw-report.json](raw-report.json) records each result. The fixture symlink stayed
present throughout. Captures and full build logs: `/tmp/eng1d-audit-v3`.
