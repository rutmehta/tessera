# Commands and provenance

External run root: `/Volumes/betterSSD/tessera-validation/exp45-independent-control/d52a0d13814ca399fc8a07e23de1d2c63f0e8404/size-640x128/`.

The phase8 runner was invoked separately so each file had a separate process/output directory and warning capture:

```sh
python3 probe/run_geometry_probes.py baseline
python3 probe/run_geometry_probes.py scaled
```

`baseline` reran the three original 80×16 inputs first and checked them against the prior capture. Only after that guard passed did `scaled` run the 640×128 uniform and split controls. Native process commands and direct exit files are retained under `native/<variant>/<tool>/`.

Probe source hashes and compiled binary hashes are recorded in `native/native-geometry-manifest.json`. The runner source is `probe/run_geometry_probes.py`; `probe/run_geometry_probes.preflight-rejected.unrun.py` is preserved as a source-review-rejected runner and was not executed. Scaled fixture generation, pinned encoder invocations, explicit ISO parser, reference decoder commands/exits, and reference qualification are preserved under the root/`qualification/`.
