# Native Develop close Stage B evidence — 2026-09-27

This directory is a portable copy of immutable source snapshots, SHA-256 manifests, exact commands, direct exit codes, and raw logs from the bounded native gate. Logs may contain vendor compiler warnings; the direct exit files and test result lines are authoritative. The product branch is `codex/develop-close-recovery-plan`; root owns integration. No Swift recovery, navigation, GUI, full-app, lease, or cross-process writer claim follows from these tests.

| Snapshot | Result |
| --- | --- |
| `red-39551980` | Initial fixture-selection mistake: all three tests failed before lifecycle assertions because a path comparison found no row. Preserved, not counted as behavioral RED. |
| `red-0a1a7462` | One successful-close test reached the intended worker-not-drained RED. Two failed-close fixtures missed the injected fault because it was keyed by the constructed photo path. Preserved, not counted as failed-close behavioral RED. |
| `red-6222f410` | Valid behavioral RED: all three tests compiled and failed where expected. Two failed-close tests found `State.closed=true`; successful close found the writer handle still present. Both direct invocations exited 101. |
| `gate-96aa223a` | Six lifecycle tests passed across five invocations: failed close 2, successful close 1, concurrent close 1, save-listener reentrancy 1, temporary histogram 1. Late-AI test failed only because the fixture consumed its last `Engine` Arc (`engine closed`). Adjacent/strict checks were not run on this snapshot. |
| `gate-f5a6b31e` | Late-AI 1/0; full Develop unit 41/0; Develop integration 9/0 with 3 ignored; masks integration 3/0 with 1 ignored; format exit 0. Strict Clippy exited 101 on one new `collapsible_if` in `set_mask_overlay`. |
| `gate-5478efe1` | Final source: late-AI 1/0, format exit 0, strict Clippy `--all-targets -D warnings` exit 0. The only source change since `f5a6b31e` was the formatting-equivalent collapse of that `if`. |

Final product HEAD: `5478efe11ee9197ee214d64c9ceb1dfc4dbc68fd`. Final `develop.rs` SHA-256: `2a4034d923e0d91030cdcc564f20f201e3a288af1e4d24da2fa70d500bb56081`; final `masks.rs` SHA-256: `2d8a9342abff5b0568b898326b500cd893bd83a1e40f61985b7124daf38a2614`. The source snapshots and manifest in each gate directory identify the bytes used by that run. The earlier valid RED source is preserved as `red-6222f410/develop.rs` and its manifest.

The final source makes a failed close retryable without marking the session closed, shares one in-flight close result with concurrent callers, joins the save worker after successful flush, rejects edits and flushes through retained Arcs once closing/closed, and rejects save-worker callbacks that would wait on or join themselves. A late AI mask completion first observed after close is ignored; an already in-flight callback may still finish after close. `Engine::depth_histogram` continues to open a temporary session while a visible editor exists. The Swift failure-safe close and navigation contract is a separate required gate.
