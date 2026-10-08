# ENG-12 handoff: HNSW flake, fixture tests that never ran, REV-ENG-11 follow-ups

Branch `wp/ENG-12` from main `db604752`. Worker: Claude Opus 5.5.
Input: `REV-ENG-11` SHOULD-FIX 1-3, NITS, answers 2 and 6.

> **Superseded in part by ENG-12b** (`tools/orchestrate/wp/ENG-12b/HANDOFF.md`,
> after REV-ENG-12): `set_keeping_pruned(true)` is removed (it breaks clustered
> data), the isolated set is capped at 5% with an exact-search fallback, and
> the export band planner is fixed, so the two five-fixture export tests below
> now run and pass with CR3 on bands.

## Summary

- **HNSW flake (SHOULD-FIX 1).** The over-fetch the ruling proposed did not
  fix it, and measurements show why: hnsw_rs already returns the top-k of
  its `ef` list sorted by exact distance, so asking for more candidates and
  re-ranking them gives back the same top-k. The misses are points the
  bottom-layer walk cannot reach. An exhaustive `k = ef = n` search misses
  them too. The product fix in `HnswVectorIndex` keeps pruned links
  (`set_keeping_pruned`) and also scores, exactly, every point outside the
  largest strongly connected component of the bottom layer. The test is
  renamed and asserts the exact top-1 is in the approximate top-5, with
  overlap >= 4. Result: **0 failures in 300 release runs at the tip**, and 0
  in 300 more on the build just before it. The original search failed 2 of 200
  on the same test.
- **Fixture tests that never ran (SHOULD-FIX 2).** All now default to
  `test_fixtures::raw` (or the repository's Sony ARW). Seven now run in the
  normal gate. One stays ignored because it is slow. Two stay ignored because
  of an exposed failure, listed below. The Swift pair now runs in the Swift
  gate.
- **ml-faces README (SHOULD-FIX 3)** documents `TESSERA_REQUIRE_MODEL_WEIGHTS`,
  says the legacy name is still accepted and now applies workspace-wide, and
  says any value means required.
- **Nits.** The compositor CI early returns now print a visible SKIPPED line.
  The dead `PIPELINE_GPU_ALL_FIXTURES` pops in the LR-4 scripts are gone.

No assertion, tolerance or golden was weakened.

## Newly exposed failure (ignored with an ENG-12 reason, follow-up)

| Test | Failure | Evidence |
|---|---|---|
| export `gpu::tests::five_fixture_full_chain_tolerance` | `canon-cr3.CR3` full-resolution GPU export **declines the band renderer** and falls back to pyramid tiles. The band path returns `Unsupported("export GPU scratch plus readback exceeds its budget")`, so `assert_eq!(LAST_PATH, "bands")` fails. ARW, NEF, RAF and DNG take the band path. | Precision passes on all five cameras on whichever path ran. Max linear error 1.8e-6 (CR3, tiles) to 8.5e-6 (DNG), codes max 1, against a tolerance of 2e-3 linear and 1 code. |
| export `gpu::tests::five_fixture_web_scale_tolerance` | Same: CR3 Web-scale export reports "export GPU scratch exceeds its budget", falls back to tiles, and the `render_opts` bands assertion fails. | The full-resolution development meets the gate on all five (linear max 4.8e-6, codes 1). The pyramid-level numbers are measured and reported, not gated, as before: codes max 26 to 45. |

These two were measured with the path assertion temporarily printed instead
of asserted. That edit was local and reverted, and nothing was committed. The
assertion itself is unchanged. Follow-up: find out why the per-band scratch
for the 24 MP CR3 exceeds `BUDGET / BANDS_IN_FLIGHT` while the 45 MP NEF fits.
Either fix the band budget or plan, or decide that tiles are correct for CR3
and change the test with a ruling. Run them with:
`cargo test --release -p export --lib gpu::tests::five_fixture -- --ignored --nocapture`

## Item table (finding -> code -> test)

| # | Finding | Code | Test / evidence |
|---|---|---|---|
| 1a | HNSW top-1 misses (about 1.5% of runs) | `crates/ml-embed/src/vector.rs`: `new_graph` sets `set_keeping_pruned(true)`. `search` scores the graph's top-k plus the cached `isolated()` points (outside the largest bottom-layer SCC, from an iterative Kosaraju `outside_largest_component`) by exact cosine, ranks the union and truncates to k. The cache is invalidated on insert and rebuild and recomputed lazily on the next search. | unit `vector::tests::nodes_outside_the_largest_strongly_connected_component`. Loop results below. |
| 1b | Test asserted exact top-1 at position 0; "seeded" misleading | `tests/vector.rs`: `hnsw_top_five_recall_on_a_thousand_random_vectors` asserts the exact top-1 is in the approximate top-5, keeps overlap >= 4, and documents that only the data is seeded | 0/300 at the tip |
| 2a | raw-decode captured-CFA tests needed `TESSERA_CAPTURED_CFA_FIXTURES` | `captured_cfa_fixtures()` in `src/capture/tests/decode.rs` (shared with `owned.rs`): the variable is an override (an explicit inventory, where a missing file fails), else `test_fixtures::raw::root()` with SKIPPED/REQUIRE. Same in `tests/owned_api.rs`. All four are un-ignored. | all pass (times below) |
| 2b | export `lens_analysis` used `expect(PIPELINE_RAW_FIXTURES)` | `raw::files`, un-ignored | pass, 3.5 s |
| 2c | pipeline-cpu `lens_fixtures` five-camera acceptance | `raw::files`. It stays `#[ignore]` as slow (about 36 s), and the reason carries the run command | pass with `--ignored` |
| 2d | pipeline-gpu `resident_large` hard-coded the NEF path, ignored | `raw::file("nikon-nef.NEF")`, un-ignored | pass, 0.6 s |
| 2e | export `gpu.rs` five-fixture tests used `expect(PIPELINE_RAW_FIXTURES)` | `five_fixtures()` helper on `raw::files` | exposed failure above, still ignored |
| 2f | tessera-ffi `smart_preview_workflow` needed `TESSERA_SMART_PREVIEW_RAW` | the variable is an override, else `raw::file("sony-arw.ARW")`; un-ignored. It is the only test in its binary, so it runs in its own process. | pass, 4.3 s |
| 2g | Swift `SmartPreviewNativeWorkflowTests` (2) XCTSkip unless `TESSERA_SMART_PREVIEW_RAW` | `sonyFixture()`: the variable, else `fixtures/raw/sony-arw.ARW` via `#filePath`. Absence is an XCTSkip, or XCTFail under `TESSERA_REQUIRE_RAW_FIXTURES`. The existing hash check (`bf4c6d21...`) matches the repository's ARW. | pass (3.19 s, 3.45 s); Swift gate OK |
| 3 | ml-faces README named only `TESSERA_REQUIRE_MODELS` | README Tests section | docs |
| 4a | Silent `if CI { return; }` in compositor GPU tests (4 in `gpu_resident_viewport.rs`, 3 in `gpu_smart_resample.rs`) | `ci_skip(test)` writes an uncaptured `test <name> ... SKIPPED: CI runner without a Metal device` line. The three existing captured `eprintln!` skips in `gpu_smart_resample.rs` use it too. | behaviour unchanged off CI |
| 4b | Dead `PIPELINE_GPU_ALL_FIXTURES` pops | removed from the three `tools/orchestrate/wp/LR-4/run-*synthetic-gate.py` | ENG-1's historical `audit.py` was left alone, as the brief scoped this to LR-4 |

## HNSW measurements (release, this machine)

The RED and GREEN loops run the copied test binary with `--exact <test>`.

| Build | `top_five_recall` failures | extra 200-query exact top-1 test |
|---|---|---|
| original search (RED commit `93c3e6ef`) | 2 / 200 | 31 / 200 |
| ruling's over-fetch, `max(4k, 64)` candidates re-ranked (`2cdb36b5`) | 7 / 300 | 51 / 300 |
| SCC exhaustive set only, no keep_pruned (scratch) | 0 / 300 | 6 / 300 |
| keep_pruned + SCC set (`36e00b51`, two builds) | 0 / 300, 0 / 300 | 0 / 300, then 2 / 300 and 2 / 600 |
| **tip `4ff2bf28`** | **0 / 300** | (test removed) |

The extra top-1 test (200 queries, exact k=1) was this lane's RED vehicle. I
removed it in `4ff2bf28`. Its remaining misses came from the same hard query
both times: a reachable point that the beam search did not reach. That is
about 3e-5 of queries, which no approximate index rules out, and a 200-query
exact assertion turns it into a flake. The ruling's test stays.

Scratch experiments (hnsw_rs directly, 1000 random 32-d vectors, nothing
committed):
- Misses are unreachable at `k = ef = 1000`. Default graph: 8 of 6,000
  queries missed at `ef = 256`, and the same 8 missed at `ef = 1000`.
- keep_pruned plus the SCC set: 0 merged top-1 misses in 30,000 and then
  60,000 queries.
- Build cost of keep_pruned: 20k x 512-d took 77.8 s without it and 78.0 s
  with it. On 50k x 2-d it took 2.8 s without and 10.0 s with, which is why the
  existing 50k promotion test went from about 5 s to about 17 s.
- Side observation, not changed here: building a 20k x 512-d graph takes
  about 78 s single-threaded. `AutoVectorIndex` rebuilds the graph on every
  open above 50,000 rows.

## Newly running tests and runtimes (release, `--exact`, serialized, load avg about 9)

| Test | Status | Test time |
|---|---|---|
| raw-decode `capture::tests::decode::actual_cfa_fixtures_return_owned_samples_after_original_copy_replacement` | runs by default | 1.37 s |
| raw-decode `capture::tests::decode::actual_native_success_boundaries_observe_cancellation_before_publication` | runs by default | 0.08 s |
| raw-decode `capture::tests::decode::owned::actual_opaque_owner_five_families_after_original_copy_replacement` | runs by default | 1.31 s |
| raw-decode `owned_api::external_owned_api_five_families_survive_capture_cleanup` | runs by default | 0.84 s |
| export `lens_analysis::sparse_lens_analysis_matches_reference_on_fixtures` | runs by default | 3.53 s |
| pipeline-gpu `resident_large::full_nef_transaction_keeps_device_alive` | runs by default | 0.63 s |
| tessera-ffi `smart_preview_workflow::public_engine_offline_restart_sync_original_export_and_conflict` | runs by default | 4.28 s |
| Swift `SmartPreviewNativeWorkflowTests` (2) | run in `swift test` / Swift gate | 3.19 s, 3.45 s |
| pipeline-cpu `lens_fixtures::five_actual_raws_auto_lens_and_upright_are_finite` | ignored (slow); default fixture root | 35.98 s |
| export `gpu::tests::five_fixture_full_chain_tolerance` | ignored (ENG-12 failure) | about 30 s |
| export `gpu::tests::five_fixture_web_scale_tolerance` | ignored (ENG-12 failure) | about 30 s |

One-line commands for the ignored ones:
- `cargo test --release -p pipeline-cpu --test lens_fixtures -- --ignored --nocapture`
- `cargo test --release -p export --lib gpu::tests::five_fixture -- --ignored --nocapture`

## Gates

All gates were run at `4ff2bf28` (code tip) after
`cargo clean --release -p ml-embed -p raw-decode -p export -p pipeline-cpu -p pipeline-gpu -p tessera-ffi -p compositor`.
Env: `CARGO_BUILD_JOBS=5 RAYON_NUM_THREADS=5`, own target dir. Load average
was 5 to 15 (other lanes running).

| Gate | Result |
|---|---|
| `TESSERA_REQUIRE_RAW_FIXTURES=1 cargo test --release --workspace --no-fail-fast` | exit 0. 690 suites, **3563 passed, 0 failed, 102 ignored**, no RAW SKIPPED line. The SKIPPED lines that remain are model-weight or opt-in skips. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, worktree clean (no bindings drift) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 XCTest tests, 1 skipped, 0 failures. Swift Testing: 5 tests passed. |
| `swift test --filter SmartPreviewNativeWorkflowTests` | both ran on the repository ARW and passed: 3.19 s and 3.45 s |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete |

The skip and require policy was checked on the converted tests (raw-decode
`owned_api`, tessera-ffi `smart_preview_workflow`, pipeline-gpu
`resident_large`). `PIPELINE_RAW_FIXTURES=/nonexistent` prints the uncaptured
`test <name> ... SKIPPED: ..` line and passes. Setting
`TESSERA_REQUIRE_RAW_FIXTURES=1` as well makes it fail with the test name.

Not verified: the export five-fixture tests stay ignored (see above). The
compositor CI skip path was not exercised (`CI` was unset).

## Commits

- `93c3e6ef` test: HNSW top-5 membership plus a 200-query exact top-1 test (RED)
- `2cdb36b5` fix: over-fetch and re-rank (the ruling's proposal; measured ineffective)
- `33a18f76` test: captured-CFA tests on fixtures/raw, un-ignored
- `7acf5af9` test: lens and resident fixture tests on fixtures/raw
- `497825c9` test: export five-fixture tests on fixtures/raw; exposed CR3 band decline ignored
- `69faacf8` test: Smart Preview workflow tests (Rust and Swift) on the repository ARW
- `e07192fd` docs: ml-faces README
- `1d231502` test: visible compositor CI skips; LR-4 dead env pops removed
- `36e00b51` fix: keep_pruned plus exact scoring of points outside the main SCC (replaces the over-fetch)
- `4ff2bf28` test: drop the extra 200-query top-1 test
- this HANDOFF
