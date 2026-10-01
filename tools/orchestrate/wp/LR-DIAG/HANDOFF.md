# LR-DIAG: one shared channel for approximate Lightroom translations

Branch `wp/LR-DIAG`, worktree `/Volumes/betterSSD/tessera-worktrees/lr-diag`,
2026-10-01. Implements the `approximate` matrix status from the coordinator rulings
(docs/coordination/CODEX-BRIEF-2026-09-30.md, rulings log). No matrix row is flipped.

## Commits

- Base: `c725429172ee4ebd4dec2e46aee447af5ec46a8b` (origin/main).
- RED: `81fe9c3b`, `test(LR-DIAG): RED shared approximate-diagnostics channel, matrix guard and report group`.
  It contains API stubs and the additive `LrcatReport.approximate` field with regenerated bindings.
  Failing at RED: 3 diagnostics unit tests, 2 matrix guard tests and the FFI report test. The Swift tests
  were also expected to fail at RED but were not run there.
- GREEN: `783f6dee21f586acbc9b57571aa9ad8d72244fc8`, `feat(LR-DIAG): shared approximate-diagnostics channel, matrix guard and report group`.
- Docs: `a736b810` (matrix legend and first handoff).
- Review fix: `7626011166f36903cc8bb007c2086328788d115f`, `fix(LR-DIAG): apply review: fail closed on foreign shapes, workspace-wide key scan, field match, AX paths`.
- The following `docs(LR-DIAG):` commit updates this file. It cannot contain its own hash.

## API (`crates/import-lrcat/src/diagnostics.rs`)

```rust
pub const KEY: &str = "lrcat_translation_diagnostics";
pub struct Entry { level, status, lane, field, reason }   // all String, serde
pub fn push_approximate(recipe: &mut Recipe, adobe_key: &str, field: &str, lane: &str, reason: &str);
pub fn entries(recipe: &Recipe) -> BTreeMap<String, Vec<Entry>>;
```

The data is stored in `recipe.unknown["lrcat_translation_diagnostics"]`. It is a JSON object keyed by Adobe
key, and each value is an array of `{"level":"info","status":"approximate","lane":"LR-n","field":"/settings/...","reason":"..."}`.
`push_approximate` appends an entry and skips an identical one. It creates the object or list when absent and
never touches another key's list or any other `unknown` member. If the value under the key is not an object, or
a key's value is not an array (for example a foreign ad-hoc writer such as LR-6's top-level array), it writes
nothing and leaves that value untouched. Debug builds hit a `debug_assert!`. The matrix guard therefore fails
closed. `entries()` returns nothing for a wrong-shaped object and skips malformed entries. A recipe
without diagnostics has no such member, so it serializes byte-identically. The import-lrcat `golden.rs`
digests and the engine-api tests pass unchanged.

A unit test (`no_other_source_writes_the_key_directly`) fails if any `.rs` or `.swift` file under any
`crates/*/src` or `apps/mac/Sources` contains the key string. The one exception is
`crates/import-lrcat/src/diagnostics.rs`, matched by full path. Tests directories are not scanned. A lane
branch that still writes the key itself fails this test at merge time.

## Converting a lane (other branches)

1. Replace your ad-hoc diagnostics insert with
   `import_lrcat::diagnostics::push_approximate(&mut recipe, "<AdobeKey>", "/settings/<field>", "LR-n", "<reason>")`.
   Delete your own key and any reader of it. Readers use `diagnostics::entries(&recipe)`.
2. Keep the exact source in `recipe.unknown["lrcat_develop_source"]["properties"][<AdobeKey>]`.
   Do not remove it when you translate. Emit zero warnings for the key.
3. Flip the matrix row in `docs/coordination/LR-TRANSLATION-MATRIX.md` to `approximate`. Set the recipe path
   column to the populated JSON pointer and the fifth column to a synthetic Lua value whose import changes that
   field from its empty-row default. `translation_matrix.rs` then enforces four things: the field is populated,
   the source is retained, there is at least one info/approximate entry, and there are zero warnings.
4. Nothing else is needed for the UI. The import report groups the key automatically.

## Matrix guard (`crates/import-lrcat/tests/translation_matrix.rs`)

`check_rows(matrix, import)` takes an injectable synthetic importer. Production uses `lua_develop::parse`. The
inventory check is separate. The new status is `approximate`, and its requirements are:

- a concrete key and a synthetic value;
- the JSON pointer exists and differs from an empty row's (`s = {}`) value;
- the source is in `lrcat_develop_source`;
- `entries()[key]` has an info/approximate entry whose `field` equals the row's recipe path (the lane is
  free-form);
- the value at the path is not JSON null (null also counts as missing for `translated` rows);
- there are no warnings.

`translated` rows now also fail if they carry a diagnostics entry. The positive case uses a test-only fixture
row (`Exposure2012 → /settings/tone/exposure`) with a synthetic lane. There is one negative test for each
dropped condition (field, including a bad path; source; diagnostic; warnings; a diagnostic that names another
field). Further negatives cover a missing
synthetic value, a translated row carrying a diagnostic, and the fixture row against the unconverted parser.
The real matrix has no `approximate` rows.

## Import report (B5-46)

- FFI: `LrcatReport.approximate: Vec<LrcatIssue>` is a new, additive field. Each entry has:
  - `category` = Adobe key;
  - `count` = photos written or resumed in this run that carry the key;
  - `reason` = the first photo's first reason;
  - `examples` = up to five paths.

  Entries are sorted by key, kept separate from `unsupported` and are not warnings. They are filled in
  `apply` by `note_approximate`.
- Swift: the report sheet (`ReportStep`) shows a separate "Approximate translations" group using `IssueRow`.
  Its AX identifier is `document.import.report.approximate`, and its value comes from
  `LightroomImportReport.approximateLines` (`"<Key>: <n> photo(s); e.g. <reason>; <path>, <path>"`). This
  includes the example paths, like the on-screen rows. `import-report.md` gains a
  `## Approximate translations (n)` table, shown only when the list is non-empty.
- Tests:
  - `lrcat::lrcat_resume_tests::approximate_translations_populate_the_report`: a synthetic approximate import
    rewrites the spooled records through `push_approximate`, then runs `apply`. A second `apply` resumes every
    photo and produces the same groups, which covers resumed-photo counting.
  - `approximate_groups_count_every_photo_but_cap_examples_at_five` uses seven photos and checks that the
    group keeps the first photo's reason.
  - `tests/lrcat.rs`: a plain import has an empty `approximate` list.
  - Swift `testReportMarkdownGroupsApproximateTranslationsPerAdobeKey` checks the markdown.
  - The AX test checks the sheet group and that the group is absent from the warnings.
- The summary step (pre-import) does not show the group. Only the post-import report does.

## Gates

Each set was run sequentially. Exits at GREEN `783f6dee` / at review fix `76260111`:

| Gate | GREEN | Review fix |
| --- | --- | --- |
| `cargo test --locked -p import-lrcat -p tessera-ffi -p engine-api --release` | 0 (750 passed, 0 failed, 29 ignored) | 0 (754 passed, 0 failed, 29 ignored) |
| `cargo clippy --locked --workspace --all-targets --release -- -D warnings` | 0 | 0 |
| `cargo fmt --all -- --check` | 0 | 0 |
| `bash tools/orchestrate/swift-gate.sh` | 0 (911 XCTest, 3 skipped, 0 failures; 5 swift-testing) | 0 (911 XCTest, 3 skipped, 0 failures; 5 swift-testing) |
| `git status --porcelain \| grep -v fixtures/raw \| wc -l` (after the docs commit) | 0 | 0 |
| `swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors` | 0 | 0 |

The diagnostics unit tests also pass in a debug build, where the foreign-shape tests catch the `debug_assert!`.

Bindings: `build-ffi.sh` regenerated `TesseraFFI.swift` in RED. The swift gate's rebuild produced no further
diff. The only linker output is the existing `blake3_neon.o` macOS-version `ld` warning.
