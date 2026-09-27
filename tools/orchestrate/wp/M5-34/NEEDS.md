# Acceptance environment needed

Implementation and regressions are present and uncommitted. Performance
acceptance is not complete: all three final default-environment runs missed
the wall-clock target while the shared host had load averages above 160.
A timed follow-up took 7.57s wall versus 1.71s CPU, with load average 189.
An idle benchmark window was requested; do not treat the earlier successful
single-worker diagnostic as acceptance evidence.

Parent follow-up:

1. Pause unrelated builds/benchmarks and run the ignored release benchmark on
   an idle host. The prepared real-photo RGB input is `/tmp/m5-34-photo.rgb`;
   preparation instructions and hashes are in RESULTS.md. No Rayon environment
   override is required or intended for acceptance.
2. Styled-layer root damage is now halo-bounded, with nested-effect, tile-seam,
   mask, global-light and undo regressions. The existing whole-source style
   evaluation barrier remains unchanged; styled latency is not claimed.
3. Wire resident damage/upload scheduling in M5-31 through the existing
   `live_tile` source hook; resident files were not modified here.

The parent independently ran the exact required gate outside the worker
sandbox: **314 passed, 0 failed, 11 ignored**, followed by passing clippy and
fmt. Evidence: `evidence/parent-gate.log`. Earlier missing-Metal results were
sandbox-specific, not a remaining gate blocker.

The parent's final benchmark also missed acceptance at host load 158.71:
typing L2 median/p95 14.965/102.478ms, shape L2 66.524/110.997ms,
typing L3 1.952/50.108ms, shape L3 6.982/90.953ms.
Evidence: `evidence/parent-latency.log`. The performance target is not verified.

Latest retry: the exact gate passes with 316 tests passed, 0 failed, 11 ignored,
followed by clippy and fmt (evidence/retry-gate.log). The real-photo benchmark
still fails at host load 93.08: typing L2 median/p95 26.124/124.949ms, shape L2
86.094/162.312ms, typing L3 1.355/55.552ms, shape L3 9.559/121.866ms.
See evidence/retry-latency.log. An idle acceptance run is still needed.

Clippy with -D warnings and cargo fmt --check pass. No document/edit or resident
files were modified; no commits or pushes were made. Every Cargo command used
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-34.

Current verification retry: no production changes. The exact required gate
again exits 0 (316 passed, 0 failed, 11 ignored; clippy and fmt pass), recorded
in `evidence/current-gate.log`. The default-thread real-photo benchmark still
fails: typing L2 26.008/92.236ms, shape L2 57.150/124.195ms, typing L3
1.360/34.030ms, shape L3 14.464/74.162ms median/p95. See
`evidence/current-latency.log`. The host reported 0.0% idle CPU and 65 running
processes afterward. Performance acceptance still requires an idle run;
unrelated processes were not modified.
