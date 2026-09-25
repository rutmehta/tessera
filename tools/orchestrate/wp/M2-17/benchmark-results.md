# M2-17 real NEF before/after operator measurements

RESULT: FAIL — these measurements do not establish the <16 ms screen / <100 ms regional-render targets.

## Method and provenance

- Before: immutable HEAD 17d5fb46cacd9ee0f7f7848f6b6d6d2bb85ea865, extracted into a disposable directory inside this work package. Only the new benchmark harness was copied into it. Baseline compiled from that snapshot and exited 0.
- After: `benchmark-after-rebuilt.log`, after explicitly cleaning pipeline-gpu/image-core release artifacts and recompiling the live worktree. Exit 0; source hashes remained unchanged during measurement. Subsequent edits moved unit tests, fixed a test initializer for Clippy, added a renderer capability query and Develop adaptive classification using it, and updated documentation/scripts. No measured standalone operator math changed.
- Same Nikon D800 NEF: SHA-256 26d58c21ed3019af5a22ef6bc02f017b863aad9eafcf84252ed375cd495c0efb. Active area 7378×4924; L2 1845×1231; Apple M4/Metal.
- Each cell is ONE timed sample after one warmup. This is a smoke comparison, not a statistically robust speedup estimate. Large changes in unchanged CPU timings show that conditions were not controlled across runs; no causal speedup is inferred.
- Serial CPU reference and GPU-backend wall time, including allocation, uploads, synchronization and pixel readback. Input cloning, RAW decode and upstream preparation are excluded. Each operator sees the same independent input. Not resident slider/IOSurface latency.
- L0-region-1024 is a standalone central crop of developed RGB, not a halo-aware regional renderer. Global dehaze statistics and effect coordinates are recomputed for that crop. It cannot prove the 1:1 refinement target.
- Submission counts include warmup and measured invocation. Zero indicates a retained CPU fallback. Local blend excludes adjustment/mask preparation.
- The earlier `after-final` log is NOT an after result: shared target-directory artifacts were stale after the baseline build. It is retained as diagnostic evidence only. Both runners now explicitly clean the two changed path packages to prevent recurrence. Initial concurrent/incomplete logs are likewise not used below.

## Measurements

### L2-full

| Operator | CPU before ms | CPU after ms | GPU backend before ms | GPU backend after ms | Submissions before→after | After max abs error |
|---|---:|---:|---:|---:|---:|---:|
| basic_tone | 121.465 | 78.885 | 37.488 | 22.467 | 6→6 | 2.623e-06 |
| curves | 56.361 | 56.469 | 60.344 | 24.158 | 6→6 | 1.431e-06 |
| texture | 337.695 | 380.983 | 128.056 | 122.631 | 8→8 | 3.219e-06 |
| clarity | 328.197 | 338.671 | 100.455 | 90.425 | 8→8 | 1.788e-06 |
| dehaze | 951.665 | 1281.235 | 231.531 | 233.079 | 12→12 | 1.669e-06 |
| sharpening | 64.685 | 63.733 | 66.259 | 59.909 | 80→80 | 4.768e-07 |
| luminance_nr | 118.776 | 309.947 | 60.770 | 168.725 | 80→80 | 2.384e-07 |
| chroma_nr | 295.349 | 1098.677 | 61.736 | 189.811 | 80→80 | 4.172e-06 |
| vibrance | 160.061 | 425.334 | 24.627 | 65.256 | 6→6 | 5.007e-06 |
| hsl | 160.230 | 534.844 | 27.364 | 70.058 | 6→6 | 4.172e-06 |
| color_grading | 159.737 | 488.628 | 26.233 | 49.958 | 6→6 | 4.053e-06 |
| vignette | 58.836 | 248.952 | 69.179 | 66.991 | 80→6 | 3.576e-07 |
| grain | 75.216 | 277.261 | 70.906 | 57.256 | 80→6 | 1.669e-06 |
| crop_straighten | 1144.474 | 3983.163 | 29.797 | 42.513 | 2→2 | 6.223e-05 |
| local_blend_only | 3.999 | 26.302 | 6.105 | 9.910 | 0→0 | 0.000e+00 |

### L0-region-1024

| Operator | CPU before ms | CPU after ms | GPU backend before ms | GPU backend after ms | Submissions before→after | After max abs error |
|---|---:|---:|---:|---:|---:|---:|
| basic_tone | 37.051 | 113.192 | 9.709 | 19.051 | 2→2 | 1.907e-06 |
| curves | 26.848 | 111.242 | 10.301 | 20.643 | 2→2 | 7.153e-07 |
| texture | 102.092 | 344.207 | 36.437 | 77.539 | 4→4 | 2.503e-06 |
| clarity | 156.773 | 490.010 | 44.512 | 61.105 | 4→4 | 1.073e-06 |
| dehaze | 389.552 | 831.786 | 91.985 | 210.225 | 8→8 | 1.341e-06 |
| sharpening | 28.618 | 73.933 | 26.572 | 79.470 | 32→32 | 2.384e-07 |
| luminance_nr | 52.933 | 112.906 | 24.294 | 62.842 | 32→32 | 2.384e-07 |
| chroma_nr | 135.032 | 445.797 | 25.386 | 74.372 | 32→32 | 2.146e-06 |
| vibrance | 68.378 | 145.722 | 10.658 | 21.151 | 2→2 | 2.742e-06 |
| hsl | 68.486 | 205.364 | 10.056 | 16.963 | 2→2 | 2.146e-06 |
| color_grading | 70.694 | 197.150 | 12.381 | 25.362 | 2→2 | 2.027e-06 |
| vignette | 26.726 | 68.754 | 48.210 | 26.178 | 32→2 | 2.384e-07 |
| grain | 36.284 | 85.376 | 33.928 | 21.532 | 32→2 | 9.537e-07 |
| crop_straighten | 583.552 | 1644.082 | 35.381 | 25.962 | 2→2 | 4.828e-06 |
| local_blend_only | 1.878 | 1.954 | 19.067 | 28.379 | 2→2 | 1.192e-07 |

### L0-full

| Operator | CPU before ms | CPU after ms | GPU backend before ms | GPU backend after ms | Submissions before→after | After max abs error |
|---|---:|---:|---:|---:|---:|---:|
| basic_tone | 2154.385 | 4140.828 | 809.763 | 797.596 | 74→74 | 4.567e-05 |
| curves | 1071.968 | 2850.293 | 553.805 | 874.926 | 74→74 | 2.252e-05 |
| texture | 4190.898 | 9630.964 | 3681.494 | 9120.995 | 0→0 | 0.000e+00 |
| clarity | 6030.723 | 17828.114 | 5614.895 | 10790.306 | 0→0 | 0.000e+00 |
| dehaze | 15617.054 | 15859.058 | 19314.314 | 14977.937 | 0→0 | 0.000e+00 |
| sharpening | 1042.708 | 1109.975 | 1081.132 | 1338.839 | 1160→1160 | 4.768e-07 |
| luminance_nr | 2071.161 | 2970.116 | 950.183 | 1051.839 | 1160→1160 | 2.384e-07 |
| chroma_nr | 4662.389 | 5357.605 | 957.341 | 1371.076 | 1160→1160 | 4.053e-06 |
| vibrance | 2523.848 | 2877.212 | 398.907 | 415.531 | 74→74 | 5.007e-06 |
| hsl | 3109.759 | 4126.094 | 402.368 | 545.968 | 74→74 | 4.530e-06 |
| color_grading | 2578.563 | 2691.141 | 409.567 | 432.880 | 74→74 | 4.530e-06 |
| vignette | 1001.899 | 1092.186 | 1101.462 | 552.968 | 1160→74 | 2.384e-07 |
| grain | 1256.931 | 1573.016 | 1184.989 | 540.201 | 1160→74 | 7.217e-06 |
| crop_straighten | 19537.074 | 18906.570 | 19820.431 | 30700.186 | 0→0 | 0.000e+00 |
| local_blend_only | 61.890 | 64.783 | 94.606 | 98.152 | 0→0 | 0.000e+00 |

## Conclusions

All 90 reported before/after same-input errors are <=1e-4 (maximum 6.22272491e-05). This does not establish full-frame multi-stage or golden ΔE tolerances.
Effects now batch into the point-stage shader (L2 submissions 80→6 for warmup + one sample). Resident fusion and X-Trans transfer reduction are separately asserted by executable tests. These standalone timings do not measure their warm resident benefit.
Texture/Clarity/Dehaze still fall back at full L0 because packed vec4 buffers exceed the requested storage limit. Extended geometry and oversized local blend also remain delegated. No downsampled-guidance approximation or target latency claim was introduced.

## Reproduce

    python3 tools/orchestrate/wp/M2-17/benchmark-baseline.py
    PIPELINE_BENCH_SAMPLES=1 python3 tools/orchestrate/wp/M2-17/benchmark-run.py after-rebuilt
    python3 tools/orchestrate/wp/M2-17/benchmark-compare.py

Use a fresh label to retain existing evidence. The fixture can be supplied with PIPELINE_BENCH_NEF. Both runners retain the required external CARGO_TARGET_DIR. Missing RAW/Metal fails explicitly; no synthetic substitution.
