# M3-14b current verification

Re-executed the exact required command chain with the inherited CARGO_TARGET_DIR=/Users/rutmehta/.cache/tessera-target/M3-14b.

- Release tests: 178 passed, 0 failed, 11 ignored.
- Clippy with --all-targets -- -D warnings: passed.
- Workspace cargo fmt --check: failed only on apps/tessera-cli/src/main.rs:10, requiring mod tether before mod understanding. This file is unchanged from HEAD and outside the allowed paths. It was not edited.
- Scoped cargo fmt for the four requested crates: passed.
- git diff --check: passed.
- No changes outside the allowlist.

Explicit five-fixture benchmark rerun passed (current-bench.log), asserting exact histogram and clipping counts against CPU counting of full-resolution rendered output, luminance error below 1e-4 on that same output, and full-resolution crop parity. The separately decoded CPU baseline is not pixel-identical on CR3/RAF; its reported luminance errors were 0.002665 and 0.005219 respectively, distinct from the same-output reduction parity assertion.

| Fixture | Median cached reduction | Amortized complete step |
| --- | ---: | ---: |
| CR3 | 3.763166 ms | 165.097930 ms |
| ARW | 10.575875 ms | 103.593527 ms |
| NEF | 11.541916 ms | 203.814347 ms |
| RAF | 6.333334 ms | 92.820583 ms |
| DNG | 6.435250 ms | 75.759486 ms |

The NEF cached reduction meets the 20 ms target, not the complete agent step. Existing implementation was preserved, with only this verification report and benchmark log added in this run. No task ID was provided through the Kanban environment, so a board transition was unavailable.

RESULT: FAIL mandatory workspace formatting fails on an unchanged, out-of-scope CLI file. Permission to format that file or an upstream correction is required.
