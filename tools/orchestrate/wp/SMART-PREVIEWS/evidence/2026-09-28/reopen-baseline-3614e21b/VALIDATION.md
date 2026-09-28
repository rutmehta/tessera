# Unchanged Smart Preview reopen baseline — qualified harness, no cache

Immutable harness checkpoint **3614e21bee508563432a9d235964b76e3c43a02f**, branch codex/proxy-reopen-baseline; base03651eaef1610907b1e495c8f92581d3d6e60c3f. Clean frozen checkout. Product behavior/defaults unchanged; only two native test-source files differ. No cache implementation or candidate timing.

01compile direct0/frozen on c77e0543.02fmt direct1/frozen retained, only harness formatting. Formatting-only commit3614e21b followed by03fmt0,04release/all-target strict Clippy0 and05four-process0, all frozen on finalsource. No full FFI regression-suite claim. Native vendor warnings remain logs. Every Cargo command has fixed BetterSSD target, deployment15 and jobs2; runner removes inherited TESSERA_* and adds explicit qualification whitelist. Independent source review and corrections are retained; runtime review pending.

All **8,640 final Git blobs** independently checked against final03/04/05 full input maps by implementer; raw checkpoints permit independent verification. Each child source/fixture/runner/threshold map froze before/after; wrapper also freezes entire tracked/untracked nonignored checkout. Fixture SHA256bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8 unchanged. Tests use disposable RAW copies and assert original/copy, journal/pixels and recipe unchanged each cycle.

## Baseline observations

Four serial fresh processes, each one Engine with initial open plus five unchanged reopens:24actual final frames, not24independent processes. Fixture copy/build warms filesystem; fresh process does not mean cold filesystem/driver. All output L1,820x546 raw, requested viewport640x426. Auto selected AppleM4 Metal each cycle and each frame has matched resident receipt/submission proof; forcedCPU has zero frame Metal submissions. Zero timed GPU pixel readback all24; post-timing readbacks retained separately. Static selected-backend labels alone were not used as execution evidence.

Medians below use the five unchanged reopens. Metric is public open through matching-generation final IOSurface callback, not physical input-to-screen or display presentation. Component medians need not sum to median total.

| Route/output | Open return ms | Post-open callback ms | Combined ms | Reopen CV |
|---|---:|---:|---:|---:|
| proxy-auto sdr | 640.148 | 5.335 | 645.563 | 5.21% |
| proxy-cpu sdr | 32.281 | 134.790 | 167.071 | 1.11% |
| proxy-auto edr | 584.903 | 4.964 | 589.431 | 1.30% |
| proxy-cpu edr | 33.623 | 131.835 | 165.954 | 8.61% |

All four cohorts have identical settings/backend/resident-route/levels/render+display dimensions, and CV<=20% preregistered baseline stability gate. The auto reopen cost remains material; this motivates testing decision reuse but proves no proposed cache benefit or first-ever-open improvement. No candidate was run and thresholds were not changed.

Twelve CPU-reference versus auto matched-dimension/settings/level pixel comparisons passed unchanged gates: SDR abs<=4/255, EDR abs<=0.002+0.002*abs(reference). Observed maximum SDR0.003921568393707275, EDR0.0009765625; EDR retained above-white samples. No resampling into a like-quality pass, no broad perceptual equivalence or cross-camera claim.

All24cycles closed and released Weak Shared/Renderer/optionalGPUops and ownedIOSurfaces within bounded5s assertion. Instrumentation strongArc cleared first. Engine retains its intentional shared device; no process/RSS/global-memory cap claim. Per-row release_ms is observed after post-release source checks and therefore is a conservative inclusive diagnostic, not precise renderer drain time.

Per-cycle measured JSON precedes route/pixel/cleanup assertions; successful aggregate rows separately record validated_and_released. Failure02fmt and all logs retained. Raw RGBf32 pixel files remain in host-local05-measurements; portable evidence includes their size/hash inventory rather than duplicating128MB of pixel payload. No originalRAW/archive/build product is bundled.

## Evidence and next boundary

FINAL-CHECKPOINT.json binds finalsource, directexits, frozenmapresults,24row/route/releasecounts and12fidelitypairs.05-measurements holds complete percycle timestamps/settings/dimensions plus first-open samples, commands, environment and hashes. source-preparation and source-preparation-v2 preserve original/revised reviewed proposals/thresholds; final-candidate.patch is formatted finalsource. Previous preparation commits remain distinct.

Sole runtime lane released to coordinator after05; no more compiler/GPU/app work. Branch remains frozen pending independent verification. Any cache experiment or new runtime requires separate coordinator authorization and the accepted plan; no acceptance thresholds are retrospectively adjusted by this baseline.
