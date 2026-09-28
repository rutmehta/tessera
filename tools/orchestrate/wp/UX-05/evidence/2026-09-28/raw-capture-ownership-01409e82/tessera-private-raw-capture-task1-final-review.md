# Private RAW capture Task1 final ownership review

Reviewed exact immutable01409e822b19a45f68e81a8eb16f0ef4fbc3f166 in codex/private-raw-capture. Source-only independent review; no build, mutation, GUI or GPU work. Evidence root: /Volumes/betterSSD/tessera-validation/private-raw-capture/task1. Machine-readable independent verification: /tmp/tessera-private-raw-capture-task1-final-review.json.

## Verdict and scope

No actionable Task1 ownership defect found. Approve this as an intermediate private-stage ownership checkpoint, not a completed RAW capture feature. Public capture intentionally returns Unsupported and cannot construct CapturedRaw; byte copying, suffix admission, source regular-file/nonblocking admission, read bounds, digest/sealing, expected identity and consumer/decoder integration remain Task2/3. Do not merge/advertise as completed captured-stream or render support.

## Ownership and error paths

- Reservations charge full max_asset_bytes and one slot under one mutex with checked addition. All filesystem calls occur after unlocking. Allocation failure drops the still-active reservation, releasing both counters exactly once. Concurrent admission tests hold the winning stage through observation; no read/byte-copy guarantee is implied yet.
- StageGuard holds Reservation→Arc<PoolInner>, retaining directory and accounting beyond external pool handles. Reservation release marks itself inactive; subsequent Drop is harmless. Explicit close takes the file before cleanup, so StageGuard Drop cannot repeat that deletion.
- NamedTempFile cleanup is disabled immediately after creation. into_temp_path closes writable ownership and preserves the disabled TempPath flag; verified this against installed tempfile implementation. Successful unlink or NotFound releases accounting. Failed unlink moves the still-owned path and numeric byte charge into quarantine, disarms reservation WITHOUT decrement, and leaves the slot charged. Quarantine has no Reservation/Arc backreference, hence no pool cycle.
- Stage Drop uses the same cleanup path and best-effort diagnostic on failure. Explicit close returns its primary cleanup error. Pool setup failure preserves the setup error and reports secondary directory cleanup failure. No panic-on-stderr-write, recursive delete, unbounded retry, or hidden TempPath cleanup path found.
- PoolInner Drop occurs only when all external pools/live reservations are gone. get_mut accesses accounting without holding a mutex guard, so injected cleanup and diagnostic operations execute outside locked accounting. It tries each quarantined path once and remove_dir once. Both TempPath and TempDir automatic cleanup stay disabled even after failure. Residual files at process teardown are intentionally possible and diagnosed, not silently claimed removed.
- Private callback seams are cfg(test), local to the particular pool, and replace filesystem operations only. No public hook, arbitrary stage-path getter, writable escape or Clone on CapturedRaw was introduced. Allocation OOM/unwinding of arbitrary test callbacks is not a claimed recoverable storage protocol.

## Requested missing coverage now present

All four preparatory gaps are covered: actual Drop unlink failure retains quota/reports; persistent unlink plus nonempty-directory sentinel survives bounded nonrecursive teardown; barrier-controlled two-contender single-slot admission; initial setup failure with cleanup-success and cleanup-failure branches. Existing tests preserve separate byte/slot bounds, NotFound, owner-retained pool lifetime, stage-create failure and Unix0700/0600 modes. These are meaningful assertions on real accounting/staging, not a fake quota implementation.

The private allocate_stage currently accepts suffix unchecked and has a temporary dead_code allowance. Because it has no public reachable caller yet, that is an explicit Task2 obligation rather than an exposed path-admission bug. Task2 must validate before calling this same guard; must not fork a separate untested ownership path. Public CapturedRaw shape will need read-only sealed storage when those bytes exist.

## Independent evidence checks

Recomputed all **8,202** final07 tracked/new source SHA-256 values against immutable Git01409e82: zero mismatches. Final07 before/after source map and HEAD match. Runs02–07 all have unchanged per-run source and fixture maps. Final bytes equal full04, strict05 and fmt06 inputs exactly; initial focused03 differs in the two capture source files, so its success is historical and final07 provides the final-byte rerun including all12 ownership tests.

Raw logs/direct exits: compile01 is preserved failure; behavioral02 exit101/0passed12failed against the Unsupported scaffold (not12 independent mutation demonstrations); focused03 exit0/12passed; full04 and final07 exit0/27unit+3integration+0doctests/0ignored. Strict05 and fmt06 direct0. The final full suite contains the12 ownership cases. No new runtime was executed by this reviewer.

Existing raw-decode regression tests read five existing RAW fixtures. Preserve the author's precise limit: only an after04 baseline exists for those five and matches after07; there is no independently established before04 preservation claim. This primitive does not decode those fixtures. Existing vendored native compiler warnings are retained; strict Rust success is not a claim of zero native warnings.

Task2/3 source and bounded tests still require independent review and an explicit later runtime lane. No workload or decoder expansion is authorized by this report.
