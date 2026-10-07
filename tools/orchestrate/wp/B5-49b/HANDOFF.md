# B5-49b — Machine A round-two corrections

Branch `wp/B5-49`, on requested base `ef8ef8ad`. Machine A remains the merger. Only the explicitly requested branch push is authorized. This follow-up supersedes the affected claims in [B5-49's handoff](../B5-49/HANDOFF.md).

## Item-by-item response

| Machine A finding | Status / implementation | Regression / evidence |
| --- | --- | --- |
| Tracked results rewritten on every gate; addresses cause drift | Done: `writeResults` returns unless `TESSERA_REGENERATE_KEYBOARD_RESULTS=1`. Sorted JSONL serialization replaces hexadecimal addresses with `<address>`. Normal tests write no tracked artifacts. | `testResultArtifactsRequireExplicitOptIn` (RED: 1 test, 1 failure, in a disposable directory); `testTraceAddressNormalizationIsDeterministic`; two clean-tree gates below. |
| Move focus claim from Tab into `setPanelsHidden(false)` | Done: removed Tab-only claim from `KeyRouter`; shared workspace method invokes the existing visible-viewport `claimKeyboardIfStray`, which only changes the responder within its own window and preserves valid control focus. | RED: 3 tests, 6 focus assertions failed. `testShowPanelsMenuActionClaimsStrayFocus`, `testScreenModeReturningToStandardClaimsStrayFocus`, `testLeavingDocumentModeRestoresPanelsAndClaimsStrayFocus`. Original Tab restoration coverage remains. |
| Step 22a is teardown, not PASS | Done: `run` supports an explicit success classification; 22a emits `TEARDOWN`, with an assertion in the combined test. | `testCombinedKeyboardChecklist`; regenerated result table. |
| Step 17 N/A too broad | Done: 17a directly focuses the real hosted Dither checkbox, sends Space through KeyRouter/window dispatch, checks one history entry, suppresses 30 repeats/release, rejects canvas pan, and verifies Undo restores model, checkbox and history head. Only 17b focus-ring traversal remains N/A. | `testCombinedKeyboardChecklist`, step 17a. |

## RED-first record

`0b5864b6` — `test(B5-49b): reproduce non-Tab focus and unconditional result writes`.

Both RED runs failed on the intended assertions before the fix. The artifact RED uses a temporary directory, leaving tracked results untouched. The first post-fix targeted run had layout-settle timeouts while the cold Rust build was compiling; it had no focus, Dither, or artifact-policy assertion failures. A later opt-in run also timed out in early layout steps despite no concurrent compiler from this lane; that failed artifact draft was discarded. The two lane-specific hosted helpers now allow five seconds instead of two for scheduling; the shared harness's 50 ms quiet-layout requirement and all behavior assertions remain unchanged. The shared harness itself is untouched.

## Regeneration policy

The tracked files remain at `tools/orchestrate/wp/B5-49/{RESULTS.md,GUI-RESULTS.md,focus-hosted.jsonl}`. Deliberate regeneration uses:

```sh
cd apps/mac
TESSERA_REGENERATE_KEYBOARD_RESULTS=1 swift test -c release -Xswiftc -enable-testing --filter DocumentKeyboardChecklistTests.testCombinedKeyboardChecklist
```

One passing regeneration is retained in the committed artifacts (one earlier failed draft was discarded). The retained checklist has 18 PASS, 10 N/A, 1 TEARDOWN, zero FAIL, and 109 consecutive trace records, all `keyWindow=false`. Both Markdown aliases match, and all three artifacts contain no hexadecimal memory addresses. The final targeted suites passed 18 tests with zero failures; the opt-in checklist passed 1 test with zero failures.

Routine gates leave that variable unset. Address normalization deliberately discards pointer identities rather than treating process addresses as stable identifiers. Sources are synthetic JPEGs and a synthetic engine document; no private pixels, catalog paths, or names are added.

## Gates and clean-tree proof

Both consecutive final runs were executed at `610e9b6e`, after the fix (`74317395`) and generated shortcut-reference correction. This final documentation update records their results; no production/test/evidence code changes follow the tested revision.

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_BUILD_JOBS=4
export RAYON_NUM_THREADS=4
export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/B5-49"
unset TESSERA_REGENERATE_KEYBOARD_RESULTS
# Repeated twice, each time with an empty status before and after:
git status --porcelain
(cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh)
git status --porcelain
```

The runner captured `git status --porcelain` to a file before and after each run and required all four files to be zero bytes. Each gate used the full required command above (including binding regeneration). No files were restored, cleaned, staged or committed between the two runs.

| Run | XCTest | Swift Testing | Gate result | Status before | Status after |
| --- | --- | --- | --- | --- | --- |
| 1 | 948 executed, 3 skipped, 0 failures | 5 passed | SWIFT GATE OK | 0 bytes | 0 bytes |
| 2 | 948 executed, 3 skipped, 0 failures | 5 passed | SWIFT GATE OK | 0 bytes | 0 bytes |

Thus each run passed 945 XCTest tests (3 skipped) plus 5 Swift Testing tests, and left the entire worktree clean, including generated bindings and the three tracked result artifacts.

The final strict product build also passed, exit 0, after both gates and against the rebuilt native archive:

```sh
cd apps/mac
swift build -c release --product Tessera -Xswiftc -strict-concurrency=complete -Xswiftc -warnings-as-errors
```

Stale-artifact handling: `cargo clean -p tessera-ffi --release` ran in the required target directory before final gates (0 files removed because this lane target was initially cold). A cached Blake3 native object with a newer deployment target was additionally cleared with `cargo clean -p blake3 --release` (22 files); the normal `build-ffi.sh` rebuilt it with its macOS 15 environment. The final strict log has no newer-deployment-target linker warning. No Rust source was changed.

The first full-gate attempt, before refreshing `docs/shortcuts.md`, had 948 XCTest tests (3 skipped) and exactly one failure: `ShortcutIntegrityTests.testMenuAndRouterDocumentationAndMenuCollisions` detected the stale generated KeyRouter excerpt. `python3 tools/orchestrate/shortcut-audit.py --write-doc` removed the old Tab-only claim. The audit then passed with 63 menu bindings, 11 routing sources and 16 reserved chords; both final full gates passed it too. No shortcut rule was relaxed.

All four B5-49b findings are done. See [the regenerated checklist](../B5-49/RESULTS.md) for the exact acceptance rows.

## Scope and acceptance boundaries

No Rust source, `Cargo.lock`, or `board.json` changes. No GUI activation, key/main-window promotion, system setting changes, or access to the prohibited user library locations. Hosted tests are background responder tests, not physical keyboard or visible focus-ring acceptance. Remaining native checks are 1b, 5, 6a, 8, 11, 16b, 17b, 19b, 21b and 22b; 22a is cleanup only. These pre-existing application/FKA boundaries are not claimed complete.
