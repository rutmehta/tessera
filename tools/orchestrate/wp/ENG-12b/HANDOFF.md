# ENG-12b handoff: REV-ENG-12 changes (keep_pruned, isolated-set cap, export band planner)

Branch `wp/ENG-12`, on top of ENG-12 `1edec5d1`. origin/main had not moved
(merge base `db604752`), so there was no rebase. Worker: Claude Opus 5.5.
Input: `REV-ENG-12` BLOCKER 1, SF1 to SF3, and the positional-assertion nit.

## Item table (finding -> code -> test)

| # | Finding | Code | Test / evidence |
|---|---|---|---|
| B1 | `set_keeping_pruned(true)` breaks clustered data into islands | Removed. `new_graph` is plain `Hnsw::new`, with a comment saying why. Exact scoring of points outside the largest SCC is kept. | New `hnsw_recall_on_clustered_vectors`: 200 clusters x 20, 64-d, 5% noise, 100 queries; the exact top-1 must be in the top-5 for >= 96. RED with keep_pruned: 85 to 99 over 60 runs (below 96 in about 88%). Without it: 100 in 598 of 600 runs, 99 once, 98 once. |
| SF2 | The SCC fallback had no ceiling | `Isolated::{Points, TooMany}`. When more than `max_isolated(n) = n / 20` (5%) points are outside the main component, the index prints one stderr line and `search` uses the exact `SqliteVectorIndex::search` until the next write. Why 5%: the extra scoring then costs at most 5% of a brute-force scan and the cached copies at most 5% of the vectors; past that the graph is not narrowing the search, and an open exact search beats a silent near-brute-force one. There is no absolute cap: exact search is always correct, so a small index just goes exact sooner. I printed to stderr instead of using `log` because ml-embed has no `log` dependency and adding one would change Cargo.lock. | unit `too_many_isolated_points_switch_search_to_exact`: two groups of 500 exact duplicates put 594 to 762 of 1000 points outside the main component over 60 builds (the cap is 50); the state is `TooMany` and `search == store.search`. Unit `at_most_five_percent_is_scored_beside_the_graph`. |
| SF3 | Recomputing after every write costs O(n x M) | Doc comment on `HnswVectorIndex`: the cost assumes batch writes followed by searches (`SemanticIndex` reopens after a background job); a caller alternating inserts and searches pays it on every search | docs |
| Nit | Positional top-1 assertion | `assert_eq!(actual[0].0, expected[0].0)` restored in `hnsw_top_five_recall_on_a_thousand_random_vectors`; the comment says why it is equivalent | 0/300 |
| Nit | Residual gap | Documented on the type: a walk that enters a closed island misses main-component neighbours | docs |
| SF1 | The export band planner charged sensor stages at the developed width | See "Band planner" below | both five-fixture tests un-ignored, unchanged, and passing |

## HNSW loops (release, copied test binaries, `--exact`)

| Test | Runs | Failures |
|---|---|---|
| `hnsw_top_five_recall_on_a_thousand_random_vectors` | 300 | **0** |
| `hnsw_recall_on_clustered_vectors` (with threshold 98) | 300 | 0 (minimum 98) |
| `hnsw_recall_on_clustered_vectors` (distribution) | 300 more | 100 in 299, 99 once |
| `vector::tests::too_many_isolated_points_switch_search_to_exact` | 300 | **0** |

Over those 600 clustered runs the minimum count was 98, so `8ec59c35` sets
the threshold to 96. That leaves two queries of margin, still fails about 88%
of runs with keep_pruned on, and is within the review's suggested 95. The
50k promotion test is back to about 5 s (`tests/vector.rs` takes 5.3 s in
total).

## Band planner (export `render_bands`)

- **Shared geometry.** image-core now exposes `BandGeometry` and `BandRows`
  through `Renderer::export_band_geometry`, passed through
  `ManagedRenderer::export_band_geometry`. They give the rows each stage of a
  band reads: the developed rows with the Detail halo, and the sensor rows
  with the linearize, demosaic and lateral-CA halos. `develop_band` now gets
  its rows from the same `BandGeometry::stages`, so the planner and the
  renderer cannot drift apart.
- **Cost model.** Each band is charged:
  - sensor rows x CFA width x **26 B**;
  - developed rows x frame width x **96 B**;
  - output px x **24 B** of readback (interleaved buffer plus staging);
  - with a resize, 36 B per output px plus 32 B per developed px instead;
  - the lens map's existing 48 B per mapped px.

  A band must fit with **5% margin** below its share
  (`BUDGET / BANDS_IN_FLIGHT` = 192 MiB).
- **Where S and D come from.** `TESSERA_EXPORT_TRACE` gives the worst band
  per camera for the full chain at level 0. Solving across cameras gives
  S = 25.1 and D = 95.6 B/px. The CR3 separates the two: sensor 6288 wide,
  developed 4000. The other cameras then check out within 2%. The resize
  extra measured 25 to 31 B per developed px at full resolution.
- **Decline at plan time.** If even a 16-row band does not fit, the plan
  returns "pyramid tiles" before any band renders. The readback check stays as
  a safety net.
- **Trace.** Each band now prints actual scratch (allocated plus staging) next
  to the planned sensor, developed, mapped and readback parts.

Both tests pass unchanged on all five cameras, and **CR3 now takes bands**.
Per camera, the worst band (actual includes buffers a worker keeps from its
previous band):

| Test / camera | bands | max actual | max planned | share |
|---|---|---|---|---|
| full chain CR3 | 14 | 179.1 MiB | 181.7 MiB | 192 MiB |
| full chain ARW | 13 | 176.3 | 180.3 | 192 |
| full chain NEF | 31 | 167.1 | 171.9 | 192 |
| full chain RAF | 13 | 176.2 | 180.3 | 192 |
| full chain DNG | 15 | 175.2 | 179.4 | 192 |
| Web full-res CR3 / ARW / NEF / RAF / DNG | 16 / 17 / 43 / 17 / 17 | 161.3 / 160.9 / 134.8 / 160.5 / 156.5 | 182.2 / 180.9 / 156.3 / 180.9 / 177.9 | 192 |
| Web pyramid-level CR3 / ARW / NEF / RAF / DNG | 16 / 6 / 14 / 6 / 7 | 161.3 / 173.2 / 172.6 / 173.7 / 169.8 | 182.2 / 182.3 / 180.0 / 182.0 / 175.9 | 192 |

- **Headroom.** No band exceeds its share; the largest actual is 179.1 MiB.
- **Actual against planned.** For every test the largest actual stays below
  the largest planned band.
- **Bands where actual passed planned.** A few individual bands show actual
  above their own plan: up to 1.058 x (NEF pyramid-level) and about 1.015 x
  (ARW and RAF Web full-res). These are smaller bands measured while the
  worker still held buffers sized for its earlier, larger band. The full-chain
  ratios are at most 0.994.
- **Precision is unchanged.** Full chain: linear max 1.8e-6 to 8.5e-6, codes
  max 1.
- **Cost of the conservative resize term.** The 32 B resize term
  over-charges the 45 MP NEF: it went from 29 to 43 bands at Web full-res.

## Gates

All gates ran at `3fa1945e` (code tip), after
`cargo clean --release -p ml-embed -p image-core -p pipeline-gpu -p export`.
Load average was 7 to 17 (other lanes running).

| Gate | Result |
|---|---|
| `TESSERA_REQUIRE_RAW_FIXTURES=1 cargo test --release --workspace --no-fail-fast` | exit 0. 690 suites, **3568 passed, 0 failed, 100 ignored** (ENG-12 had 102 ignored; the two export five-fixture tests now run and pass). No RAW SKIPPED line. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift (only the HANDOFF files were untracked or modified) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 1 skipped, 0 failures; Swift Testing: 5 passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete |

An earlier gate run, before `3fa1945e`, caught a regression that I fixed and
then re-gated. Three synthetic export tests (`lens_and_geometry_recipes_render_on_gpu`,
`band_renderer_matches_tile_renderer`, `gpu_matches_cpu_and_bands_match_whole`)
pass a tiny `budget` to force many bands. My first plan-time decline compared
the smallest band with that sizing share, so these exports fell back to
tiles. Now bands are sized by the share (16 rows minimum), and the plan
declines only when the smallest band exceeds `BUDGET / in_flight`, the scratch
each band renderer actually gets. All 41 export lib tests pass. Across their
1162 traced bands the largest actual scratch was 179.1 MiB, against 192 MiB.

## Commits (on top of 1edec5d1)

- `7efb6c2c` test: clustered recall test (RED with keep_pruned); positional top-1 restored
- `bc035835` test: un-ignore the export five-fixture tests (RED for the planner)
- `e03ec706` fix: drop keep_pruned; cap the isolated set at 5% and fall back to exact search
- `140b2fa8` fix: band planner charges sensor stages per sensor pixel; plan-time decline; trace actual vs planned
- `8ec59c35` test: clustered threshold 96
- `3fa1945e` fix: the plan-time decline compares with the band renderer's scratch, not the sizing budget
- this HANDOFF
