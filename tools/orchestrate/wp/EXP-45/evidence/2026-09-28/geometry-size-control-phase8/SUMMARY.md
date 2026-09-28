# EXP45 geometry-size control (phase 8)

This isolated diagnostic compares the pinned phase7 80×16 controls with exact 8× nearest-neighbor 640×128 copies of the white base and uniform/split gain maps. The run changes image geometry only; it does not alter the producer, decode options, renderer, or product source.

## Results

- The 80×16 uniform ImageIO control reports headroom 16; the split control and original Tessera control report 8. At 640×128, uniform remains 16 (RGB maximum 15.951576) and split remains 8 (RGB maximum 7.983763). Core Image reports approximately 16 for both uniform and split at both sizes.
- The reference decoder remains peak 16 for both scaled controls. The base bands and extracted auxiliary gain samples were qualified after scaling; each scaled image is exact nearest-neighbor replication of its 80×16 source.
- Thus the 80×16 extent alone is not sufficient to explain the split control's ImageIO headroom result. This does not establish a general ImageIO limitation or resolve the original acceptance failure.
- Warning attribution was captured per file/process. Uniform emitted “too few samples” warnings (2 at 80×16 and 5 at 640×128); split emitted none at either size. Warning count does not track the measured headroom here.
- ImageIO ordinary source properties report `Headroom=16` for every source, including split and original controls whose returned decoded image headroom is 8. Returned ICC bytes and properties plists are retained per input. This is a measurable distinction, not an explanation.

## Qualification history

`qualification/attempt-01-qualification.py` is the preserved first harness attempt and `qualification/attempt-01.exit` records its nonzero exit. It failed on a row-slice assertion in the checker. The corrected `qualification/qualify_reference.py` and `reference-and-sample-qualification.json` record successful fixture qualification; this was a harness correction, not a failed fixture.

## Reproduction and limits

See `COMMANDS.md`, the exact source probes and runner under `probe/`, per-process native outputs under `native/`, scaled fixtures, and the raw payload hash/length index. Large provider and floating-point pixel arrays are omitted from this portable checkout; they remain in the external phase8 directory named in `native/native-geometry-manifest.json`, with hashes and byte counts recorded. The root independently verified all 40 saved provider/ImageIO float/ICC/CoreImage float payload hashes and lengths and recomputed all global RGB maxima. All 10 native process direct exits were zero.

No acceptance tolerance was changed. The original ImageIO acceptance result (4 pass, 1 fail) remains unchanged and unresolved. This phase is a bounded diagnostic only.
