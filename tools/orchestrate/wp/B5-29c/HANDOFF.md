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
3. **Actionable report entries remain individual.** Every image imported as unedited, including empty or
   absent develop rows, gets `image <id>: <msg>`. Only harmless unknown-Lua-key notes and the explicitly
   requested named extended-curve limitation use `<n> images (first: image <id>): <msg>`.
   The FFI summary keeps each unedited image as a distinct issue with its id in the reason and count 1;
   grouped unknown-key notes still preserve occurrence counts and the first example.
4. **Retention.** Lua rows no longer keep the generated `sidecar_xmp` packet or the whole literal.
   `recipe.unknown["lrcat_develop_lua"]` is now an object `Lua key -> value source text`, present only for
   unknown or unrenderable keys (positional root entries keep the literal under `(positional entries)`).
   Known-only rows retain nothing extra. XMP rows are unchanged (they keep their own packet as `sidecar_xmp`).
   Consequence: `sidecar::XmpPacket::from_imported_recipe` requires `sidecar_xmp`, so it can no longer export
   Lua-row recipes and returns `no source XMP`. There are no production callers today; its existing test
   covers an XMP-derived recipe. Lua-row export needs a future synthesis path.
   Codec warnings on Lua rows still say "retained in original XMP"; the data is kept per property
   (`crs:<Key>` in `recipe.unknown`, same as before), not in a packet.
5. **Indexing, bounded cells, streaming.** See "Performance". Cells over `MAX_CELL_BYTES` (8 MiB) are measured on
   SQLite's borrowed cell. Ordinary oversized cells read NULL with a table/SQLite-rowid/column note.
   Oversized develop text instead enters the normal unedited degrade path, names the image id and SQLite
   rowid, and retains at most the first 64 KiB (ending at a valid UTF-8 boundary) in
   `lrcat_develop_source.text`, alongside `processVersion` and `truncated: true`. The full cell is never copied.
6. **ExtendedToneCurvePV2012 (+Red/Green/Blue/Name2012): not mapped. This is a named limitation.** The recipe has
   no slot for it: `ToneCurves` holds rgb/red/green/blue/luminance over 0..=1, and `CrsKey` has no
   `ExtendedToneCurve*`. Per Machine A's update, codec translation belongs to Codex on A, so nothing is mapped
   here. Each image with a non-identity curve gets one warning, `ExtendedToneCurvePV2012 (+Red/Green/Blue):
   extended-range (HDR) tone curves are not supported by Tessera; not applied, source preserved`. That gives one
   grouped report entry, and the source of those keys is retained. Identity master and R/G/B curves do not
   warn; the name alone is not an edit. An edited channel still warns with an identity master. On this catalog, 718 of 737 rows have the exact two-point identity master curve (`0,0,255,255`),
   and two more have identity master polylines with intermediate points. Three identity-master rows have
   an edited color channel. All four curves are identity on 717 rows; the named limitation covers the other
   20 rows (17 edited masters + 3 channel-only edits). This supersedes the earlier master-only count of 19.
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

## Original performance baseline (real catalog COPY, read-only; counts only)
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
  recipe file gets plain `libc::fsync` in `write_staged_recipe`; this does not explicitly flush the drive's
  cache. After the plan is finished, `File::sync_all()` invokes `fcntl(F_FULLFSYNC)` on Apple targets,
  requesting the full drive-cache flush before publication. `libc` was added for plain `fsync`, not full flush.
  Verified against [Rust's Apple fsync implementation](https://doc.rust-lang.org/src/std/sys/fs/unix.rs.html).
  Recipe
  bytes and checks are the same as `Sidecar::write_recipe`.
- `tests/scale.rs` uses a counting global allocator. On the synthetic catalog, Rust peak heap is about 30 MB at
  2k images and 50 MB at 20k. The test asserts the peak stays flat (< 1.5x + 16 MB) and < 256 MB. Time is
  bounded loosely; the test runs in release only.

Bundle size barely moved (3.12 -> 2.78 GB). The bulk is **not** `sidecar_xmp` or the literal (item 4 saved
~0.3 GB). It is the BLOB history serialized as JSON number arrays, pretty-printed one byte per line, in both
`import-plan.json` and each recipe's `lrcat_history`. Changing that changes the output format, so it is left
for A (see Follow-ups).

## Original report baseline on the real copy
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

## Original commits
`41f64170` RED follow-ups · `340902cc` Lua retention/Upright/extended curve · `d66748c3` indexing, cell bound,
degrade, grouping · `6e7c448a` RED extended curve always reported · `3c380cfd` fix · `d4098e00` synthetic
generator + golden · `f84d56a1` streaming · `ffe28ee9` generator lint · this HANDOFF.

## Original gates
`cargo test --release -p import-lrcat` (all targets incl. golden + scale), `-p tessera-ffi --test lrcat`,
`-p tessera-ffi --lib lrcat`, and `-p tessera-cli --test import_models`: pass.
`cargo clippy --release --all-targets -p import-lrcat -- -D warnings` and `-p tessera-cli -- -D warnings`: clean.
`cargo fmt --all --check`: clean. `apps/mac/build-ffi.sh`: ok. `tools/orchestrate/swift-gate.sh`: SWIFT GATE OK
(861 XCTest tests, 3 skipped, 0 failures; 5 swift-testing tests).

## Machine A review follow-up (2026-10-01)
Preserved the existing branch history through `324566da`; all review commits are additive and local.
`6034edef` is the RED regression commit: four importer failures reproduced before fixes (oversized source,
individual unedited notes, duplicate develop row ordering, identity extended curves). CLI tests also cover
last-write-wins duplicate image records and injected pre-rename error / skipped rename.

Duplicate `Adobe_images.id_local` records and duplicate develop `image` records now select the last SQLite
rowid, with a report entry. Image records are deduplicated before streaming to the writer, so recipes and
plan agree and duplicate file creation cannot abort apply. Develop rows sort by image/rowid; NULL ids and
orphan ids do not block later valid rows. Identity/path structure errors otherwise remain strict.
The publish fault tests assert both absence of the destination and cleanup of the staging directory.
The golden digest remains pinned to B5-29b; no expected bytes were changed.

Review fix: `5cba6b63` (`fix(B5-29c): preserve oversized develop failures and make apply deterministic`).
Rust verification: 53 importer tests (including 9 review follow-ups, 2 golden tests and the scale test),
7 FFI `--test lrcat` tests, 4 FFI `--lib lrcat` tests, 4 CLI `import_models` tests, and the CLI publish-failure
unit test (both injected cases) passed. After the clippy style correction, the 9 follow-ups and 2 golden tests
were rerun and passed. `cargo clippy --release --all-targets -p import-lrcat -p tessera-cli -p tessera-ffi --
-D warnings` and `cargo fmt --all --check` passed. `apps/mac/build-ffi.sh` completed with unchanged bindings.

The first Swift gate run completed 861 XCTest tests (3 skipped) with 9 assertions failing across
`DocumentHistoryKeyboardTraversalTests.testDocumentSwitchKeepsGlobalHistoryPreferencesAndShowsSelectedDocument`
and `testInspectorTabSwitchKeepsHeightControlIdentityFocusAndValue`; all 5 Swift Testing tests passed.
Several other Swift test processes were active on the host. These unchanged UI tests use standard shared
preferences and short layout waits; contention is a possible cause, not established by this run. No Swift
code or test expectation was changed to bypass the failures. The final gate retry is recorded below.


### Fresh performance and counts (review fix)
Read-only catalog COPY only; `/usr/bin/time -l`, release CLI rebuilt from the review fix. SHA-256 prefix
`eb60e744dbec2547` matched before and after. All three targets still hold.

| operation | wall | peak RSS (bytes) | peak RSS (decimal GB) | target |
|---|---:|---:|---:|---|
| inspect | 13.49 s | 504,053,760 | 0.504 | <20 s, <1 GB |
| apply | 17.22 s | 622,034,944 | 0.622 | <60 s, <1 GB |

Apply published 21,656 recipes and 21,658 total bundle files (2,827,345,328 bytes). The report now has
548,861 entries, including 18 grouped entries, 41 individually listed empty/unedited images, and no degraded
rows. The extended-curve limitation is one grouped entry covering 20 images. The increase from the original
105-entry report is intentional: only unknown-Lua-key notes and the explicitly requested extended-curve
limitation remain grouped. Every other note, including each image requiring action, remains individual.
An independent read-only curve audit confirms 737 rows with these keys, 720 identity master curves (718
simple two-point + 2 with intermediate points), and 3 identity-master rows with edited channels. Therefore
717 rows get no extended-curve warning, and 20 correctly retain it. No catalog names or paths are recorded.

Every published recipe JSON file was parsed, its filename matched against the complete catalog image-id
set, and library JSON parsed successfully. The task-owned measurement bundle was then removed; the catalog
copy and source originals were not changed.

The second unchanged Swift run passed both History tests but reported one failure in
`ShellLayoutTests.testShellContainedAtEverySizeStateAndAppearance` (861 tests, 3 skipped; 5 Swift Testing
tests passed). Other XCTest processes had started during that run. A temporary Foundation-home probe did
not demonstrate preference-file isolation on this host, so no such override was used for the final rerun.

The third unchanged run finished with 6 assertions failing across 4 existing UI tests (861 tests, 3 skipped;
5 Swift Testing tests passed). Its preserved log includes a height preference written as 10000 subsequently
reading 192 while another XCTest process was active. This is consistent with cross-process preference
interference, without proving causation. A fourth run was queued to wait for 60 seconds with no Swift test runner before starting the same gate.
External suites kept restarting, so it did not execute and the task-owned wait was cancelled. No test skips,
assertion changes, or environment overrides were introduced.


### Final review-gate status
**Rust, golden, clippy, fmt, FFI regeneration, real-copy performance and bundle validation: PASS.**
**Swift gate: NOT OK / blocked on a coordinated quiet run.** No `SWIFT GATE OK` was obtained for this review
fix. The third run's exact failure methods were:
- `DocumentHistoryKeyboardTraversalTests.testExternalPreferenceWriteUpdatesHostedReadout`
- `DocumentHistoryKeyboardTraversalTests.testInspectorTabSwitchKeepsHeightControlIdentityFocusAndValue`
- `DocumentHistoryKeyboardTraversalTests.testWindowResizeClampsDisplayWithoutRewritingOversizedRequest`
- `ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize`

The second run passed both first-run History failures; the failing set changed across three unchanged runs.
The gate still needs to be rerun with other Swift/XCTest jobs paused. There are no task-owned background
builds/tests queued or running. All commits are local and additive; `board.json` and `Cargo.lock` are untouched.
