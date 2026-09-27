# Machine A recovery status

Updated: 2026-09-27. Coordinator: Codex on Machine A.
Recovered Claude chat: `Multi-model execution plan (fork)`, session
`1ef5c604-ef13-4903-b9c9-757556764307`.

## Integration checkpoint

Local `main` was `8a80fe5` at takeover; published `main` was `4a4bd71`.
The local commits include the B5-16a adjustment round-trip fix, merge `ae6055a`,
and the raw-fixture preflight in `swift-gate.sh`.

The interrupted coordinator's saved gate log reported three Rust adjustment JSON
tests passing, 397 XCTest tests with one skipped and zero failures, five Swift
Testing tests passing, and `SWIFT GATE OK`.

Codex reran verification on the unchanged tracked source at `8a80fe5`:

- `cargo test -q -p tessera-ffi --release --test adjustment_json`: 3 passed,
  zero failures, exit 0.
- `bash tools/orchestrate/swift-gate.sh`: FFI build and Swift build passed;
  397 XCTest tests, one skipped, zero failures; five Swift Testing tests passed;
  `SWIFT GATE OK`, exit 0. XCTest duration: 99.623 seconds.
- Both commands used
  `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/main`.
- Generated bindings produced no tracked changes; `git diff --check` passed.
- This verifies the B5-16a integration, not the unfinished worktrees or all
  project acceptance criteria. Local gate log:
  `/tmp/tessera-codex-resume-main-gate.log`.

## Retained work and next steps

| Package | State recovered | Required next action |
| --- | --- | --- |
| M5-31 | `wp/M5-31` at `811fc7f`, seven tracked files modified plus untracked `resident_styles_large.rs`, round-2 work uncommitted | Preserve changes; investigate inherited long-running compositor test before taking worker ownership. Correctness gates were reported green, but cold resident timings of 110.561–253.233 ms fail the unchanged 100 ms target. Do not merge from the old top-level `pass` verdict. |
| M2-58 | `wp/M2-58` at `d8e5f43`, 16 tracked source/test files modified plus five untracked source/test files, runner escalated | Safely integrate current main into the existing work after preserving its diff and untracked files. Rerun strict gates with B5-16a; complete actual input-to-present, Auto-Upright, and detail-settle acceptance. Do not treat historical Document JSON failures as current or claim unmeasured presentation latency. |
| M2-45d | `wp/M2-45d` at `49d8e0f`, tracked tree clean, four commits ahead of its main base, runner escalated | Review the committed DNG, PQ/HLG, and native metadata slices and their final gates. Gain-map JPEG remains incomplete after failed ImageIO interoperability; do not call the whole package complete. |

No existing worktree was removed, reset, or cleaned at takeover. Main had no
tracked edits, but many untracked evidence files; these were preserved. The
board still labels these packages `running`; that is historical, not proof of a
live implementation worker. The inherited M5-31 runner was still waiting on
`photomerge_panorama` (PID 72566, parent cargo PID 72394), with over 11 hours
elapsed and zero sampled CPU. It was not terminated; confirm current ownership
and diagnose before replacing it or launching competing GPU measurements.

## Requests for Machine B

1. Acknowledge the Git coordination protocol and publish your status at
   `origin/codex/machine-b-coordination:docs/coordination/MACHINE-B.md`.
2. Report the exact local branch/commit and uncommitted state for B5-16 and any
   B5-15 work; neither branch was visible in the remote branch query at takeover.
3. Fetch current main before integration. B5-16a was already merged on Machine A;
   do not recreate its adjustment fix. Keep the remaining inspector/UI work on
   Machine B and list it in READY only after its gates and acceptance checks.
4. Preserve the existing ownership split. In particular, Machine A's M2-58 work
   touches Develop/Loupe and non-document FFI, and M5-31 touches compositor styles.
5. Report whether you can run the M5-31 resident-style performance acceptance on
   your M4 Max after Machine A publishes a coherent reviewed branch. No benchmark
   pass is claimed from prior mixed-load samples.

Direct chat messaging to Machine B is unavailable from this instance. This
status is an invitation to coordinate, not a claim that Machine B has received
or acknowledged it.
