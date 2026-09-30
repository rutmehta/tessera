# B5-29 handoff: Lightroom catalog import on a real LrC 15.5 catalog

Branch `wp/B5-29` from `origin/main` `1d74771b`. Changes only in `crates/import-lrcat` (lib, fixture, one new
test file). No FFI/Swift API change, no new dependencies.

**Status: BLOCKED on Machine A design input.** Inspect of the real catalog still fails, now on the develop
settings encoding (see "Blocker"). Everything below the blocker is fixed and tested.

## Fixed (report, don't abort)
- Keyword tree: Lightroom's single unnamed root (NULL name, NULL parent) is the tree root and is not emitted;
  its children are the top-level keywords. Other unnamed keywords are skipped with a `plan.report` entry and
  their children move up to the skipped keyword's parent. A second unnamed root gets a report entry
  ("second unnamed root"); its children are imported at the top level.
- Collections: NULL name imports as `Untitled <id>` with a report entry; NULL `creationId` or any kind outside
  `com.adobe.ag.library.*` (e.g. `com.adobe.ag.slideshow.unsaved`) is skipped with a report entry;
  `com.adobe.ag.library.group` (LrC's collection set) now maps to an album group (it was imported as an album).
- Smart collections with no rules, or rules the Lua reader rejects (e.g. `value_units`), are skipped with a
  report entry instead of aborting.
- Empty develop rows (`text` = '' / NULL processVersion, LrC's never-developed images) import as unedited.
- Still strict: `id_local`, file/folder/root identity and path columns, keyword hierarchy cycles/missing parents.
- Synthetic fixture (`src/fixture.rs`) now has the unnamed root (`KEYWORD_ROOT = 100`) as parent of the top
  level keywords; `tests/real_catalog_shapes.rs` covers every case above.

## Real catalog (read-only copy only)
Copy: `scratchpad/lrimport/cat.lrcat`, sha256 `eb60e744dbec2547e1a68722d987f88487e7e12d3a4c9d09497cefc9f191e73b`,
LrC schema `1504001`. The original under ~/Pictures was not opened. Counts only below.

Source tables: 21,656 images / files, 121 folders, 33 keyword rows (1 unnamed root), 37 collections
(18 collection, 3 group, 15 smart, 1 slideshow.unsaved), 21,656 develop rows, 30,460 history steps,
9,293 faces, 220 stacks.

Progress on `tessera import lrcat --inspect`:
1. main: `decode error (lrcat): missing column name` (unnamed root keyword) - fixed.
2. then: `decode error (lightroom-saved-search): unknown rule field` (`value_units`) - fixed (skip + report).
3. now: `decode error (xmp): unknown token at 1:1` - **blocker**.

Scouting run (uncommitted patch that skipped Lua develop rows, NOT on the branch), to show nothing else fails:
exit 0 in 115 s; images 21,656, virtual copies 0, folders 121, keywords 32, albums 18, album groups 3,
smart albums 3 (12 skipped: rules not translatable), stacks 220, faces 9,293. Diagnostics expected:
12 smart collections + 1 slideshow skipped, 21,615 develop rows untranslated (Lua), 41 empty develop rows.
The `--apply --dest` run was not done: with the blocker it would import every edited photo as unedited.

## Blocker (needs Machine A)
`Adobe_imageDevelopSettings.text` in LrC 15.5 is a Lua table serialization (`s = { AutoLateralCA = ..., ... }`),
not XMP: 21,615 of 21,656 rows (the other 41 are empty). The importer passes it to `xmp::parse`, so every
edited image is a decode error. Needs a design decision: a data-only Lua develop-settings reader (the saved-search
reader in `src/lua.rs` is rule-specific) mapped onto the crs key tables used by `xmp.rs`, or another source
(e.g. history-step text / sidecars). Degrading to "unedited + report" was not done because it silently drops
all edits.

Also for A:
- Performance: the per-image loop does linear scans of files, develops, history, faces, etc., so inspect took
  115 s on 21k images (quadratic). Index by id before this ships.
- Smart collection rules: real LrC shapes the Lua reader rejects are `value_units` on `inLast` date rules and
  unary `empty`/`notEmpty` rules with no `value` (mostly third-party plug-in `sdk:`/`sdktext:` criteria);
  12 of 15 smart collections in this catalog are skipped.

## Gates
cargo test --release -p import-lrcat; -p tessera-ffi --test lrcat; -p tessera-ffi --lib lrcat;
clippy --all-targets -D warnings -p import-lrcat; fmt --all --check: all pass. build-ffi.sh + swift-gate.sh:
SWIFT GATE OK (861 XCTest tests, 3 skipped, 0 failures; 5 swift-testing tests).
