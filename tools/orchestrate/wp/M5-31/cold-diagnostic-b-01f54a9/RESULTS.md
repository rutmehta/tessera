# Passive cold-render diagnostic on Machine B

Diagnostic commit `01f54a9b1e65253a68e6ae7ce13c0770ba40c1fd`, branch `codex/m531-cold-diagnostics`, based on product 441da3e plus evidence-only 97eb4ca. This is diagnostic evidence, not acceptance. The original six-run gate remains failed at 106.084084 ms.

A directly ran B through authenticated SSH. Separate no-run release compilation passed (exit 0, 35.019 s). Exactly one original and one unique-ID fresh-process test ran with TESSERA_COLD_DIAGNOSTIC=1; both passed unchanged assertions. No extra warmup, fences, feature toggle, GPU queries, fourth/replacement samples, or production fix. Output was deferred until after the warm timer; CPU spans themselves add small measurement overhead. All 147 recorded events per process fit within the 4096-event cap.

| Measurement (ms) | Original | Unique IDs |
|---|---:|---:|
| CPU reference cold | 140.168333 | 89.853667 |
| CPU reference warm | 176.096750 | 85.881083 |
| Resident cold, original timer | 41.824334 | 42.780375 |
| Resident warm | 4.712875 | 3.912375 |
| CPU render call span | 24.524 | 14.128 |
| Final device-poll wait span | 17.282 | 28.650 |
| All style preparation span (nested in render) | 23.730 | 13.630 |
| First source materialization | 8.224 | 4.738 |
| First source render call | 9.308 | 5.312 |
| Conversion call, including lazy pipeline | 3.292 | 1.425 |
| Effects pipeline initialization | 1.798 | 0.810 |
| Five effect encode/submit calls, sum | 8.718 | 5.600 |

Nested spans overlap and cannot be summed into a total. The five source calls after the first are individually <=0.097 ms; key generation is <=0.016 ms each. The source reuse behavior remains active for both fixtures.

Two background specializations overlap each cold interval. Original: first worker starts +8.824 ms, lasts 6.303 ms; root worker starts +23.945 ms, lasts 9.496 ms. Unique: +5.003/4.112 ms and +13.708/5.531 ms. Most recorded worker duration is WGSL translation (3.871–8.233 ms), while create_compute_pipeline spans are 0.066–0.456 ms. Both workers finish before final cold completion. This demonstrates overlap, not that the workers cause latency or block the device. The historical 106 ms outlier was not reproduced, and no long synchronous pipeline-creation stall appeared in these samples.

The largest remaining measured interval is final wait: 17–29 ms after CPU encoding returns. That interval is queue drain plus poll/scheduling overhead; CPU preparation and prior GPU work overlap. These spans cannot determine individual GPU effect/mip/composite time or attribute host scheduling. No production change is justified by these two samples. A specialization-off experiment is not yet supported by evidence of a long compiler stall. If the next diagnostic is authorized, supported per-pass GPU timestamps would distinguish GPU execution from queue/poll delay; keep query readback outside the original measured interval and continue recording passive CPU spans. It would still not retroactively explain the prior outlier without reproducing it.

Host: M4 Max, macOS 26.1 (25B78), same cache and two build jobs as prior gate. Load averages original 16.62/15.62/14.91, unique 17.29/15.77/14.97. Existing yes PID27454 remained at ~85–91% CPU; other apps remained untouched. pmset recorded no thermal/performance warning level, which does not prove a cold or idle host. Host load is recorded condition, not an asserted cause or waiver.

Completed 2026-09-27T18:21:42.315200Z; B heavy slot released. B checkout clean detached at diagnostic SHA; wp/B5-16a still bcd0e792f248b3d5ce20ea4a121b9c20e61c7d6c. Original A wp/M5-31 remains 97eb4ca; diagnostic checkout owns only its new branch. Gain-map checkout untouched.

Evidence: manifest.json commands/exits/environment; compile.log; original-1.log/unique-1.log complete deferred spans; corresponding host samples; host-toolchain.txt; source.patch; remote-runner.py; analyze.py and spans.json. Eleven remote evidence files copied to A and byte-verified against remote SHA256 values in sha256.json. The current ownership note was copied separately; previous completed note preserved. This was not a B chat receipt.
