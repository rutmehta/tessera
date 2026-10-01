# B5-29c re-review follow-up, 2026-10-01

This section supersedes the historical handoff below. Work is additive on reviewed `041456c9`,
on branch `wp/B5-29c`; no rebase, history rewrite, push, board edit, or lockfile change.

## B2 report rule

Keep separate per-image entries **only** for messages containing `imported as unedited` and the
exact duplicate-image/develop-ID `last-write-wins` notices. Group every other image note by message,
including unsupported CRS properties, decoder limitations, and future harmless notes. Repeated
messages render as count plus first image ID; singleton messages retain the existing image-ID form.
Repeated diagnostics within one image count that image once. Stable first-image ordering is preserved.
Memory for harmless image diagnostics now follows distinct message count, not image count; the required
unedited-image and duplicate-ID lists can still grow with the number of affected images.

The unit test feeds two images every note class (plus an intra-image repeated unsupported note) and
asserts 13 entries, exact grouped counts/first IDs, individual exceptions, and singleton shape. Before
the fix it produced 18 entries and failed. New shape tests also failed before implementation.

## Cheap recommendations included

- After publishing by rename, the destination parent now uses `File::sync_all` (full flush on macOS),
  replacing the plain-fsync directory helper for this final publication boundary.
- `lrcat_develop_source` now has explicit shapes. Parsed maps use
  `{shape: "lua-values" | "xmp-fragments", properties: {AdobeKey: exactSource}}`.
  The envelope avoids collisions with real Adobe keys named `shape` or `properties`.
  Oversized source uses `{shape: "cell-descriptor", cell, truncated, processVersion, text?}`;
  `cell.status` still distinguishes externalized and omitted. Existing parse-failure whole text is a
  fourth shape, `{shape: "raw-text", text, processVersion}`. Exact source values are unchanged.
- Empty develop settings now say `never developed (no develop settings)`; nonempty failed/oversized
  imports say `edits failed to import` with their existing detailed reason. Both keep an individual
  `imported as unedited` report entry.
- **Export limitation:** `XmpPacket::from_imported_recipe` requires retained `sidecar_xmp` and returns
  `no source XMP` for Lua-row recipes because synthesized XMP is intentionally removed. These source
  tags do not add Lua-to-XMP export support.
- **FFI/in-app limitation on this branch:** `open_lrcat` still calls `import_lrcat::import`, materializes
  images, and records oversized cells as `omitted` with a bounded prefix/recovery descriptor. It does
  not use bundle side files. B5-29d may add app-path side files; CLI results below do not establish app
  performance or lossless oversized app import.

## Golden and commits

- RED: `adc415fc` — `test(B5-29c): cover report grouping and retained-source shapes`.
- Fix: `6be9003b` — `fix(B5-29c): bound harmless reports and tag retained sources`.
- Documentation follows in a separate `docs(B5-29c):` commit.

The full-source golden first failed with the tagged envelope, then was intentionally re-pinned from
`fcbb457c63eba5adc6256d8c64874a91a5a9408abb4bf46cb692cfe36ce5415a` to
`d42640939d17a76668916260b58d77a568c5979f84d23c285480f7c1fd7441b8`.
This is solely the retained-source shape change; no source stripping was added to the golden.
Per-key exact literal/fragment assertions now look inside `properties`; they still verify original bytes.
Streaming/import and PlanJson serialization parity also pass.

## Fresh catalog-copy measurements (counts only)

Used only the supplied scratchpad `lrimport/cat.lrcat` COPY, read-only. Never opened the original under
`~/Pictures`. Full SHA-256 matched before and after, with prefix `eb60e744dbec2547`.
Standalone release CLI rebuilt before `/usr/bin/time -l` measurements; inspect and apply ran serially.

| Operation | Prior round wall s | New wall s | Prior peak RSS bytes | New peak RSS bytes |
|---|---:|---:|---:|---:|
| inspect | 9.81 | 9.78 | 591,904,768 | 536,821,760 |
| apply | 11.46 | 11.13 | 764,100,608 | 647,823,360 |

Both pass inspect <20 s / apply <60 s / peak <1 GB. These are single-run measurements, not statistical
speedup claims. Prior values and the baseline report count are from the prior run recorded below.

Report **548,861 → 146 entries**. Of the new entries, 77 are repeated-message groups (previously 18),
15 are harmless single-image notes, 13 are catalog-level notes, and 41 are individually listed unedited
images. Thus 105 non-unedited entries plus 41 individual unedited entries. All 41 say never developed;
0 failed-edit imports and 0 duplicate-ID notes. There are 21,656 images and recipes, 21,615 `lua-values`
source envelopes, 21,658 bundle files, and 2,849,688,999 bundle bytes.

Validation streamed every plan image, compared each recipe to its recipe file, checked distinct IDs
against all recipe filenames, matched the library to the plan, and matched the full report to
`--apply --json`. All passed. No image names, keywords, collections, or original photo paths are recorded.
The task-owned temporary bundle is removed after validation; counts, timings, and gate logs are retained.

## Gates for this follow-up

- Requested release `--test lrcat`: PASS (7 tests).
- Requested release `--lib lrcat`: PASS (new importer grouping regression plus 4 FFI tests).
- Broader release suite for import-lrcat, tessera-cli, tessera-ffi: PASS, **654 passed, 28 ignored, 0 failed**.
- `cargo clippy --release --all-targets -p import-lrcat -p tessera-cli -p tessera-ffi -- -D warnings`: PASS.
- `cargo fmt --all --check`: PASS.
- `cd apps/mac && ./build-ffi.sh`: PASS; generated Swift/header/modulemap unchanged.
- `tools/orchestrate/swift-gate.sh`: **FAILED / INCOMPLETE**, exit 1. Swift build passed.
  `MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth` failed at
  `MasksPanelLayoutTests.swift:88`: `screencapture` could not create an image from the window, followed
  by `XCTUnwrap` of a nil `CGImageSourceRef`. The console session was verified locked before and during
  the run. All four shell-layout tests passed this time; no additional assertion failure was recorded.
  After the existing opt-in Smart Preview skip, the test process stopped logging for over two minutes
  and fell to 0% CPU. A 3-second sample showed 6.0 GB physical footprint (7.2 GB peak) and blocking
  AppKit animation workers. Only this worktree's exact XCTest PID 7569 was terminated after verifying
  its full executable/bundle path, existing failure, and >120 seconds without log output. The gate then
  reported failure. This is not a completed Swift suite and no `SWIFT GATE OK` was obtained.
  An unlocked full rerun remains required; no Swift source, assertion, skip, UI activation policy, or
  gate implementation was changed. Full output and sample are retained as
  `/tmp/B5-29c-rereview-swift-full.log` and `/tmp/B5-29c-rereview-swift.sample.txt`.

Logs for this follow-up use `/tmp/B5-29c-rereview-*` (Rust tests, clippy, formatting, CLI build,
measurements/counts, before/after SHA-256, FFI build, and Swift gate).

---

# Historical handoff before this re-review

# B5-29c handoff: Machine A binding contracts, 2026-10-01

Branch `wp/B5-29c`. This round is additive on `1f98b17d`; no rebase, history rewrite, push, or board edit.
Read the independent review after its `tokens used` marker and the complete branch-only commit list.
The prior `5cba6b63` fix already supplied `apply_with_publish`, individual unedited reporting,
last-row-wins joins, and Lua identity-curve handling. This round verified those and closes the remaining gaps.

## Contracts

1. **Unconditional source retention.** `recipe.unknown["lrcat_develop_source"]` maps Adobe keys to exact
   Lua value literals, or exact XMP element/attribute fragments for XMP rows. Retention does not depend
   on decoder warnings or on whether the current value is active. It includes `MaskGroupBasedCorrections`
   (including nested `Mask/Image`), inactive/active `LensBlur`, `RetouchAreas`, `RetouchInfo`, `PointColors`,
   every `ExtendedToneCurve*`, every `Upright*`, and unmapped/pending keys. The predicate conservatively
   retains every key absent from `CrsKey`, including newly added unmapped `KEY_MAP` entries. Parse failures
   still retain the whole source under `{text, processVersion}` because there is no trustworthy parsed key map.
   Lua-generated XMP is removed; original XMP packets remain on actual XMP rows. Decoder warnings on Lua rows
   now say `source preserved per property`, not `retained in original XMP`.

   Ordinary unknown string keys remain available in `lrcat_develop_lua`. The authoritative collision-free
   unknown-key representation is `lrcat_develop_lua_entries`: ordered `{key: {string: ...}|{number: ...}, value}`
   entries. Numeric `[1]` and string `"[1]"` cannot overwrite each other. Positional-root fallback lives in
   `lrcat_develop_lua_positional`, separate from an ordinary key literally named `"(positional entries)"`.

2. **Oversized cells (>8 MiB).** No oversized cell is represented as genuine NULL. The bounded SQL projection
   uses `typeof`/`octet_length` to identify large values without loading SQLite overflow pages into a result
   or sort record. `rusqlite`'s existing dependency enables its `blob` feature; incremental BLOB I/O reads
   both TEXT and BLOB cells. No new package or lockfile change is needed.

   `import_each_with_storage` streams bytes into the private staging bundle at
   `large/<image-id-or-none>-<table>-<SQLite-rowid>-<hex-column-name>.bin`. Filenames distinguish tables,
   rows, images and columns, and column encoding prevents path traversal. Side files are synchronized
   before any referencing recipe can be published. A descriptor records `status: externalized`, `path`
   relative to the bundle root, byte `length`, `kind`, `table`, `column`, `rowid`, `scan_ordinal`, and
   `image_id`. It appears in the source row and/or report; oversized current develop text is retained at
   `lrcat_develop_source.cell` and always produces that image's `imported as unedited` note with recovery path.

   Inspect/non-bundle import instead records `status: omitted`, length and a prefix of at most 64 KiB
   (UTF-8 boundary for text; byte array for BLOB). Recovery names the original catalog table/row/column.
   Genuine SQL NULL stays NULL. Inspect is explicitly not a lossless bundle. Actual SQLite rowids are
   labelled `row`; the separate scan counter is labelled `scan_ordinal`/`scan ordinal`, never a fake rowid.

   The new isolated SQLite allocation regression first failed: a 32 MiB TEXT cell caused a 64 MiB SQLite
   allocation even though Rust merely borrowed it. Both inspect and externalized apply now pass the
   <8 MiB largest-allocation assertion, and the 32 MiB external file is checked byte-for-byte with a bounded
   buffer. CLI integration separately verifies oversized develop TEXT and history BLOB files and references.

3. **Degraded images and report order.** Every absent/empty, invalid, missing-process-version or oversized
   develop row imports as unedited with its own image ID and reason. The existing FFI summary preserves
   distinct unedited-image issues, each with count 1, and its integration tests pass. Only harmless
   unknown-Lua-key messages and the explicitly required named extended-curve limitation are grouped.
   Catalog/oversized-cell diagnostics precede the per-image section. That section is stably ordered by
   ascending image ID (grouped entries by first image ID), not SQLite row order. Duplicate notices no longer
   jump ahead of lower-ID image diagnostics; the new ordering regression reproduced `[31,30,...]` before the fix.

4. **Duplicate IDs / joins.** `Adobe_images.id_local` duplicates and develop `image` duplicates use the
   highest SQLite rowid (last-write-wins), with a report entry. Images are deduplicated before recipes are
   enqueued, so `create_new` cannot fail because of duplicate image IDs. Tests cover out-of-order develop
   rows, orphan IDs both below and above real IDs, NULL develop image IDs, and duplicate image/apply rows.
   NULL/invalid identity on an actual image record remains a strict structural error. File/master-image,
   keyword and collection joins continue to use catalog local IDs.

5. **Durability and recovery.** Each recipe is serialized, validated, written with `create_new`, then
   synchronized by **plain `libc::fsync` on macOS, retrying EINTR**; other platforms use `File::sync_all`.
   Oversized side files use `File::sync_all`. `Library::write` synchronizes its temporary file, renames it
   to `library.json`, and synchronizes its directory. After all writer threads drain, the plan's BufWriter
   is flushed and `File::sync_all` synchronizes `import-plan.json`. On Apple, std `sync_all` is
   `fcntl(F_FULLFSYNC)`, not plain fsync.

   The CLI then synchronizes `recipes/`, optional `large/`, and the staging root with `sync_directory`
   (plain `libc::fsync` + EINTR retry on macOS; std elsewhere). It reserves an empty destination directory,
   renames staging over the reservation on the same filesystem, checks publication, and synchronizes the
   destination parent. A crash before rename may leave an empty reservation: retry uses nonrecursive
   `remove_dir` to recover it. Nonempty destinations are never removed/replaced. Tests inject a pre-rename
   error and a skipped rename, asserting no destination/partial bundle or staging leftovers; another test
   recovers an empty reservation and confirms retry cannot overwrite a published bundle.

6. **ExtendedToneCurve.** Identity master/channel polylines emit no limitation entry; names alone do not
   count as edits. Non-identity master or channel curves emit one named limitation per image, grouped by
   report aggregation. This is now tested on Lua and XMP rows. Raw source is retained in either case,
   including identity and nil Lua values. No HDR translation was invented; codec work remains with A.

7. **Nits.** Warning wording, true rowid vs scan ordinal, and image-ID report ordering are corrected and
   documented above. No FFI/Swift API changes.

## Full retained-source golden baseline

`tests/golden.rs` now hashes every unknown/source entry on every row; it no longer strips Lua keys.
Only `recipe.image_id`, derived from the absolute temporary catalog path, is normalized to None.
The 2,000-image synthetic baseline is intentionally re-pinned from
`32b8574f77399f028e85ef7436f6e77315b0753d7b996792ffd88db7e7c01fa9` (B5-29b translation-only)
to `fcbb457c63eba5adc6256d8c64874a91a5a9408abb4bf46cb692cfe36ce5415a` (this retention contract).
The new digest failed against the old pin before being adopted. Per-key tests assert literal source spelling,
inactive LensBlur, nil/identity retention, XMP fragments, and key-collision behavior independently of the hash.
Format-parity tests normalize only format-specific source when comparing Lua to XMP; the golden does not.
Streaming/import parity and exact `PlanJson` serialization checks still pass. Report text is not part of the digest.

## Test-first evidence

- Retention, numeric/string collision and omission descriptor assertions failed before their fixes.
- Empty-reservation recovery failed before the durability fix; pre-rename failure behavior already passed.
- XMP identity curves failed with an unsupported-property warning before the named-limitation fix.
- The initial CLI external-file assertion used an incorrect serialized `unknown` nesting (Recipe flattens it).
  After correcting that test, disabling bundle storage reproduced the intended RED (`omitted` vs `externalized`),
  and enabling it passed with exact byte checks for both develop TEXT and history BLOB.
- The SQLite allocation and duplicate-report-order tests failed before their respective fixes, as above.
- Previous round's RED coverage (`6034edef`) for individual degradation, last-write-wins and Lua identity
  curves was retained and verified; no accepted contract was reverted just to match an old expectation.

## Historical review context

`6ccc6eaa` changed an older mask parity fixture from attributes on `rdf:li` to an inner
`rdf:Description`, matching the Lua renderer. Both RDF forms decode to the same settings; raw retained
XML spelling differs. This round's separate exact-retention tests and full-source golden prevent that
format-parity normalization from hiding a source-retention regression.

The previous handoff's <20 s inspect / <60 s apply / <1 GB RSS targets remain in force. Its former
byte-identical-bundle claim and translation-only golden no longer describe this intentionally changed
retention format. The app's FFI `import()` path still materializes all images; the real-catalog performance
measurements below cover the streaming CLI. History BLOB JSON arrays remain unchanged, including their
size cost; a compact history representation is still A's format decision.

## Fresh catalog-copy measurements and verification

Only the supplied read-only catalog COPY was used. The original catalog under `~/Pictures` was not opened.
Measured with `/usr/bin/time -l`, rebuilt release CLI, serial inspect then apply. SHA-256 prefix
`eb60e744dbec2547` matched before and after; the full SHA-256 was compared, not just the prefix.

| Operation | Wall seconds | Peak RSS bytes | Peak RSS decimal GB | Targets |
|---|---:|---:|---:|---|
| inspect | 9.81 | 591,904,768 | 0.592 | PASS |
| apply | 11.46 | 764,100,608 | 0.764 | PASS |

Targets: inspect <20 s, apply <60 s, each peak <1 GB. All met.

Counts only: 21,656 images; 21,656 recipes; 21,658 bundle files; 2,889,014,776 bundle bytes. Report: 548,861 entries, 18 grouped entries, 41 individually listed unedited images, 0 decode-degraded images, and 0 oversized-cell diagnostics. The grouped non-identity extended-curve limitation covers 20 images.

Unconditional raw-source entry occurrences (including inactive/empty values, not counts of applied edits):

| Adobe source key | Recipe occurrences |
|---|---:|
| `MaskGroupBasedCorrections` | 697 |
| `LensBlur` | 17,133 |
| `RetouchAreas` | 330 |
| `RetouchInfo` | 11,543 |
| `PointColors` | 17,132 |
| `ExtendedToneCurvePV2012` | 737 |
| `ExtendedToneCurvePV2012Red` | 737 |
| `ExtendedToneCurvePV2012Green` | 737 |
| `ExtendedToneCurvePV2012Blue` | 737 |
| `UprightFourSegmentsCount` | 21,615 |
| `UprightTransformCount` | 21,615 |

Validation parsed every real plan image one at a time and compared its recipe to the corresponding recipe
file. The full distinct catalog-ID set matched the recipe filenames and plan IDs; library JSON and the
complete report matched the plan. No duplicate plan image or recipe was present. The task-owned bundle
was removed after successful validation. No real catalog image names, keywords, collections or photo paths
are recorded here. Synthetic oversized-cell tests cover external storage because this copy has no oversized cells.

## Gates

- Requested `cargo test --release -p import-lrcat -p tessera-cli -p tessera-ffi --test lrcat`: PASS (7).
- Requested `cargo test --release -p import-lrcat -p tessera-cli -p tessera-ffi --lib lrcat`: PASS (4).
- Broader `cargo test --release -p import-lrcat -p tessera-cli -p tessera-ffi`: PASS, 651 tests passed,
  28 existing ignored tests, no failures. This includes importer golden/scale, CLI apply and FFI integration.
- After the clippy-only conditional cleanup, retention/golden/SQLite-memory tests passed again. The final
  expanded follow-up suite (all unmapped KEY_MAP entries plus exact XMP attribute spelling) passed all 15 tests.
- `cargo clippy --release --all-targets -p import-lrcat -p tessera-cli -p tessera-ffi -- -D warnings`: PASS.
- `cargo fmt --all --check`: PASS. `Cargo.lock` and `board.json` unchanged.
- `cd apps/mac && ./build-ffi.sh`: PASS; generated bindings unchanged.
- Swift gate: **BLOCKED / NOT OK**. Both unchanged attempts failed the masks window-capture test; the second also reported an
  unchanged shell-layout assertion failure. No `SWIFT GATE OK` has been obtained. The console session is locked; an unlocked full rerun is required.

The first unchanged Swift attempt reported
`MasksPanelLayoutTests.testPopulatedInspectorKeepsComponentActionsReadableAtMinimumWidth`: `screencapture`
reported "could not create image from window", leaving no valid PNG for ImageIO. Shell/history tests passed.
The run then stopped making progress after the opt-in Smart Preview skip. A 3-second sample of this
worktree's XCTest process showed the dispatch soft limit (80) reached throughout, with many AppKit
`NSAnimation._runBlocking` workers; its physical footprint was 6.0 GB (peak 7.3 GB). After several minutes
without log progress, only this task's already-failed XCTest process was terminated. The gate correctly
reported failure; no assertion was changed and no new test was skipped. Full log and sample were retained
at `/tmp/B5-29c-swift-tests-full.log` and `/tmp/B5-29c-swift-hang.sample.txt` for diagnosis. Other worktrees'
Swift/XCTest processes were active; that is observed context, not a proven cause of the failure/stall.

The second unchanged attempt reproduced the same masks capture failure and also failed
`ShellLayoutTests.testDocumentInspectorEveryTabAndHistoryStateAtEverySize` at line 194:
`document-960x600-channels-history-open region historyBody not laid out`. Its history tests and the other
shell-layout tests passed. The already-failed run was stopped. A read-only I/O Registry check reported `CGSSessionScreenIsLocked = true` for the
on-console session. This supplies a concrete environmental blocker for the window-capture test; an unlocked
rerun is required before claiming the gate passes. Only this worktree's failed second XCTest process was
terminated. Its full log is `/tmp/B5-29c-swift-tests2-full.log`; gate output is
`/tmp/B5-29c-swift-gate2.log`. No Swift source, capture implementation, assertion, preference domain, test skip,
GUI activation policy or gate script was changed to bypass this. The user was asked to unlock the Mac.

All authorized Rust implementation, verification, real-copy measurement and bundle validation are complete.
The remaining acceptance step is the unchanged full Swift gate on an unlocked session. Local commits remain
on top of the reviewed branch; no push, rebase, history rewrite, board update or Cargo.lock edit.
