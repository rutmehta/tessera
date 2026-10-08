# ENG-13 handoff: export GPU scratch accounting (REV2-ENG-12 NEW SHOULD-FIX 1)

Branch `wp/ENG-13`, on top of `wp/ENG-12` `82f3ce4f`. origin/main had not
moved (`db604752`, ENG-12 not yet merged), so there was no rebase: the
branch is ENG-12 plus the commits below. Worker: Claude Opus 5.5.

## The bug

`resident::Batch::new` moved a worker's recycled buffers into the pool's
free list without counting them. Reusing them was not counted, unused ones
stayed allocated for the whole band, and `recycle()` kept up to the 192 MiB
share idle per worker. `checkpoint` then subtracted drained bytes that
included those uncounted buffers. A band could hold its share of idle
buffers plus its share of fresh allocations.

A second, smaller problem turned up while measuring: every band renderer
cloned from one export base shared one `Counters`, so with two workers in
flight `renderer.stats()` could return the other worker's band. The ENG-12b
trace's "actual" (and its "1.058x" bands) came from those shared counters.

## Item table (finding -> code -> test)

| # | Finding | Code | Test |
|---|---|---|---|
| 1 | Recycled buffers uncounted when `Batch::new` takes them | `Pool` keeps them in a separate `idle` list and counts them in a `Meter` (`recycled + fresh - released`, with peak). The meter is now the budget counter (`allocated_bytes` is gone). | `export_footprint.rs` (both shape-changing tests), five-fixture tests |
| 2 | Reuse uncounted / idle buffers held all transaction | `buffer()` reuses `free`, then `idle`, by exact size. Before a fresh allocation `Batch::charge` drops idle buffers, oldest first, until the allocation fits the export scratch, and refuses only when none are left. Idle buffers are referenced by no command, so dropping one releases its memory. | same |
| 3 | Uploads bypassed the budget | Sensor-row, raw-tile and packed-CFA uploads go through `charge` too (counted, and refused past the scratch for export). | same |
| 4 | `checkpoint` subtracted uncounted bytes | Drains and releases only `free`, which is fully counted now. Its 128 MiB threshold uses `touched()` (live minus idle), which is what the old counter approximated. | `export_retires_uploads_without_intermediate_readback` still passes |
| 5 | `recycle()` could hold up to the share idle with no relation to live bytes | Unchanged cap (`export_scratch` per worker). Between bands live is 0, and the next band counts its idle intake and evicts on demand, so idle + live never exceeds the share. | peak column below: max 192.0 MiB against 192 MiB |
| 6 | Trace "actual" used the blind counter | Readback (`finish_rows_impl`, export `finish`) drops the remaining idle buffers, then checks and publishes the true footprint. `last_resident_allocated_bytes` now equals `last_resident_live_bytes`. The trace also prints `peak`, `recycled_in` and `fresh_buffers`. | five-fixture trace |
| 7 | Concurrent bands overwrote each other's statistics | `ManagedRenderer::band_with` gives every band renderer its own `Counters`. Nothing read aggregate statistics across band renderers. | `band_renderers_keep_their_own_statistics` |
| 8 | Planner refit if "actual" rises | Not needed: over all 331 traced five-fixture bands, the largest actual/planned ratio is **0.989** (none above 1). The 26/96/24 and 36/32 constants stand. | five-fixture trace |

### Instrumentation and tests

- `GpuStats` has three new fields: `last_resident_live_bytes` (held at
  readback, without staging), `last_resident_peak_bytes`, and
  `last_resident_recycled_bytes`.
- `crates/pipeline-gpu/tests/export_footprint.rs` covers one worker's
  bands that change shape (256, 256, 48, 128, 256, 16, 200, 64 rows), plain
  and resized (2048x1024 to 1600x800). The scratch is the largest band's own
  need plus 1/8. Every band must satisfy `max(peak, live + readback) <=
  scratch`, and recycling must actually happen. RED: the second 256-row band
  held 99.9 MiB + 6.0 MiB of readback against 82.6 MiB, and 6 of the 8 bands
  were over. In the resized test the worst band reached 189.6 + 4.7 MiB
  against 121.5 MiB.
- Export: in test builds `render_bands` records a `BandFootprint` per band
  (`BAND_FOOTPRINTS`). `five_fixture_full_chain_tolerance` and
  `five_fixture_web_scale_tolerance` now also require `max(peak, live +
  readback) <= BUDGET / in_flight` for every band. They print one
  `FOOTPRINT` line per export.

## Footprint, five fixtures, before and after (MiB; share 192)

Before = RED commit `46ff7a45`, which adds only the meter. After = `ee3369b1`.
331 bands in total. Before, **251 bands were over the share**; after, none.
Before-values were read through the shared counters (item 7). Each value
is a real band's footprint, but possibly the other worker's band.

| test / camera | bands | before: over share | before: max live+readback | before: old "actual" | after: over share | after: max live+readback | after: max peak |
|---|---|---|---|---|---|---|---|
| full-chain CR3 lens-off | 14 | 12 | 349.5 | 179.1 | 0 | 179.1 | 190.1 |
| full-chain CR3 default | 14 | 12 | 342.2 | 178.2 | 0 | 179.1 | 190.1 |
| full-chain ARW lens-off | 13 | 8 | 325.0 | 175.2 | 0 | 176.3 | 191.5 |
| full-chain ARW default | 13 | 7 | 316.1 | 176.3 | 0 | 176.3 | 191.5 |
| full-chain NEF lens-off | 31 | 26 | 319.3 | 165.5 | 0 | 167.1 | 191.9 |
| full-chain NEF default | 31 | 25 | 319.5 | 165.5 | 0 | 167.1 | 191.9 |
| full-chain RAF lens-off | 13 | 9 | 309.8 | 175.3 | 0 | 176.2 | 189.9 |
| full-chain RAF default | 13 | 9 | 308.2 | 176.2 | 0 | 176.2 | 189.9 |
| full-chain DNG lens-off | 15 | 10 | 278.8 | 175.2 | 0 | 175.2 | 191.8 |
| full-chain DNG default | 15 | 11 | 273.7 | 174.2 | 0 | 175.2 | 191.8 |
| web CR3 full-res | 16 | 11 | 311.7 | 161.3 | 0 | 165.2 | 191.5 |
| web CR3 pyramid-level | 16 | 11 | 346.2 | 165.2 | 0 | 165.2 | 191.5 |
| web ARW full-res | 17 | 8 | 323.4 | 160.9 | 0 | 161.3 | 191.4 |
| web ARW pyramid-level | 6 | 3 | 326.1 | 173.2 | 0 | 176.6 | 192.0 |
| web NEF full-res | 43 | 42 | 323.0 | 134.8 | 0 | 134.8 | 192.0 |
| web NEF pyramid-level | 14 | 8 | 339.0 | 172.2 | 0 | 172.6 | 191.9 |
| web RAF full-res | 17 | 16 | 322.0 | 160.8 | 0 | 160.8 | 192.0 |
| web RAF pyramid-level | 6 | 4 | 305.8 | 173.7 | 0 | 176.3 | 190.9 |
| web DNG full-res | 17 | 16 | 342.7 | 156.4 | 0 | 156.5 | 192.0 |
| web DNG pyramid-level | 7 | 3 | 278.7 | 169.8 | 0 | 169.8 | 191.3 |

- After the fix, live + readback at readback is each band's own working
  set, and it equals the counter the budget checks use.
- Peak is the largest footprint during a band, staging excluded (staging is
  allocated after idle buffers are dropped). It sits near the cap because a
  worker keeps idle buffers up to its share and drops them only on demand.
- With two workers, export scratch is now at most 2 x 192 = 384 MiB =
  `BUDGET`, as intended. Before the fix it reached about 700 MiB.
- Precision is unchanged; the PRECISION and WEBGATE lines are identical.

## Export time, before and after

These are `EXPORT_TRACE` "GPU bands (incl. yields)" times for all 20
five-fixture exports. Before = RED binary (no behaviour change); after =
fix. Runs alternated before, after, 4 times each, and the table shows the
median per export. Load average was 12 to 19 because other lanes were
running.

| export | before ms | after ms | ratio |
|---|---|---|---|
| full CR3 lens-off / default | 70.5 / 63.5 | 62.9 / 58.5 | 0.89 / 0.92 |
| full ARW lens-off / default | 64.0 / 53.5 | 58.5 / 53.0 | 0.91 / 0.99 |
| full NEF lens-off / default | 136.2 / 128.5 | 115.0 / 112.7 | 0.84 / 0.88 |
| full RAF lens-off / default | 57.0 / 65.5 | 53.3 / 62.5 | 0.94 / 0.96 |
| full DNG lens-off / default | 86.9 / 75.6 | 59.8 / 61.1 | 0.69 / 0.81 |
| web CR3 full-res / pyramid | 63.8 / 63.3 | 54.0 / 54.9 | 0.85 / 0.87 |
| web ARW full-res / pyramid | 62.3 / 29.0 | 59.3 / 30.9 | 0.95 / 1.07 |
| web NEF full-res / pyramid | 149.6 / 60.2 | 163.9 / 58.6 | 1.10 / 0.97 |
| web RAF full-res / pyramid | 64.0 / 29.3 | 64.5 / 30.6 | 1.01 / 1.05 |
| web DNG full-res / pyramid | 74.2 / 31.8 | 70.8 / 30.5 | 0.95 / 0.96 |
| **total** | **1428.8** | **1315.2** | **0.92** |

There is no slowdown. The individual ratios (0.69 to 1.10) are within the
noise at this load.

## Viewport and other non-export paths (checked, reported)

- **Accounting is fixed for every path.** The `Pool` changes apply to
  viewport transactions too. Their `last_resident_allocated_bytes` now
  includes the recycled buffers they hold, and the 768 MiB bound in
  `tests/resident.rs` still passes.
- **No budget is enforced on non-export transactions, by design.** It was
  never enforced: the scratch check is `export_float` only, so nothing
  there was miscounted against a budget.
- **Report (not changed):** non-export transactions recycle up to
  `RECYCLE_BYTES` = 1 GiB of idle buffers per `GpuStageOp`, besides the
  resident memo cache (512 MiB default). Neither is tied to the 128 MiB
  `VIEWPORT_RESERVE` that `export::gpu` assumes for the viewport. The
  `GPU_SCRATCH` envelope is therefore an export-side split, not a
  device-wide bound. Enforcing it on the viewport is a design decision (it
  trades away interactive reuse), so it is outside this lane.
- **Report (not changed):** the effects constants map (vignette or grain)
  is allocated outside the pool. It is `resident::Batch::effects_map`: 8 B
  per pixel of the whole image at the render level, kept on the shared
  `GpuStageOp` and therefore shared by all of an export's band renderers. A
  full-resolution export of a 45 MP image with vignette or grain would
  allocate about 350 MiB that no budget counts. Neither five-fixture test
  sets vignette or grain. `band_renderer_matches_tile_renderer` does, on
  small synthetic images. This finding comes from reading the code; I did
  not measure it on a fixture. A follow-up could skip the map for export
  (bands touch every pixel once, so the map saves no work there) or count
  it against the scratch.
- The tile path (`render_tiles`) uses `export_band` without recycling, so
  it never had idle buffers. Its uploads are now charged as well.

## Gates

All gates ran at `ee3369b1` (code tip) after `cargo clean --release -p
pipeline-gpu -p export`. Load average was 13 to 18, with other lanes
running.

| Gate | Result |
|---|---|
| `TESSERA_REQUIRE_RAW_FIXTURES=1 cargo test --release --workspace --no-fail-fast` | exit 0. 691 suites, **3571 passed, 0 failed, 100 ignored** (ENG-12b: 3568 passed in 690 suites; the difference is the 3 new tests in the new `export_footprint` suite). No RAW SKIPPED line; the only SKIPPED lines are absent model weights, as before. |
| `cargo clippy --release --workspace --all-targets -- -D warnings` | exit 0 |
| `cargo fmt --all -- --check` | exit 0 |
| `cd apps/mac && ./build-ffi.sh` | exit 0, no bindings drift (only this HANDOFF was untracked) |
| `tools/orchestrate/swift-gate.sh` | **SWIFT GATE OK**: 996 tests, 1 skipped, 0 failures; Swift Testing: 5 passed |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | Build complete |

Also run separately: `cargo test --release -p pipeline-gpu -p export`
(85 suites, 323 passed, 0 failed), and the two five-fixture tests with
`TESSERA_EXPORT_TRACE=1`, which produced the tables above.

## Commits (on top of 82f3ce4f)

- `46ff7a45` test: measure export bands' true GPU footprint (RED)
- `ee3369b1` fix: count every byte an export band transaction holds
- this HANDOFF
