# B5-29c handoff: Lightroom import follow-ups (report, retention, degrade, streaming)

Branch `wp/B5-29c` from `origin/main` `10f0a3f7`. Changes: `crates/import-lrcat` (lib, lua_develop, tests),
`crates/tessera-ffi/src/lrcat.rs` (report parsing only), `apps/tessera-cli/src/import.rs` (streamed `--apply`),
`apps/tessera-cli/Cargo.toml` (+`libc`, already in the lock). No FFI/Swift API change.

## Items (Machine A's order)
1. **Upright counts.** `UprightFourSegmentsCount` and `UprightTransformCount` are in `KEY_MAP` as ordinary crs
   properties. They now go through the XMP path as "unsupported property" (retained per property) instead of
   "unknown Lua key", which also forced whole-literal retention on every edited row.
2. **Bad develop rows degrade.** A develop row that fails to decode, or has text but NULL `processVersion`, imports
   that image as unedited. It gets a report entry (`develop settings not imported (<reason>); imported as
   unedited, source preserved`) and `recipe.unknown["lrcat_develop_source"] = {text, processVersion}`. The rest of
   the catalog still imports. Identity/path/keyword-structure errors are still strict.
3. **Report grouped per message.** Per-image entries are `image <id>: <msg>` for one image and
   `<n> images (first: image <id>): <msg>` otherwise (n counts images, first-seen order). The FFI summary parses
   both forms and keeps per-image counts (`issue.count` = n, example = first image).
4. **Retention.** Lua rows no longer keep the generated `sidecar_xmp` packet or the whole literal.
   `recipe.unknown["lrcat_develop_lua"]` is now an object `Lua key -> value source text`, present only for
   unknown or unrenderable keys (positional root entries keep the literal under `(positional entries)`).
   Known-only rows retain nothing extra. XMP rows are unchanged (they keep their own packet as `sidecar_xmp`).
   Codec warnings on Lua rows still say "retained in original XMP"; the data is kept per property
   (`crs:<Key>` in `recipe.unknown`, same as before), not in a packet.
5. **Indexing, bounded cells, streaming.** See "Performance". Cells over `MAX_CELL_BYTES` (8 MiB) are measured on
   SQLite's borrowed cell and not copied. The column reads NULL and the report names table/row/column.
6. **ExtendedToneCurvePV2012 (+Red/Green/Blue/Name2012): not mapped. This is a named limitation.** The recipe has
   no slot for it: `ToneCurves` holds rgb/red/green/blue/luminance over 0..=1, and `CrsKey` has no
   `ExtendedToneCurve*`. Per Machine A's update, codec translation belongs to Codex on A, so nothing is mapped
   here. Each image that has any of these keys gets one warning, `ExtendedToneCurvePV2012 (+Red/Green/Blue):
   extended-range (HDR) tone curves are not supported by Tessera; not applied, source preserved`. That gives one
   grouped report entry, and the source of those keys is retained. On this catalog, 718 of the 737 rows have an
   identity master curve (`0,0,255,255`). The other 19 have a non-identity extended curve, which is not applied.
7. **Why 6ccc6eaa changed a RED expectation (mask-item XMP shape).** The B5-29b RED test
   `structured_values_map_like_their_xmp_form` wrote the gradient mask item in Adobe's shorthand,
   `<rdf:li crs:FullX=… crs:What="Mask/Gradient" …/>`, with properties as attributes on `rdf:li`. The Lua
   renderer writes every keyed-table list item in the generator form, `<rdf:li><rdf:Description …/></rdf:li>`.
   The two are equivalent RDF, and the codec decodes both to the same mask. Re-running that test at 10f0a3f7 with
   the RED string shows settings equal and warnings equal. The only difference is
   `recipe.unknown["crs:MaskGroupBasedCorrections"]`: `xmp::parse` retains each property's raw source substring
   (`&text[node.range()]`), and `comparable()` strips only `sidecar_xmp`. 6ccc6eaa therefore wrote the expected
   XMP in the generator's shape, so parity compares translation and not source spelling. The outer correction
   item already used the generator form in the RED test. Only the inner mask item was shorthand.

## Recipe stability (golden)
`tests/common` generates an LrC 15.5-shaped catalog of any size: Lua + XMP + empty develop rows, BLOB history,
faces, GPS, keywords, collections, stacks and virtual copies. `tests/golden.rs` pins the 2,000-image import to
the digest that **B5-29b (10f0a3f7) produces with the same generator**. The test was run there and gives the
same value here, before and after streaming. XMP and empty rows are hashed byte for byte. On Lua rows, the two
item-4 keys (`sidecar_xmp`, `lrcat_develop_lua`) are removed before hashing. `recipe.image_id` is cleared because
it derives from the catalog path. A second test checks that `import_each` gives the same images and report as
`import()`, and that `PlanJson` writes exactly `serde_json::to_vec_pretty(&plan)` (synthetic, fixture and empty
catalogs). On the real copy, the full `--apply` bundle (21,658 files) is **byte-identical** between the
index-only build (d66748c3) and the streaming build (sha256 of every file compared).

## Performance (real catalog COPY, read-only; counts only)
Copy sha256 prefix `eb60e744dbec2547`, unchanged after every run. The original under ~/Pictures was not opened.
Measured with `/usr/bin/time -l` (peak = maximum resident set size). The machine was shared with other agents'
builds: load average ran 50-370 during these runs, so wall times are pessimistic.

| build | inspect wall | inspect peak RSS | apply wall | apply peak RSS | bundle |
|---|---|---|---|---|---|
| main 10f0a3f7 (B5-29b) | 137.8 s | 5.09 GB | 353.9 s | 5.10 GB | 3.12 GB |
| items 1-4 + indexing (d66748c3) | 42.9 s | 4.82 GB | 181.5 s | 6.02 GB | 2.77 GB |
| **final (streaming)** | **14.2 s** | **0.40 GB** | **20.3 s** | **0.50 GB** | 2.78 GB |

Targets: inspect < 20 s, apply < 60 s, peak < 1 GB: all met. (A lighter-load run of the streaming build gave
10.8 s and 0.43 GB for inspect, 18.1 s and 0.63 GB for apply.) Before streaming, these were the causes:
- Quadratic per-image scans (fixed by indexing: 138 s -> 43 s).
- `Adobe_libraryImageDevelopHistoryStep.text` is a **BLOB** in LrC 15.5 (38 MB). Each byte loaded as a
  `serde_json::Value` (~32 B), so the history table alone held ~1.4 GB, then was cloned per image twice
  (`image.history` and `recipe.unknown["lrcat_history"]`).
- `--apply` built the 1.8 GB pretty `import-plan.json` in memory.
- `--apply` did a per-file `F_FULLFSYNC` (~5 ms each, ~100 s for 21k recipes, measured).

How the streaming build works:
- `import_each(path, begin, visit)` loads only the small tables. Images, develop settings, history, snapshots,
  faces and EXIF are read side by side `ORDER BY image, rowid` and merged per image.
- Develop settings are translated in batches of 256 on all cores. Report and visit order are unchanged.
- `import()` and `inspect()` are built on it. `inspect` counts and drops each image.
- `--apply` streams: one thread writes `import-plan.json` (`PlanJson`), and 8 threads write recipe files. Each
  file gets a plain `fsync`, then one `F_FULLFSYNC` runs before the staging dir is renamed into place. Recipe
  bytes and checks are the same as `Sidecar::write_recipe`.
- `tests/scale.rs` uses a counting global allocator. On the synthetic catalog, Rust peak heap is about 30 MB at
  2k images and 50 MB at 20k. The test asserts the peak stays flat (< 1.5x + 16 MB) and < 256 MB. Time is
  bounded loosely; the test runs in release only.

Bundle size barely moved (3.12 -> 2.78 GB). The bulk is **not** `sidecar_xmp` or the literal (item 4 saved
~0.3 GB). It is the BLOB history serialized as JSON number arrays, pretty-printed one byte per line, in both
`import-plan.json` and each recipe's `lrcat_history`. Changing that changes the output format, so it is left
for A (see Follow-ups).

## Report on the real copy
570,311 entries -> **105**: 77 grouped, 15 single-image, and 13 catalog entries (12 smart collections + 1
slideshow skipped, as before). That is 568,085 per-image occurrences across 21,615 edited rows.
- Unknown Lua keys: EnableDistractionRemoval 9,553; FilterList 8,029; AILook 615; Preset 146;
  CropConstrainAspectRatio 47; RemoveAreas / CustomTint / CustomTemperature 42; plus the rare ones B5-29b listed.
  UprightFourSegmentsCount / UprightTransformCount are no longer unknown (21,615 each now "unsupported
  property").
- Extended tone curve: 1 entry, 737 images.
- Codec-retained structures are unchanged from B5-29b: masks (Mask/Image 616, radial Flipped 31, translated with
  fidelity note 44), LensBlur 417, RetouchAreas 330, RetouchInfo 327, PointColors 277.
- Develop rows: 0 degraded (no bad rows in this catalog), 41 empty rows imported as unedited.

## Follow-ups (for A)
- FFI (`open_lrcat` / `inspect_lrcat`) still calls `import()`, which holds every image with its BLOB history as
  JSON values. The app path needs the same streaming (or a compact history representation); the CLI targets
  above do not cover it.
- History BLOBs in the bundle: storing them as a base64 or UTF-8 string instead of a number array would cut
  most of the 2.8 GB. This is a plan/recipe format change, so it needs A's call.
- Codex on A: ExtendedToneCurve*, Mask/Image, LensBlur focal range, RetouchAreas/RetouchInfo, PointColors.

## Commits
`41f64170` RED follow-ups · `340902cc` Lua retention/Upright/extended curve · `d66748c3` indexing, cell bound,
degrade, grouping · `6e7c448a` RED extended curve always reported · `3c380cfd` fix · `d4098e00` synthetic
generator + golden · `f84d56a1` streaming · `ffe28ee9` generator lint · this HANDOFF.

## Gates
`cargo test --release -p import-lrcat` (all targets incl. golden + scale), `-p tessera-ffi --test lrcat`,
`-p tessera-ffi --lib lrcat`, and `-p tessera-cli --test import_models`: pass.
`cargo clippy --release --all-targets -p import-lrcat -- -D warnings` and `-p tessera-cli -- -D warnings`: clean.
`cargo fmt --all --check`: clean. `apps/mac/build-ffi.sh`: ok. `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK
(861 XCTest tests, 3 skipped, 0 failures; 5 swift-testing tests).
