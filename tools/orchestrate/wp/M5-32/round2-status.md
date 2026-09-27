# M5-32 Round 2: blocked by excluded FFI integration

Implemented the newly permitted ColorLookup rest pattern in resident/program.rs
and updated the two existing ICC test patterns. These are compilation fixes only,
not GPU dither/neutralize implementation. No forbidden files were changed.

The exact requested chained gate was run twice with the inherited external
CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-32.
The latest run exited 101 during release compilation:

```
error[E0004]: non-exhaustive patterns: `&compositor::channels::ChannelKind::AlphaDisplay { .. }` not covered
 --> crates/tessera-ffi/src/document/channels.rs:275:52
```

Full output: round2-gate.log. Later stages of the && gate were not reached.

Focused real-source verification passed:
- m532_adjustments: 7 passed
- m532_channels: 3 passed
- m5_26_icc: 4 passed

Full output: round2-focused.log. git diff --check passed.

The explicit prohibition on crates/tessera-ffi/src/document/** prevents repairing
the exhaustive channel match. Photo Restoration's required existing raster enum
and dispatcher are also in document/filters.rs, with neural bridging in
 document/retouch.rs. Owner integration or expanded permission is required.
The stroke integration in document/tools.rs also needs review for the new target.
Do not duplicate or reroute these modules through document.rs to evade ownership.

All five items are NOT complete. The outstanding functional work in STATUS.md
remains, including GPU semantics, PSD lookup metadata, bounded Remove and its
benchmark, restoration integration, and complete contract documentation.

No commit was made. The pre-existing brief.md modification was preserved.
No board transition was possible: kanban_show reported that no task ID was set.
