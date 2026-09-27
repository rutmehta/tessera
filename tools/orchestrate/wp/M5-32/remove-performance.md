# M5-32 Remove performance, round 3

CPU/Auto-without-model Remove now bounds PatchMatch to the stroke coverage plus a 128-pixel sampling margin and dilation, rounded outward to source tile boundaries. Explicit Rect/Custom sampling expands the crop to include every requested donor. The output shares unchanged canvas tiles; touched tiles retain canvas coordinates. This is not a bound on the size of a large/disconnected selection or widely separated explicit donors.

CAF also uses a conservative early-rejection SSD bound and an interior translation row fast path. Accepted candidate accumulation order is unchanged.

Parent verification of the compiled release benchmark executables (external target directory `/Volumes/betterSSD/tessera-cache/target/M5-32`):

- `m532_remove_perf`: CPU, 6000x3000 canvas, 300x300 stroke, dilation 2, default CAF: **757.424 ms**, passed `<1s` assertion.
- `remove_patchmatch_roi`: Auto without model, same dimensions, default dilation: **807.665 ms**, periodic-texture masked-region MAE **0.000000**, passed quality and `<1s` assertions.

Raw output: `round3-perf-direct.log`. These are synthetic periodic-texture fixtures, not measurements on a real photograph. Timings exclude fixture construction and include the Remove call. Concurrent builds were running on the host during the parent measurement.

The implementation worker separately reported 6738.580 ms before optimization and 148.255 ms CPU / 130.136 ms Auto after optimization. Those earlier measurements are worker reports, not the parent's independently captured baseline. Existing CAF quality and all filters release tests passed in that worker; the full package gate is recorded separately in `round3-gate.log`.
