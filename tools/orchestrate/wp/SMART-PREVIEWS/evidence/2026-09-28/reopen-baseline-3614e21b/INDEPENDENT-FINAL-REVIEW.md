# Proxy reopen baseline — final independent review

**Approved: bounded baseline evidence at `3614e21bee508563432a9d235964b76e3c43a02f`.** No actionable evidence or route discrepancy found. This review is read-only; no compiler, app or GPU workload ran.

Independently streamed Git blobs and compared every recorded input: 8,640 files for each gate. 01 compile exit0 and preserved 02 formatting-check exit1 match preformat `c77e0543`; 03 formatting, 04 strict and 05 four-process wrapper each exit0 with unchanged before/after maps exactly matching final3614. Inspected the two-file formatting delta; do not describe 01 as compiled final bytes. The subsequent four runtime processes compile/test final source. Runner, threshold and original fixture SHA256 match their frozen records and current files. The original Sony fixture hash remains bf4c6d21136aa4fd626212fe72b962b6404e3fca45cdc3b6afbed8e73fee2cf8.

All four direct child exits are0. Each executed one opt-in test, with six opens in one Engine: initial plus five unchanged reopens, 24 total. Independently compared measured-phase records to validated/released records and verified open + post-open timing equals combined timing. Source timestamps callback entry and accepts the first matching-generation FINAL callback, not channel dequeue or physical presentation. Pixel extraction is after that timestamp.

| Cohort | Reopen combined median ms | Population CV |
|---|---:|---:|
| Auto SDR | 645.563000 | 5.2058% |
| CPU SDR | 167.071208 | 1.1101% |
| Auto EDR | 589.430667 | 1.2980% |
| CPU EDR | 165.954083 | 8.6086% |

Recomputed from the five raw reopen rows, excluding initial. Each cohort has a stable contract and CV below20%. Matched auto/CPU pairs have equal settings, viewport, render/display extent and level. All24 actual frames are L1,820×546 for requested640×426; calibration is L0 and is not confused with actual-frame routing. Auto selected AppleM4 Metal with resident receipt and positive post-calibration submissions. CPU has no resident receipt or submissions. All have zero timed GPU pixel readback. This supports actual route claims rather than names alone.

Independently reread all24 saved RGBf32 buffers and compared12 matched pairs. All samples finite; SDR maximum absolute difference0.003921568393707275 <=4/255; EDR maximum0.0009765625 with every sample within0.002+0.002*abs(CPU). EDR buffers contain above-white samples on both routes. No resizing/tolerance relaxation used.

All24 rows completed resource-release assertions: Weak Shared/Renderer/optional GPU stage operations and owned IOSurface lookups disappear after listener/session/ring/instrumentation release. Source bounds the drain loop at5s. Recorded release_ms includes subsequent hash checks, so it is conservative diagnostic time rather than exact drain latency; observed maximum28.846667ms. Engine's intentional shared GPU device is outside that per-session release claim. Each cycle also asserts unchanged journal/proxy/fixture/copy and full recipe.

The failure02 log remains preserved. Independent verifier initially assumed per-child before.json; the runner actually shares the global before freeze with per-child after records. Corrected this read-only verifier assumption and verified equality; no product evidence changed.

Scope: four baseline processes on one Sony fixture/AppleM4, not five paired candidate trials, a cold-filesystem test, proxy-versus-Original speedup, edit latency, cache benefit, physical display latency, cross-camera qualification or memory cap. No cache candidate exists. These results qualify this baseline/harness only and motivate a separately authorized decision-reuse experiment. Final source/fixture/runner digests and recomputed samples are in the accompanying JSON.
