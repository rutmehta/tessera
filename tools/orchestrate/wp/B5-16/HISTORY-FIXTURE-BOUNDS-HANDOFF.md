# History fixture bounds correction — source only

Request a9f170bf-b8dc-4e36-bdf4-14d3b4b87755; separate branch codex/b5-16-history-fixture-bounds from frozen b7687116. Test correction 1fed7383. All tests UNRUN on B; A owns compilation, serialized runtime/GUI gates and main integration.

A evidence main56dd677e, tools/orchestrate/wp/B5-16/evidence/2026-09-28/history-cd0b850d-red, records 66 tests / 3 assertions failing only the actual-inspector preference fixture. The other five History tests and keyboard tests passed there. Earlier 65-test/22-assertion evidence remains preserved. Neither run is broad acceptance.

Only DocumentHistoryHeightControlTests.swift changes. After attaching NSHostingController, the fixture now sets window content size and hosting view frame to 288x848, lays out, retains the existing 0.25-second event-loop settle unchanged, then lays out again. Both initial and recreated hosts assert positive and exact bounds plus exact window content bounds. Increase must be enabled before its native action. No extra delay or polling, no production/KeyRouter changes, no full-shell singleton attachment.

Existing exact preference increment, recreated readout, reset, containment, native callback semantics and global owner/preference/activation restoration assertions remain. Test failures remain failures; bounds are asserted after settling rather than patched after the check. Production control and DocumentView hashes are unchanged; companion manifest records them and the test source.

A acceptance: rerun original combined 66-test filter/order on the composed candidate and keyboard-alone against the same binary; retain exact logs/exit/hash identity. Then remaining layout/full/strict gates and actual external AX/keyboard GUI: reachable labelled Increase/Decrease/Reset controls, current value, focus traversal/activation, clamp behavior, persisted recreation, pointer drag/double-click and footer containment. B has performed source diff review and git diff --check only. No build, test, app, GPU, benchmark or heartbeat run.
