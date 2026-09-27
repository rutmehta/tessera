# RES-01 follow-up gates

The two added integration cases preserve exact cloned smart-object child/filter identity while giving their parent layers distinct IDs. The initial fixture had both parent layer IDs at the `LayerId(0)` default, causing an existing smart-tile cache collision. That run compiled and passed seven tests; the mask-pixel case failed for the fixture reason. The initial test source is in [`../extra-cases/`](../extra-cases/) with SHA-256 `401bb8638a6cc5db1154dcc95a5c782f6167e2753e5ca1aff29751f4b77d0dd2`. The corrected source is in [`corrected-source/`](corrected-source/) with SHA-256 `5473942572deaaa165a950b9384078af5b2074a36ca652d6ad6104c609d40a24`. Only the two parent `LayerId`s and import changed; the cloned `SmartObject` still shares its exact child key, revision, filters, and native context, while masks and placement differ.

| Gate | Exit | Outcome | Log |
| --- | ---: | --- | --- |
| Initial extra-case file | 101 | 7/8 passed; mask fixture failed from duplicate parent layer IDs | [`initial-extra-7-of-8-fixture-failure.log`](initial-extra-7-of-8-fixture-failure.log) |
| Corrected extra-case file | 0 | 8/8 passed, including same-frame masked/unmasked pixels and concurrent independent passes; child deadlock watchdog passed | [`corrected-extra-8-of-8-green.log`](corrected-extra-8-of-8-green.log) |
| Compositor library unit tests | 0 | 60 passed, 1 ignored benchmark. Several `resident::output` tests **requested an A-host Metal device**; this was not CPU-only. | [`compositor-lib-60-plus-1-ignored-metal.log`](compositor-lib-60-plus-1-ignored-metal.log) |
| Eight selected integration binaries | 0 | 48 passed across caching, layer styles, live render/style damage, smart filters, structure, transform content, and transforms; includes a GPU-capable resident live-geometry case | [`cpu-integration-48-green.log`](cpu-integration-48-green.log) |
| Exact-file `rustfmt --check` and `git diff --check` | 0 | Both passed before strict Clippy | Recorded command in [`next-gates-plan.md`](next-gates-plan.md); no separate output log (both emitted none) |
| Strict `cargo clippy -p compositor --release --lib --test smart_filter_deadlock -- -D warnings` | 0 | Passed | [`strict-clippy-green.log`](strict-clippy-green.log) |

All Cargo gates used cached release target `/Volumes/betterSSD/tessera-cache/target/main`, `CARGO_BUILD_JOBS=2`, `RAYON_NUM_THREADS=2`, and a 600-second process-group timeout. The deadlock watchdog constructs local Rayon pools of 1, 2, and 4 workers. No full compositor matrix, GPU integration binary, benchmark, large stress fixture, or Machine B run was launched. The library gate's small Metal-backed unit tests are reported above rather than hidden under a CPU-only label.

These outcomes validate the scoped pass reuse and retained-result admission. They do not measure process working memory, GPU memory, or the cause of Machine B's swap. Reapplying a smart-filter mask still edits a full child raster on each pass hit; this is separate, uncounted work for RES-03 and remains open.

## Scope correction from source review

The historical `cpu-integration-48-green.log` filename is retained. That run included `live_render::resident_live_geometry_and_masks_match_cpu`, which requests a GPU device and can early-return if none is available. Therefore the 48-test run was not strictly CPU-only, and its ordinary captured pass output alone does not separately prove that case executed its GPU path. The library tests above require Metal and passed. No benchmark or B workload ran. Counts and raw logs are unchanged; future CPU-only target lists exclude this GPU-capable case.
