# Task 3 performance qualification: failed

The fixed22-process sequence completed once using the accepted functional source/runner/binaries. Every child exited0 and source/artifact/fixture/runner freezes matched. The outer runner exited1 at the final preregistered benefit/regression assertion. No selective rerun, order change, source modification or threshold change occurred. Runtime was released immediately.

Both SDR/EDR matched Metal/L1/820x546 cohorts, baseline stability and132open+66edit resource/route checks passed.180 CPU-reference comparisons passed (maxSDR0.003921568393707275, EDR0.0009765625). Candidate hits had no further calibration. These functional results do not override the failed performance gate.

| Metric, median ms | SDR baseline | SDR candidate | EDR baseline | EDR candidate |
|---|---:|---:|---:|---:|
| Same-Engine reopen to final callback |590.619|67.426|589.635|66.600|
| First open to final callback |623.645|631.549|621.760|616.543|
| Warm exposure edit |4.863|6.150|4.954|5.614|
| Warm white-balance edit |5.016|14.738|4.850|14.446|

Reopen and first-open gates passed. Exposure passed. White-balance regression failed in both formats: candidate exceeds baseline+max(2ms,10%). Overall candidate is NOT performance-qualified and must not be promoted on the reopen result alone. Baseline pooledCV SDR0.03452, EDR0.02624. Exact raw rows, per-process timings/settings/routes/counters, pixels, freezes and aggregate calculations remain under03-performance.

Measurements concern listener delivery, not physical presentation; fresh processes do not imply cold filesystem caches. The cause of warmWB regression has not yet been diagnosed. No cache repair or new runtime is authorized by this report.
