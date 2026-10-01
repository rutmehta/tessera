# ENG-2 proposed RED patch rationale

Artifact: `eng2-publication-red-a187355e.patch`, source freeze `a187355eade82a76927c49bcd14471f42b38e25f`. Adds only a private macOS unit-test module at the end of `crates/tessera-ffi/src/document.rs`; no public/FFI test hooks, no product implementation. This patch is not applied and has not been compiled or executed. Source inspection predicts RED; it is not observed RED. Follow the parent coordinator's exclusive runtime handoff before applying/running.

## Blocking test

`published_session_readers_do_not_wait_for_writer_state` establishes a real session with a completed layer rename, named history snapshot and selected layer, waits idle, then directly holds `Shared.state`. It starts each reader independently, so `info` blocking does not hide whether other readers are also blocked. Completion must arrive before the controlling thread releases the writer guard. There is no synthetic new publication API and no mutation midway through a critical section. Readers have a fully established committed state available for the future publication implementation.

The included reader closures are core session info/layers/layer/history/snapshot names/history memory/surface planning/test-state getter, channel records/names/selection outline, smart-filter/style/global-light/vector-mask/transform metadata, advanced-transform status, text/shape wrong-kind error paths, and Export Flat admission. These paths are currently State-backed: document.rs:1350-1415,1926-1996,2005-2022,2221-2226,2430-2431; channels.rs:311+ via channel_state; tools.rs:2042+,2099; filters.rs:3625; styles.rs:172,195,213; vector.rs:605,612; transform.rs:662,1168; text.rs:540. Export validates that the private handle contains the completed layer rename; it never calls run or writes a file.

The broad reader inventory is intentionally a stronger writer-independent publication target than a viewport-backend-only compatibility check. The parent may split auxiliary getters into a second test if rollout scope is initially core HUD/snapshot only, but should explicitly document omissions rather than assert all session getters are covered. Actual record construction can be proportional to layers/history; this test checks lock independence, not microsecond runtime for arbitrary document sizes.

Excluded: pixel-producing thumbnails, sample_color, read_level/read_presented_level, filter_detail and analysis operations intentionally perform pixel/backend work; they are not lightweight status getters. Filter error/remove bounds/style clipboard use other state stores and need their own lock audit if included in the product latency promise. Successful text/shape-specific records, transform_stage, hit-testing and filter-preview-level require additional fixture-specific cases for complete API coverage; current wrong-kind cases only exercise acquisition/error behavior, not successful projection fidelity.

## Cleanup and deadlock reasoning

The test uses one sync_channel whose capacity equals the exact worker count. Each worker sends exactly once, so even every result arriving late cannot block a sender on queue capacity. The sender held by the controller is dropped after spawning. The receiving loop uses one absolute five-second deadline, not five seconds per getter, and breaks on timeout/disconnection.

The controller executes **drop(held) before join and before assertions**. Baseline getter workers then acquire State, finish, send their late results into the sufficiently large queue, and join; only afterwards does the test close the session and fail with the complete list of readers that missed the held-lock window. A worker panic is detected through join; no expect/assert in the held-lock receiving loop can bypass explicit cleanup. Standard unscoped JoinHandles are deliberate: using thread::scope around workers while the outer scope retains State can join during unwinding before the guard is released, deadlocking the expected RED.

This guarantees release and normal baseline cleanup for the known State-lock blockage, not a universal kill switch for arbitrary future deadlocks inside a getter. Rust JoinHandle has no timed join. An unrelated permanent internal deadlock after State release still requires the test runner's process timeout; claiming this harness can guarantee bounded completion for all regressions would be false. The five-second receive deadline is a scheduling watchdog, not a measured performance acceptance threshold. Normal uncongested execution after implementation should complete immediately.

## Confirm-time compatibility test

`confirm_snapshot_excludes_later_commits_and_preserves_live_scratch` captures an export with an uncommitted 0.25 opacity draft, then commits and changes both name and opacity to later values, closes the session, and inspects the export handle's private retained Arc. It must retain the earlier name and 0.25 opacity. This is expected to pass at the baseline and guards against a new committed-only publication silently changing the existing live snapshot contract (`document.rs:2208-2226`). No export pixel render is required to prove Arc state isolation here; end-to-end file/encoding verification belongs to the separate export tests.

## Distinguish from backend compatibility

The existing normal viewport releases State before filtering/backend rendering (`document/render.rs:1030-1035,1063-1067`). Thus a test holding only the backend mutex should already pass for these State-only readers and cannot establish the new independent publication RED. This patch deliberately holds writer State, not the backend. Keep backend compatibility tests as a separate regression lane and do not cite their baseline pass as proof the broader ENG-2 publication request is implemented.

No product implementation should start until the targeted held-writer test has actually been observed failing for lock blockage, rather than compilation, fixture, unrelated runtime error, or test-runner timeout.
