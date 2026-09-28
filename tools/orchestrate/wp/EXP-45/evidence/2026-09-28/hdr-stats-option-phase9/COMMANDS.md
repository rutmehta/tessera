# Phase 9 command and provenance

Probe source: `probe/imageio-probe-stats.m`; SHA-256 `5cfcb528ba3ea5da4698999cc6c1f46c43c753853fe4f370554dedd3213df80b`. Compiled binary: `probe/imageio-probe-stats`; SHA-256 `f7613645cb60c4866d8c332075f3badec4794cb3c3fb0532264082812d76592b`. Runner: `probe/run_stats_comparison.py`; SHA-256 `8d8de47680f0687f658893826779e39d2b54ad14d95d374b9b3ba1e501d25839`.

Compile command (direct exit in `probe/compile.direct.exit`):

```sh
xcrun --sdk macosx clang -O2 -fobjc-arc -framework Foundation -framework ImageIO -framework CoreGraphics imageio-probe-stats.m -o imageio-probe-stats
```

Run command (direct exit in `run-stats.direct.exit`):

```sh
python3 probe/run_stats_comparison.py
```

The runner processes each fixture and each option state in a separate process/output directory. It clears inherited decode overrides, uses only `PROBE_HDR_STATS=1` for enabled runs, verifies exact input hashes against phase8, then checks each default decode against phase8 before launching the enabled pair.
