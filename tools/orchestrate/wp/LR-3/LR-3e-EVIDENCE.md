# LR-3e verification evidence

Continuation verification on 2026-10-01, initial source HEAD `3c3fe55c496578fcda01e0315ce035fcc2ca695f`, base `38684d9b` (includes LR-SCHEMA `02ae8196`). The first broad gate exposed a remaining MCP sampling-order issue. Test commit `48a00d45` and fix commit `c967a490` close it; the final full gate runs on `c967a490`.

## Reviewed requirements

- B2: `import-lrcat/tests/retouch.rs::lr3d_import_is_one_history_entry_and_approximate_without_warnings` imports Exposure2012=0.75 with each retouch alias, checks one history entry, `Import XMP` / `xmp`, retained source, no warnings, shared diagnostics and schema 4.
- Diagnostics: `retouch::translate` uses the shared helper with `/settings/locals/retouch`. `translation_matrix.rs` is byte-identical to the LR-DIAG base. All 107 base matrix rows remain; only RetouchAreas and RetouchInfo rows change.
- N1: `pipeline-cpu::retouch::apply_retouch_scaled` reduces only the solve, lifts changed-cell deltas into the original image, and preserves downstream resolution/crop/depth/scale. Injected and real heal/clone tests check outside-support bits at scales 2 and 4 on odd dimensions. The real test states a radius plus 32 input pixels and two sampling cells as its margin; global dehaze is not covered by that locality claim.
- N2: session-owned OnceLock retains CPU RGB and Upright caches, current request state is snapshotted, and capability replacement resets the fallback. Internal and Metal-selected session tests count L0 solves.
- N3: merge and sidecar equality tests assert expected written schema before normalization. The retouch predicate checks settings and history.base; both have bumped-only-when-present tests. Base schema remains 3. The coordinator must retain LR-7d's shared checklist at integration, as explained in HANDOFF.
- No Cargo manifest/lockfile delta relative to the base. No board, Swift, app, personal catalog, or remote-write operation.

## Preserved timing evidence from the interrupted run

Verified directly against `/tmp/lr3e/red-metal.log` and `/tmp/lr3e/green-metal-real.log`. These are prior-run samples, not measurements newly collected by the continuation.

Actual Metal-selected session, 768x512 synthetic RGB, L2, one clone + Upright Level, rotations 0–4 degrees, unoptimized test profile, jobs=3, Rayon=3:

| Frame | Before ms | After ms |
| --- | ---: | ---: |
| 0 (cold) | 2595.034500 | 5796.349542 |
| 1 | 1077.959792 | 315.531167 |
| 2 | 1246.036375 | 271.855084 |
| 3 | 1323.126666 | 245.193042 |
| 4 | 1099.288625 | 240.286542 |

Steady median: 1172.662500 → 258.524063 ms, approximately 4.54x. L0 solve counts: `[2,3,4,5,6]` → `[2,2,2,2,2]`. Cold performance did not improve in these samples. Concurrent work on the shared machine makes these illustrative timings, not isolated release benchmarks. No threshold was changed.

## MCP follow-up found by the full gate

The existing MCP test expected graph previews to match the former CPU pipeline's early reduction. After N1, direct CPU previews keep downstream stages at source resolution while graph previews use their normal requested pyramid level. The test now compares each with its appropriate reference, retaining exact requested-resolution output checks.

Investigation also found that MCP selected a different size depending on whether a spot was active. New exterior-bit tests failed at (0,0), distant from the spot: RGB 161 → 181 and graph 161 → 176. The fix removes both active-spot size switches; requested maximum size determines the RGB scale and graph level for empty and retouched recipes alike. This also changes empty MCP previews from the prior fixed-1024-level/final-resize behavior to the requested sampling path.

The tests fix the backend to CPU to isolate resolution selection from the explicitly retained GPU-to-CPU fallback's arithmetic differences. They compare equivalent cold caches, since graph WB checkpoints are deliberately stored as f16; cold-vs-warm comparisons would include unrelated cache quantization. The real brush bridge runs on textured 512x384 input at 128x96 with nondefault contrast/clarity/texture/vibrance. The exterior margin is radius 0.02 of the short edge plus 32 input pixels and two output sampling cells. Both exterior tests and the original requested-resolution regression pass, along with all nine preview tests (20.68 s test execution).

## Initial full gate and investigation

The initial full test command on `3c3fe55c` ran 5219.368 s and exited 101 with three failed targets: FFI Develop, FFI document Liquify, and MCP lib. The MCP failure is fixed as described above. Workspace clippy passed (143.280 s); fmt passed (5.956 s).

- Export-slider broad run: 100/120 frames at L2 (required at least 108); render p90 11.9 ms (under the 16 ms bound), five exports succeeded. Serial rerun also failed L2 retention (61/120 L2, render p90 6.5 ms, five exports succeeded); its exact samples remain in `/tmp/lr3e/gate-export-slider-serial.log`.
- Liquify broad run: p95 467.2 ms, median 252.1 ms; serial rerun p95 434.6 ms, median 266.7 ms. Both failed the unchanged latency threshold. These are shared-machine observations; a serial test thread does not isolate other processes.
- Logs: `/tmp/lr3e/gate-*.log`, commands/elapsed times `/tmp/lr3e/gates.json`; MCP RED `/tmp/lr3e/red-mcp-exterior.log`, GREEN `/tmp/lr3e/green-mcp-preview2.log`.

## Final gates

Final full clean-build verification on `c967a490`, 2026-10-01. No source changed after this run began.

| Gate | Wall seconds | Exit |
| --- | ---: | ---: |
| clean | 11.516 | 0 |
| test | 4304.505 | 0 |
| clippy | 51.123 | 0 |
| fmt | 2.026 | 0 |
| liquify-serial | 74.940 | 101 |
| export-slider-serial | 90.192 | 0 |

The full 12-crate test run passed: 1,563 top-level tests passed, zero failed, 60 repository-marked ignored tests. The log totals 1,565 passes across 266 result summaries because two protected-source restart tests spawn helper test processes. Those helpers each report nine filtered tests; the outer gate itself had no test-name or `--skip` filter. Repository RAW fixtures were included.

All LR-3e regressions passed in this full run: mixed exposure/retouch provenance, shared diagnostics guard, settings/history-base schema predicates, scaled CPU exterior bits, real heal/clone locality, CPU session analysis reuse, Metal-selected spot + Upright frames, and both MCP exterior paths. The original MCP requested-resolution regression also passed. `export_batch_does_not_starve_slider_drag` and Liquify's 20MP latency test both passed in the broad run. Successful broad tests do not print captured timing output, so no precise broad p95 is inferred from that pass.

**The full suite, clippy, and fmt are green; the additional serial Liquify gate is not green.** Its final p95 was 266.6 ms against the unchanged `<250 ms` threshold (median 179.5 ms, max 336.5 ms, open 2677 ms, full-resolution apply 2124 ms). Test execution was 37.44 s; the command wall time above also includes compilation. The initial broad/serial samples were 467.2/434.6 ms. No threshold was relaxed and no repeat was run to obtain a passing sample. Serial test threads do not isolate other work on the machine; this remains a reported performance limit, not a claim that all gates passed.

The final serial export-slider run passed: 120/120 frames at L2, idle render p90 3.1 ms, during-export render p90 4.7 ms (p50 2.7 ms, max 13.3 ms), set-to-frame p90 7.9 ms; five exports succeeded in 39.339 s. Test execution was 87.37 s. Earlier L2-retention failures remain recorded above rather than replaced by the later pass.

Environment and exact commands, executed sequentially:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/LR-3-retouch"
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
cargo clean -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p brush -p image-core -p export -p previews -p tessera-mcp -p sidecar -p merge -p tessera-ffi
cargo test --locked --no-fail-fast -p import-lrcat -p engine-api -p pipeline-cpu -p pipeline-gpu -p brush -p image-core -p export -p previews -p tessera-mcp -p sidecar -p merge -p tessera-ffi
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo test --locked -p tessera-ffi --test document_liquify_ui brush_latency_on_a_20_megapixel_layer -- --exact --nocapture --test-threads=1
cargo test --locked -p tessera-ffi --test develop export_batch_does_not_starve_slider_drag -- --exact --nocapture --test-threads=1
```

Raw logs are `/tmp/lr3e/final-gate-{clean,test,clippy,fmt,liquify-serial,export-slider-serial}.log`; the machine-readable command/elapsed-time ledger is `/tmp/lr3e/final-gates.json`. The results and failure measurements are preserved in this committed document so the handoff does not depend on temporary-log retention.

Final checks: `git diff --check` passed; Cargo manifests and Cargo.lock have zero delta from `38684d9b`; the shared matrix guard is unchanged; all 107 base matrix rows remain. Only local source/test/docs commits were made. The existing untracked `LR-RULINGS-FROM-A.md` was left untouched.
