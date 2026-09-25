# Presence scheduling retry measurements

Before is the inherited partial implementation, not pristine HEAD. After adds
shared radius-independent moments and omits the inactive fine scale for clarity.
Both executions used the same real NEF, one warmup and one timed sample per
operator. Sources were stable during each execution and artifacts rebuilt.
Wall times include transfers and completion. These are NOT resident slider
latency or halo/global-statistics-aware regional refinement. Substantial CPU
timing variation means these are observations, not controlled speedup estimates.
Zero submissions mean CPU delegation. Counts include warmup and timed runs.

Validated 90 rows; maximum CPU-reference error: 6.22272491e-05.

## L0-full

| Operator | CPU before ms | GPU backend before ms | CPU after ms | GPU backend after ms | Before / after submissions |
|---|---:|---:|---:|---:|---:|
| basic_tone | 2731.282 | 716.694 | 1209.373 | 320.773 | 74 / 74 |
| chroma_nr | 4584.878 | 889.483 | 10432.596 | 1596.311 | 1160 / 1160 |
| clarity | 5801.274 | 5187.422 | 5182.439 | 11092.234 | 0 / 0 |
| color_grading | 2448.038 | 358.539 | 2561.561 | 454.084 | 74 / 74 |
| crop_straighten | 26995.083 | 17525.015 | 20954.805 | 65804.748 | 0 / 0 |
| curves | 1664.144 | 675.065 | 828.736 | 352.518 | 74 / 74 |
| dehaze | 14252.746 | 19697.808 | 18759.099 | 16199.374 | 0 / 0 |
| grain | 1152.738 | 448.027 | 1990.020 | 795.576 | 74 / 74 |
| hsl | 2518.011 | 374.587 | 2533.369 | 438.114 | 74 / 74 |
| local_blend_only | 61.304 | 93.101 | 132.403 | 178.370 | 0 / 0 |
| luminance_nr | 1812.180 | 852.614 | 1964.553 | 1234.100 | 1160 / 1160 |
| sharpening | 1018.974 | 883.234 | 1032.450 | 1263.106 | 1160 / 1160 |
| texture | 3864.095 | 3401.337 | 3364.721 | 3600.989 | 0 / 0 |
| vibrance | 2487.147 | 359.203 | 2689.523 | 505.301 | 74 / 74 |
| vignette | 1018.649 | 466.632 | 1630.272 | 504.531 | 74 / 74 |

## L0-region-1024

| Operator | CPU before ms | GPU backend before ms | CPU after ms | GPU backend after ms | Before / after submissions |
|---|---:|---:|---:|---:|---:|
| basic_tone | 36.853 | 8.800 | 36.972 | 8.302 | 2 / 2 |
| chroma_nr | 188.114 | 51.276 | 131.240 | 23.925 | 32 / 32 |
| clarity | 156.886 | 34.046 | 159.530 | 26.781 | 2 / 2 |
| color_grading | 117.522 | 23.817 | 65.898 | 9.013 | 2 / 2 |
| crop_straighten | 1351.077 | 19.509 | 571.664 | 13.952 | 2 / 2 |
| curves | 25.616 | 9.957 | 26.322 | 9.392 | 2 / 2 |
| dehaze | 394.187 | 32.951 | 385.281 | 35.655 | 6 / 6 |
| grain | 159.237 | 31.330 | 32.248 | 12.480 | 2 / 2 |
| hsl | 139.451 | 24.645 | 66.053 | 9.594 | 2 / 2 |
| local_blend_only | 4.625 | 14.223 | 1.666 | 8.875 | 2 / 2 |
| luminance_nr | 52.980 | 26.765 | 51.176 | 22.894 | 32 / 32 |
| sharpening | 29.294 | 27.038 | 27.881 | 26.009 | 32 / 32 |
| texture | 101.816 | 25.394 | 109.080 | 25.700 | 2 / 2 |
| vibrance | 113.324 | 21.909 | 66.053 | 9.793 | 2 / 2 |
| vignette | 62.162 | 42.631 | 24.984 | 12.025 | 2 / 2 |

## L2-full

| Operator | CPU before ms | GPU backend before ms | CPU after ms | GPU backend after ms | Before / after submissions |
|---|---:|---:|---:|---:|---:|
| basic_tone | 76.092 | 22.515 | 77.064 | 21.990 | 6 / 6 |
| chroma_nr | 285.857 | 58.777 | 296.603 | 58.371 | 80 / 80 |
| clarity | 320.864 | 65.265 | 331.567 | 56.187 | 2 / 2 |
| color_grading | 153.597 | 23.359 | 156.326 | 23.967 | 6 / 6 |
| crop_straighten | 1118.407 | 30.709 | 1264.697 | 36.297 | 2 / 2 |
| curves | 53.193 | 22.686 | 55.065 | 22.873 | 6 / 6 |
| dehaze | 881.714 | 63.862 | 1100.797 | 62.721 | 6 / 6 |
| grain | 75.187 | 36.586 | 73.280 | 29.377 | 6 / 6 |
| hsl | 162.536 | 23.298 | 157.458 | 24.076 | 6 / 6 |
| local_blend_only | 3.967 | 5.985 | 3.864 | 6.190 | 0 / 0 |
| luminance_nr | 113.702 | 56.273 | 114.866 | 56.614 | 80 / 80 |
| sharpening | 61.648 | 65.331 | 61.339 | 58.808 | 80 / 80 |
| texture | 208.412 | 52.873 | 215.733 | 52.946 | 2 / 2 |
| vibrance | 153.761 | 23.328 | 158.577 | 24.674 | 6 / 6 |
| vignette | 54.941 | 30.143 | 55.944 | 33.968 | 6 / 6 |
