# Independent review: d908374f owned-captured-CFA qualification (retry-v2)

Verdict: PASS. No discrepancies found. This was a read-only review. Nothing was built or run; only git, shasum and python3 hashing were used.

## 1. Worktree and source freeze
- HEAD is d908374f. `git status --porcelain` is empty. The only ignored path is `fixtures/raw/`.
- Tree at d908374f has 9231 blobs, no symlinks and no submodules. Every working file matches its git blob (sha256 over `git cat-file --batch`).
- 280 files were cross-checked with `git show d908374f:path`: all 31 files under crates/raw-decode plus 250 random files. There were 0 mismatches.
- The 33 freeze maps (8 phase before.json files, 9+8 command inputs-before files, and their after counterparts, which are equal) each have 9231 entries. The key set equals the tree, and there are 0 hash mismatches, over 221,544 entries in total. before == after in every phase, and inputs-before == inputs-after in every command.
- Runner, oracle, API-oracle and fixture-manifest hashes match in every freeze. The 257-entry dependency_context matches artifacts.json in every command and still matches on disk now.

## 2. Phases (all exit 0, freeze.json equal:true, no oracle-error or launch-error)
- 02-green: all 5 required-tests.json names pass. Result: 5 passed, 0 failed, 0 ignored, 92 filtered.
- 03-api: pass.rs exits 0 with an empty log. The eight negative controls exit 1, and each has exactly one coded error with a primary span in its own fixture file:
  - clone, conversion and default: E0277
  - metadata_mutation and opcode_mutation: E0596
  - parts and sample_getter: E0599
  - private_fields: E0451, primary spans on lines 4-7
  - The only other diagnostic is a benign warning in private_fields.rs that `forge` is never used.
- 04-full: 98 passed, 0 failed, 4 ignored across unit 94/3, linear_dng 3/0, owned_api 1/1 and doctests 0. The 4 ignored tests are:
  - the 2 existing tests from 55a241ee: `actual_cfa_fixtures_return_owned_samples_after_original_copy_replacement` and `actual_native_success_boundaries_observe_cancellation_before_publication`
  - the 2 new family tests, which run in phases 05 and 06
  - `fixtures_decode` prints all five fixtures. The two "panicked" lines come from expected unwind tests that report ok. Cargo had nothing to rebuild (0.48s), and the binaries are hash-equal to the retained ones.
- 05 and 06: each run via the retained binary with --exact --ignored reports 1 passed, 0 failed, 0 ignored. Each of the five families is EXERCISED exactly once.
- Fixtures: the directory holds exactly the 5 files. Their bytes and sha256 match fixture-env.json, which matches the authority file. They are frozen and identical in every phase and command.
- 07-strict: clippy -D warnings passes. The only warnings are from the libraw-ffi C build script. 08-fmt: empty log.

## 3. Retained artifacts
- The unit binary (155f7145…) and the integration binary (a3dcc94c…) match artifacts.json. Both are mode 0755 (executable), the same as their sources in the target directory, and the sources' sha256 also matches. The engine_api and raw_decode rlibs match.

## 4. Runner diff
- RUNNER-DIFF.patch applied to ../run.py reproduces run.py byte for byte. The patch only changes these things:
  - adds `import stat`
  - records a launch OSError to launch-error.json with direct_exit null, then re-raises
  - starts `result` as None instead of 0
  - replaces copyfile with copy2 and asserts the mode and executable bits
- There are no oracle or test changes. PREPARATION-SHA256.json matches.

## 5. Diff 55a241ee..d908374f
- 18 files changed: 17 under crates/raw-decode (the capture.rs and capture/decode.rs changes, owned.rs, tests/owned.rs, the tests/decode.rs module hook, and tests/owned_api/*) plus docs/superpowers/plans/2026-09-28-owned-captured-cfa.md. Nothing else changed, including Cargo.toml and Cargo.lock.
- Worth knowing, but disclosed and made before this run: c0993358 changed the clone.rs oracle from E0599 to E0277 to match what the compiler actually reported.

## Original failed ../02-green
- Unchanged. All mtimes are 21:53:20 or earlier, before retry-v2 was created at 21:54. There is no exit file, run.log is empty, and oracle-error.json records the PermissionError. The original retained copies are still mode 0644. This was a launch failure, not a product failure.
