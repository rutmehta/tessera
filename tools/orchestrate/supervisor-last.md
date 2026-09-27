Next wave: finish export and expose the engine features already built, rather than add more engine surface.

Live main is 2b1f361, newer than the supplied snapshot. Its STATUS reserves B5-06+ for Machine B.

Coordinator first
- Don’t remerge B5-ui, M5-28 or M5-29. They’re already merged. Machine B must update to current main/engine-api 1.4.
- Continue M2-45’s existing worktree. Its round-2 PASS covers items 1–4 only, not HDR, gain maps or post-processing. Audit those separately.
- Require Opus review of public API changes; reconcile Cargo.lock, engine-api, FFI lib.rs and generated bindings centrally after export merges.
- Fix stale STATUS entries and reserve these IDs with Machine B. Preserve the dirty supervisor-last.md and unrelated files.

Command convention: WT is /Users/rutmehta/Developer/tessera/.worktrees/<ID>. Each worker uses its own external CARGO_TARGET_DIR. Swift gate: cd "$WT/apps/mac" && ./build-ffi.sh && swift test && swift build. Generated bindings are build outputs, not independently edited source.

1. M2-45 | GPT-6 Astra | Machine A
Continue export completeness: AVIF/JXL decode-and-tag verification, LibRaw DNG round trip, watermark placement, size-limit convergence, metadata policy and output sharpening. Report remaining HDR/post-action work explicitly. Allowed paths: existing M2-45 brief’s exact allow-list, including export, export FFI, CLI, MCP export adapter/tests and dependency files. Gate: cd "$WT" && cargo test -p export -p tessera-ffi -p tessera-cli --release && cargo clippy -p export -p tessera-ffi -p tessera-cli --all-targets -- -D warnings && cargo fmt --check && cargo deny check licenses, then Swift gate. No dependency; sole export-engine writer.

2. B5-06 | Opus 5.5 | Machine B
Fix B5-v before expanding UI: Cmd-E on a selected DNG must open an engine-backed layered document; document-mode Cmd-E must still merge down. Make Save As filename entry targetable and prove save/reopen. Replace the stale test-count assertion with named coverage. Allowed: apps/mac/Sources/Tessera/**, Sources/TesseraCore/Document/** and Tests/TesseraCoreTests/**, narrowed to traced routing/sheet files in the launch brief. Gate: Swift gate with new routing/save regression tests; Sol rechecks steps 140/144. Depends on current main; exclusive document-shell writer.

3. B5-07 | Opus 5.5 | Machine B
Expose the 13 M5-26 adjustment layers plus HDR Toning through menus, properties and the real engine backend. Require parameter dispatch, visible changes, one history entry per completed edit, undo/redo and persistence. Allowed: apps/mac/Sources/Tessera/Document/**, Sources/TesseraCore/Document/** and Tests/TesseraCoreTests/**; coordinator handles outside menu registration. Gate: Swift gate including new adjustment coverage. Depends on B5-06; no Rust/schema changes.

4. B5-08 | Opus 5.5 | Machine B
Expose M5-29’s Camera Raw, Liquify, Content-Aware Fill/Move, Remove/Distractions and three neural filters. Validate layer/mask selection, distinguish destructive versus smart-filter operations, and show missing-weight errors rather than silent no-ops. Allowed: the same document paths as B5-07. Gate: Swift gate with CPU PatchMatch, missing-model, undo and persistence tests. Depends on B5-07; serialize because controllers/backends overlap. Escalate missing APIs rather than editing Rust opportunistically.

5. M2-46 | Opus 5.5 | Machine A
Expose only verified M2-45 export capabilities in the dialog, with backward-compatible presets and unsupported combinations disabled. Do not advertise unfinished HDR/gain-map/post-action support. Allowed: apps/mac/Sources/Tessera/Export/**, Sources/TesseraCore/Export/** and Tests/TesseraCoreTests/Export*Tests.swift. Gate: Swift gate with settings-to-FFI, preset round-trip and real small-file export tests. Depends on M2-45 merge/review; can parallelize with document UI within this allow-list.

6. B5-v2 | GPT-6 Sol | Machine A
Recheck the two concrete document failures first, then all V/W steps and the new adjustment/filter/export acceptance cases on the merged real-engine app. Require screenshots, exact build commit and per-step verdicts; unexecuted steps are not passes. Allowed writes: tools/orchestrate/wp/B5-v2/** and isolated temporary fixture/app data, no source. Gate: cd /Users/rutmehta/Developer/tessera && /Users/rutmehta/Developer/tessera/tools/orchestrate/verify-sol.sh B5-v2, after coordinator builds the app and supplies acceptance.md. Depends on chosen UI merges; one screen owner, no concurrent app rebuilds.

Defer Channels, advanced transforms and Auto-Align/Blend/Photomerge UI to the following wave. They share too much document-controller surface to add safely here.

Saved plan with full gates and conflict rules:
/Users/rutmehta/Developer/tessera/.hermes/plans/2026-09-26_194537-next-wave.md
