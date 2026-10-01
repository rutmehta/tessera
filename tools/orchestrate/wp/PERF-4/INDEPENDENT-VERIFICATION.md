# PERF-4 independent evidence verification

Scope: read-only check of saved evidence in `/Volumes/betterSSD/tessera-validation/perf4` for baseline `67e257db` and final `04e481cd`; no reruns/builds. Worktree `/Volumes/betterSSD/tessera-worktrees/codex-lr-0-inventory` HEAD is `04e481cd4014309df5e3ca31d4693534a56b5fd6` (`codex/perf-4-gaussian`). At review, only `tools/orchestrate/wp/PERF-4/HANDOFF.md` was modified; product and test files matched that HEAD.

Final full release filters suite (`final-04e481cd-filters-release-full.log`, exit 0): 150 passed, 10 ignored, 0 failed across 28 test harness result lines, including unit, integration, and doc tests. Log SHA-256 matches its sidecar: `0e84c24d215f2dcdec9140b3aaa5b607f29f320b8e1f0dcf016a1b4387420765`. Final clippy and fmt logs also have exit 0 and matching sidecars.

Numerical evidence: the final 6000x4000 sigma=12 paired convolution benchmark reports median baseline 1938.054 ms / optimized 867.070 ms = 2.235x, with matching baseline/optimized digest `14666caab0baf1c8`. Public apply reports medians 2180.016 ms baseline and 1086.382 ms final, approximately 2.0067x, with matching digest `245b9465587756d3`. The prior baseline paired benchmark itself exited 101 because the old timing assertion saw 0.966x; the per-trial numerical digests matched, so this is a performance-threshold failure, not a correctness failure.

Caveats: host is Apple M4 / 24 GiB; load averages varied and were elevated (baseline 2.79/4.08/4.52, final convolve 3.73/3.14/3.60, final apply 3.43/3.12/3.57), so report these as same-host diagnostic measurements, not isolated performance proof. The initial no-host direct log was overwritten; its raw original is not preserved. No `finished.log` exists; the final per-command logs and sidecars were verified.
