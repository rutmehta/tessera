# M5-32 retry verification

The retry ran the exact requested chained gate on the actual worktree, with
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32 exported.

Result: exit 101 during release test compilation, before any later gate stage.

```
error[E0027]: pattern does not mention fields `source_filename`, `dither`
 --> crates/compositor/src/resident/program.rs:493:13
493 | Adjustment::ColorLookup { size, data } => {
```

The inspected source still has this exhaustive pattern. This path is explicitly
excluded and assigned to M5-31. No temporary source overlay, disabled module,
or forbidden-file edit was used. Existing partial implementation was preserved.

Unblock requires owner integration of the new adjustment fields in resident
code (including GPU behavior, not merely ignoring fields), or explicit expanded
edit permission. STATUS.md records additional excluded FFI integration points
and remaining implementation work. M5-32 is not complete and is not ready to merge.

The kanban orientation call returned `task_id is required (or set
HERMES_KANBAN_TASK in the env)`. No board task was updated or marked complete.

## Latest re-run

The required chained gate was executed again on the actual worktree, explicitly
exporting the same external CARGO_TARGET_DIR. It again exited 101 at
resident/program.rs:493 with E0027 for source_filename and dither. Later stages
were not reached. No source changes or temporary overlays were made in this run.
The blocker remains an ownership/allowlist conflict, not the LibRaw warnings.
Owner integration or expanded permission is required before this package can
be completed and verified. The existing partial implementation is preserved.
