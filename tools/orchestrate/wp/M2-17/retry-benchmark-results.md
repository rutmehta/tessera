# M2-17 incremental retry measurements

RESULT: FAIL — performance/coverage acceptance remains incomplete.

Before is the existing partial M2-17 implementation at retry start, NOT pristine HEAD. After includes queued local-tone compute dispatches, exact percentile selection, and the identity-curve roundtrip bypass. The original HEAD comparison is preserved in benchmark-results.md.

Both commands ran on the real NEF with PIPELINE_BENCH_SAMPLES=1: one warmup and one timed sample. The before benchmark executable was compiled before production edits and continued running while focused tests were developed. The after executable was rebuilt from the final production sources. CPU/GPU wall times include transfers and completion, not decode, upstream preparation or cloning. This is not a controlled speedup study or a resident slider benchmark. The independent RGB crop is not halo/global-statistics-aware 1:1 refinement. Submission counts include both invocations; zero means CPU fallback.

## L2-full

| Operator | CPU before ms | CPU after ms | GPU before ms | GPU after ms | Submissions before/after | After max error |
|---|---:|---:|---:|---:|---:|---:|
| basic_tone | 472.960 | 86.712 | 119.802 | 26.578 | 6/6 | 2.62260437e-06 |
| curves | 311.491 | 55.386 | 134.502 | 25.113 | 6/6 | 1.43051147e-06 |
| texture | 904.994 | 208.706 | 268.954 | 65.362 | 8/2 | 3.21865082e-06 |
| clarity | 1579.240 | 322.696 | 387.062 | 112.511 | 8/2 | 1.78813934e-06 |
| dehaze | 5029.740 | 848.894 | 1098.887 | 66.171 | 12/6 | 1.66893005e-06 |
| sharpening | 267.663 | 61.116 | 618.716 | 57.443 | 80/80 | 4.76837158e-07 |
| luminance_nr | 657.631 | 113.441 | 619.147 | 55.550 | 80/80 | 2.38418579e-07 |
| chroma_nr | 2182.029 | 343.789 | 623.252 | 61.678 | 80/80 | 4.17232513e-06 |
| vibrance | 608.991 | 154.008 | 102.864 | 23.510 | 6/6 | 5.00679016e-06 |
| hsl | 2320.971 | 154.442 | 280.517 | 23.803 | 6/6 | 4.17232513e-06 |
| color_grading | 2036.069 | 170.035 | 103.682 | 26.633 | 6/6 | 4.05311584e-06 |
| vignette | 559.850 | 55.202 | 226.843 | 31.659 | 6/6 | 3.57627869e-07 |
| grain | 1389.061 | 76.512 | 147.682 | 28.154 | 6/6 | 1.66893005e-06 |
| crop_straighten | 7598.586 | 1080.998 | 149.051 | 27.262 | 2/2 | 6.22272491e-05 |
| local_blend_only | 10.909 | 4.263 | 96.153 | 6.040 | 0/0 | 0.00000000e+00 |

## L0-region-1024

| Operator | CPU before ms | CPU after ms | GPU before ms | GPU after ms | Submissions before/after | After max error |
|---|---:|---:|---:|---:|---:|---:|
| basic_tone | 274.205 | 35.260 | 47.497 | 8.507 | 2/2 | 1.90734863e-06 |
| curves | 347.937 | 24.990 | 107.444 | 9.222 | 2/2 | 7.15255737e-07 |
| texture | 611.878 | 99.905 | 118.455 | 26.727 | 4/2 | 2.50339508e-06 |
| clarity | 788.131 | 160.249 | 63.791 | 34.879 | 4/2 | 1.07288361e-06 |
| dehaze | 1592.297 | 386.913 | 354.260 | 74.108 | 8/6 | 1.34110451e-06 |
| sharpening | 141.875 | 29.440 | 111.429 | 28.478 | 32/32 | 2.38418579e-07 |
| luminance_nr | 208.005 | 53.997 | 144.388 | 27.801 | 32/32 | 2.38418579e-07 |
| chroma_nr | 1817.403 | 131.765 | 202.463 | 27.483 | 32/32 | 2.14576721e-06 |
| vibrance | 463.011 | 66.666 | 44.316 | 10.063 | 2/2 | 2.74181366e-06 |
| hsl | 359.016 | 66.904 | 46.630 | 10.702 | 2/2 | 2.14576721e-06 |
| color_grading | 403.714 | 66.217 | 78.338 | 10.087 | 2/2 | 2.02655792e-06 |
| vignette | 108.355 | 26.800 | 38.029 | 12.708 | 2/2 | 2.38418579e-07 |
| grain | 203.767 | 36.142 | 46.552 | 12.408 | 2/2 | 9.53674316e-07 |
| crop_straighten | 4244.319 | 513.622 | 39.009 | 13.440 | 2/2 | 4.82797623e-06 |
| local_blend_only | 1.882 | 1.769 | 18.805 | 9.716 | 2/2 | 1.19209290e-07 |

## L0-full

| Operator | CPU before ms | CPU after ms | GPU before ms | GPU after ms | Submissions before/after | After max error |
|---|---:|---:|---:|---:|---:|---:|
| basic_tone | 6254.653 | 1215.304 | 861.009 | 329.800 | 74/74 | 4.56720591e-05 |
| curves | 3198.146 | 868.707 | 967.146 | 381.854 | 74/74 | 2.25193799e-05 |
| texture | 12359.319 | 3594.860 | 10071.138 | 4463.641 | 0/0 | 0.00000000e+00 |
| clarity | 7618.239 | 5913.021 | 7994.668 | 5533.867 | 0/0 | 0.00000000e+00 |
| dehaze | 23145.204 | 14421.410 | 15191.417 | 15593.259 | 0/0 | 0.00000000e+00 |
| sharpening | 1026.337 | 1046.623 | 1006.455 | 900.762 | 1160/1160 | 4.76837158e-07 |
| luminance_nr | 2189.038 | 1794.955 | 949.144 | 876.008 | 1160/1160 | 2.38418579e-07 |
| chroma_nr | 5176.870 | 4966.968 | 1042.976 | 937.763 | 1160/1160 | 4.05311584e-06 |
| vibrance | 2652.761 | 2767.838 | 368.200 | 374.552 | 74/74 | 5.00679016e-06 |
| hsl | 2458.007 | 2830.291 | 380.269 | 509.630 | 74/74 | 4.52995300e-06 |
| color_grading | 2475.997 | 2768.905 | 367.733 | 507.791 | 74/74 | 4.52995300e-06 |
| vignette | 924.873 | 931.580 | 551.736 | 685.622 | 74/74 | 2.38418579e-07 |
| grain | 1550.374 | 1200.387 | 582.220 | 560.922 | 74/74 | 7.21681863e-06 |
| crop_straighten | 18533.273 | 18849.023 | 17571.795 | 17546.262 | 0/0 | 0.00000000e+00 |
| local_blend_only | 80.892 | 61.934 | 106.915 | 108.788 | 0/0 | 0.00000000e+00 |

90 verified timing rows. Maximum same-input error: 6.22272491e-05.

Full-L0 presence remains CPU-delegated. Downsampled guidance, the requested hashed per-image LUT/map caches, and the remaining delegated geometry/local operations are not implemented. No universal <16 ms screen or <100 ms refinement guarantee is established.
