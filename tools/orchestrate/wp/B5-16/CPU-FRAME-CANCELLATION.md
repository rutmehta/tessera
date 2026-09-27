# CPU viewport frame cancellation source candidate

Request aa0a9692-0d9a-493b-a82a-faef576c1970 validated for exact B chat target,
expiry and local hold; accepted receipt published before work. Integrated A
maina1d51f6 into B at04930f0 (source merge only) for its tested render_region API.
One product file changes: crates/tessera-ffi/src/document/render.rs.

## Ownership and publication

Signal owns an Arc<CancellationToken> per selected frame. Selecting a frame
installs it under the Signal lock, then releases the lock before evaluation.
Renderer::request cancels the active token and schedules the next frame;
notify_layers only queues layer notifications. Renderer::stop cancels through
Signal without needing the backend mutex held during rendering. Completion
compares Arc identity before removing the active owner and rejects cancelled,
stopped or superseded work. Frame results and render-failure callbacks are
suppressed when rejected; layer/history notifications retain their existing path.

The final publication decision is serialized under Signal; listener calls run
unlocked to permit reentrant host requests. A request that occurs after that
accepted publication decision schedules a later frame; it does not retroactively
revoke the accepted callback. Existing document/surface generation checks remain
inside present_frame. Cancellation is checked before work, around backend access,
and before updating frame metadata, then rechecked by the final owner gate.

## Region rendering and surface reuse

cpu_present now calls render_region(doc, level, src, caller_token), preserving
region clipping and one shared FilterPass instead of individual direct-tile calls.
The same token is checked per tile and per copied row. If a copy fails/cancels,
the locked target surface is cleared before unlock and no completed frame is
published. Its unpublished ring slot can be recycled; a later successful copy
writes every pixel in the newly reported src rectangle. A pre-cancelled request
returns before touching the surface. Cancellation after a completed copy still
fails the owner gate, so complete-but-obsolete bytes are not announced as a frame.
No extra full-frame staging allocation is introduced.

GPU rendering has boundary checks only, including after its wait. This does not
preempt submitted GPU commands. Likewise a worker already waiting for the backend
mutex may wait for its current holder, but requesting cancellation itself never
needs that mutex. Public synchronous read_level, thumbnail/composite readback,
PSD-copy handles, destructive apply and engine-api are unchanged.

## Five UNRUN deterministic tests

`frame_cancellation_tests` adds:

1. Supersession cancels the old source; old completion cannot clear a new owner;
   only one publication decision is accepted per request.
2. Actual notify_layers leaves an active source uncancelled; actual stop cancels
   it and completion is rejected.
3. A worker calls actual Renderer::request while the test holds backend.lock;
   a bounded channel rendezvous proves cancellation does not wait on that lock.
4. A pre-cancelled 2x2 CPU region leaves sentinel surface bytes untouched and
   cannot pass the publication gate.
5. A deterministic row-check callback cancels after the first copied row. The
   aborted surface is all zero and cannot publish; a fresh 2x2 cropped region
   then overwrites it with expected pixels and is accepted.

No sleeps or external fixtures. The last two cases require macOS IOSurface but
no window, app launch or GPU renderer. Tests cover production Signal/CPU helpers;
full live worker/listener/surface consumer scheduling remains an integration gate.

ALL FIVE TESTS UNRUN, SOURCE UNCOMPILED on B. Existing rustfmt and git diff --check
passed; these are source checks only. A validation: two build/Rayon workers,
focused lib-test filter frame_cancellation_tests, outer process timeout, then
existing viewport/ring-generation tests and strict lint in A's available slot.
Preserve failures. No main merge or B workload/heartbeat restart implied.
