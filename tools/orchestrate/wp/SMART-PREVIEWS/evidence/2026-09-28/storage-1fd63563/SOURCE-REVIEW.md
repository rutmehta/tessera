# Source-only Smart Preview journal and admission review

Reviewed the two new files in `/Users/rutmehta/.codex/worktrees/export-integration/tessera` against `docs/superpowers/specs/2026-09-28-smart-previews.md` and `/tmp/tessera-smart-preview-storage-ui-design.md`. No source edits, builds, tests, or subagents. These modules are not declared in `lib.rs`, so existing crate builds do not validate them. All described tests remain UNRUN.

## Findings

### P1 — Missing `incarnation` initializer prevents the journal module from compiling

`crates/tessera-ffi/src/smart_preview_store.rs:217-222` constructs `SmartPreviewJournal` without initializing the required `incarnation: [u8; 32]` field declared at line 69. Rust rejects this struct literal when the module is enabled. Initialize the temporary handle's incarnation before create/open replace it with the validated record identity. A build with the module declared is required before any test-pass claim.

### P1 — First journal creation does not durably publish its directory ancestry

`crates/tessera-ffi/src/smart_preview_store.rs:205-209` creates `smart-previews/<image-id>` using `create_dir_all`, but `write_record` only syncs the file and its immediate containing image directory (`:369-374`). When either directory is newly created, syncing the image directory does not ensure the new image-directory entry in `smart-previews`, or the new `smart-previews` entry in the app-support root, survives a crash. Thus `create` (and a subsequent offline save to that newly created hierarchy) can return success while the hierarchy naming its durable file has not itself been durably published. Sync each parent whose directory entry was created, or use a helper that establishes the directory ancestry durably before publication. This matters directly to the durable-offline-save prerequisite; ordinary reopen tests cannot establish crash durability.

### P2 — Clean discard returns success before its deletion is durable

`crates/tessera-ffi/src/smart_preview_store.rs:200-201` unlinks the journal and immediately returns success without syncing its containing directory. A crash can resurrect a successfully discarded clean journal; a subsequent build using `create` will then return `AlreadyExists` (`:113-114`). Sync the containing directory after unlink, consistent with the durable publication path. This does not discard dirty edits (the dirty and CAS checks run first), but successful cleanup is not durably reflected.

## Source assessment of the bounded components

- Journal recipe and baseline bytes are stored verbatim; validation parses but does not reserialize them. Unknown recipe/document members therefore survive these storage APIs. Existing payload digests, owner checks, version checks, positive source length, nonzero generation, bounded file reads, and serialized-output size checks are present.
- Save/snapshot/discard compare both revision and incarnation while holding the canonical per-record process mutex. Create refuses an existing journal. Stale handles cannot replace/delete a recreated record merely because its generation returned to one. Canonicalized directory roots make ordinary root aliases share a mutex. This remains process-local locking, not cross-process admission/CAS.
- Save writes a unique temporary file, syncs it, renames it and syncs the image directory. Pre-publication failures preserve the old record and attempt temporary-file cleanup. A directory-sync failure after rename produces an error after the new record may already be visible; callers must reopen after such errors rather than assume no state changed. The current module documents revision-conflict recovery but integration must account for this ambiguous commit case too.
- Dirty save state cannot be removed via `discard_clean`; that method validates both the current revision/incarnation and dirty flag first. Adjacent image journals use separate paths. No source/original-sidecar writes occur in this module.
- No actionable source defect found in `image_edit_admission.rs`. Its weak registry reuses a live ImageId gate; Original and SmartPreview reservations exclude each other and direct recipe writes. Selection writes are allowed with Original and denied with SmartPreview. Opaque weak authority is checked against both gate and current reservation identity. Worker lease clones and validated operation guards retain admission. Final reservation drop does not acquire the gate mutex, avoiding the final-drop deadlock. Guards serialize participating operations, while different image gates remain independent.

## Remaining prerequisites, not defects in this deliberately bounded scope

- Declare both modules and run their unit tests; source review alone does not verify compilation or concurrency behavior. Add failure-path coverage for publication errors and directory durability helpers where practical.
- Integrate the shared image guard into all ordinary/proxy opens, direct recipe setters, selection setters, saves and relevant reads. Acquire image guard before existing destination guards and retain both through destination validation/publication. Keep the existing destination-based protection and owner baseline checks.
- Ensure save workers retain lease clones before session close can release admission. Weak authority alone intentionally cannot keep a worker authorized after the final lease/operation drains.
- Supply app-support root, actual original digest/length and correct full owner baseline from validated integration paths; this journal is not a proxy payload/manifest validator or reconnect reconciler.
- Repeated `discard_clean` currently returns NotFound after the first deletion; any product-level repeated-discard-is-safe contract needs an idempotent wrapper or explicit API adjustment.
- Proxy codec/generation, stale/source/calibration binding, journal-aware selection merging, reconnect conflict resolution, UI source status, original-only export, and offline restart integration remain outside these modules. No integrated offline-feature or shipping acceptance claim is justified.

## Scoped rereview — commit 3cff7441cdff395e09b2ce726188e50aeaf31e17

Reviewed the constructor, directory durability changes, future-schema validation, module declarations and their added tests. No code edits/builds/test execution by reviewer. Inspected actual `*release.command.txt`, `*release.log`, `*release.exit`, and `release-source-commit.txt` in `/Volumes/betterSSD/tessera-validation/smart-previews/storage`: release store tests 13 passed, admission 6 passed, complete FFI library 156 passed, and strict release Clippy (`-- -D warnings`) finished with exit 0. Historical debug logs are not the basis for this assessment. Root is responsible for final source-input manifest binding.

### Original findings disposition

- Missing constructor incarnation: **resolved**, `SmartPreviewJournal::new` now initializes `[0; 32]`; create/open replace it with the real validated identity. Both modules are declared and release compilation evidence exists.
- Unsynced clean deletion: **resolved**, `discard_clean` invokes `remove_file_and_sync_directory`, which syncs the parent after successful unlink and propagates sync failure.
- Unsynced new directory ancestry: **partially resolved, remains open** as the retry/race case below. Straight-line first creation now syncs each newly created entry's parent.

### P1 — Retry after parent-sync failure still permits nondurable ancestry

`crates/tessera-ffi/src/smart_preview_store.rs:380-388,407` stops walking as soon as an existing directory is observed, but creates that directory before syncing its parent. If creating `smart-previews` succeeds and syncing the app-support root fails, the helper returns an error leaving `smart-previews` visible. On retry, that directory is treated as an already-established ancestor: only the new image directory's parent is synced, never the app-support root containing the still-unconfirmed `smart-previews` entry. The retry can therefore report a successful durable journal while the ancestor publication was never synced. The added injected-failure test explicitly demonstrates that the directory remains visible, but does not retry. A concurrent create can also observe another caller's directory before its parent sync completes because this work precedes acquisition of the per-record lock. Ensure a successful call establishes durability of the relevant ancestry even when entries already exist from an earlier failed/in-flight creation. Add a failure-then-retry test that asserts the originally failed parent is synced before success.

### P2 — New directory helper rejects a supported symlink root on first creation

`crates/tessera-ffi/src/smart_preview_store.rs:380-388` uses `symlink_metadata` and tests the link's own file type, rejecting a symlink that resolves to a directory. With an existing empty real app-support directory and an alias symlink to it, `SmartPreviewJournal::create(alias, ...)` walks back through missing `smart-previews/<id>` to the alias, then returns `NotADirectory`. The former create/canonicalize path supported this root alias. The existing symlink test passes because it creates through the real path first, so `open(alias)` stops at the already-existing image directory rather than the alias. Resolve existing directory aliases consistently before establishing missing ancestry and retain the canonical lock key. Add first-create-through-alias coverage, not only reopen-through-alias.

No new actionable issue found in schema validation: it rejects unsupported future versions before mutation, exercises writer compatibility, and preserves input bytes rather than persisting the validation serialization. No admission changes requiring a new finding. Bounded primitive acceptance remains blocked on the above helper defects; integrated offline editing remains out of scope regardless of component acceptance.

## Final scoped rereview — commit 1fd635638f50b38776a69bb1c02374ccdbf6057b

**Approved as bounded journal/admission primitives; no remaining actionable finding in this scoped rereview.** This does not approve or claim an integrated offline editor.

Both remaining directory-helper findings are resolved:

- The API now explicitly requires an existing application-support root. `new` canonicalizes and checks that root before creating the two generated child directories. `ensure_child_dir_durable_with` syncs each child's parent even when the child already exists, including the raced `AlreadyExists` case. A retry therefore reestablishes the previously failed app-root/preview-root sync before returning success. The failure-then-retry test checks that the existing generated entry is synced again.
- Canonicalizing the application-support root first supports first creation through a symlink alias and keeps the shared canonical record lock key. The revised symlink test creates through the alias first and then opens through the real root.

The existing-root contract is acceptable for these bounded primitives: the caller supplies the established app-support root, and the store establishes its own generated entries beneath it. This review makes no claim about durability of arbitrary external filesystem ancestry or a physical power-loss experiment.

Verification evidence inspected, without running builds/tests: `smart-preview-store-dirretry-fix.command.txt` specifies `cargo test --release --locked -p tessera-ffi --lib smart_preview_store`; its log reports 13 passed, explicitly including retry-after-sync-failure and first-create-through-alias coverage; exit is 0. `rustfmt-dirretry.command.txt` checks both modules and its exit is 0. The immutable `1fd63563` journal blob hashes to `d9ffede2aaa68f3b836442bd791c8d7efb075c9c14b63a01da35241513cfed09`, matching `relevant-after-dirretry-fix.sha256`. This evidence is not dependent on later unrelated working-tree changes.

The prior 156-test full-library release result and 6-test admission result recorded above apply to the earlier 3cff7441 validation; admission source is unchanged. They are not relabeled here as a final-head full-suite run. The strict release Clippy result recorded above likewise remains attached to that earlier validation. No source edits or compiler use by reviewer.
