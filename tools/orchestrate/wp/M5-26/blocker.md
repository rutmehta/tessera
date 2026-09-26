# M5-26: historical round-1 blocker (superseded)

Round 2 implemented the pointwise adjustments and an explicit live CPU
neighborhood fallback. The required gate now passes. See `result.md` for the
current delivered scope, verification and remaining limitations. Everything
below is preserved historical context, not current status.

Status: blocked, not implemented. No production code was changed.

## Required scope change

Allow `crates/compositor/src/render/**` for neighborhood adjustment execution,
backdrop evaluation across tile boundaries, and radius-aware cache/damage handling.
The current allowlist excludes this directory. This is necessary for the requested
live Shadows/Highlights local-radius adjustment, not an optional refactor.

Evidence at the starting revision:

- `docs/02-photoshop-spec.md:146` requires an edge-aware bilateral base with
  amount/tone/radius controls for shadows and highlights.
- `crates/compositor/src/render/exec.rs:377-380` executes adjustments on the
  current tile accumulator.
- `crates/compositor/src/render/exec.rs:600-627` compiles the adjustment once,
  then calls `compiled.apply(unpremul(b))` for individual pixels. The call
  supplies no coordinates, neighboring pixels, backdrop sampler, or mip level.
- `crates/compositor/src/adjust.rs:277` accepts only a straight RGB triple.
  Changing code behind this API cannot recover the surrounding composite.
- `crates/compositor/src/render/mod.rs` owns tile construction, caching, and
  partial dirty-rectangle recomposition. A local operator needs neighboring
  backdrop samples and dependency-aware invalidation, not only a new formula.

Two documents with identical center RGB but different neighboring RGB must be
able to produce different center results for the same Shadows/Highlights
parameters. The existing adjustment evaluation interface cannot express that.
Baking a one-time result, ignoring radius, or using hidden global mutable state
would not satisfy the requested live adjustment behavior and CPU/GPU parity.

The pointwise subset can be implemented within the existing scope, but it would
not complete M5-26. No such partial implementation is claimed here.

## Other specification findings

- `Compiled::Luts` in `adjust.rs:190-192` is the internal one-dimensional
  Levels/Curves evaluator, not a Color Lookup adjustment or 3D LUT file loader.
- `SoCo` is already parsed as `FillKind::SolidColor` in
  `crates/psd/src/metadata.rs:308`; it must not be repurposed as an adjustment key.

## Verification performed

From `/Users/rutmehta/Developer/tessera/.worktrees/M5-26`, with
`CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-26`, executed:

```
cargo test -p compositor -p psd --release && cargo clippy -p compositor -p psd --all-targets -- -D warnings && cargo fmt --check
```

The command returned exit status 0 on the unchanged implementation. Existing
LibRaw C/C++ compiler warnings were emitted. This establishes only the baseline
gate, not completion or verification of any new adjustment.

Retry verification: inspected the current CPU call site and confirmed it still
calls `compiled.apply(unpremul(b))` without neighborhood context. Re-ran the exact
gate above with the external target directory explicitly exported; it returned
exit status 0. Full output is saved in `tools/orchestrate/wp/M5-26/gate.log`.
The allowlist remains unchanged, so the required scope expansion still blocks
completion. No production code was changed on this retry.

The board orientation call returned `task_id is required (or set
HERMES_KANBAN_TASK in the env)`, so no board card was updated and no task was
marked complete. This file records the blocker inside the authorized worktree.

Latest retry independently re-read spec §7, `Compiled::apply`, and
`render/exec.rs:600-628`. The neighborhood interface is still absent. Ran the
required three-command gate in this session with the external target directory;
it returned exit status 0 and refreshed `gate.log`. This is baseline verification
only, not an implementation pass. The same allowlist expansion is still needed.

RESULT: FAIL required neighborhood CPU rendering changes are outside allowed paths.
