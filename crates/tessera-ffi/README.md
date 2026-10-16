# tessera-ffi

The UniFFI surface the macOS app calls (`apps/mac`). Commands are narrow and
synchronous; Swift dispatches blocking work off its main actor.

## Locks (ST-1)

`Engine` guards the catalog (the index writer) with one lock, and each
`CullSession` guards its queue with its own. Both go through `lock::Traced`:

- A holder never keeps the lock across file I/O, rendering, ML, hashing or a
  folder scan of its own making. It snapshots what it needs under the lock,
  works outside it, then takes the lock again briefly to write results back.
  Sidecar reads and writes are serialized by the per-destination recipe-write
  gate (`recipe_write`), not by the catalog lock.
- In debug builds, or with `TESSERA_LOCK_TRACE=1` in the environment, a wait or
  a hold over 50 ms is reported on stderr with the calling line, e.g.
  `catalog lock: crates/tessera-ffi/src/lib.rs:502:30 held 183 ms`.
- Tests can assert on the longest hold (`Traced::stats`); see
  `drive::tests::flush_never_holds_the_catalog_lock_across_the_drive`.

### UI-safe methods

These wait at most 20 ms (`lock::UI_BUDGET`) for their lock and then fail with
`BridgeError::Busy` instead of blocking the caller. Nothing was done when
`Busy` is returned; the host keeps what it has and retries shortly
(`EngineBusy` in `TesseraCore` does this: `EngineBusy.waiting` off the main
actor, a delayed retry on it). Every other method may block for as long as a
worker holds the lock and must be called off the main actor.

| Object        | Method              |
| ------------- | ------------------- |
| `Engine`      | `roots`             |
| `Engine`      | `agent_provenance`  |
| `CullSession` | `images`            |
| `CullSession` | `groups`            |
| `CullSession` | `people`            |
| `CullSession` | `face_strip`        |
| `CullSession` | `assist_status`     |
| `CullSession` | `review`            |

`people(refresh: true)`, and `face_strip` or `people` on a queue whose people
have not been computed yet, still run the people job under the queue lock
once they have it; they are safe to call from the main actor only in the sense
that they never wait on another holder.
