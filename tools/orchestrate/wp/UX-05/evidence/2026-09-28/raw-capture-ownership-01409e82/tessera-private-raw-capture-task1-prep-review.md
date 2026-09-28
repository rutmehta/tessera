# Private RAW capture Task1 preparatory review

Reviewed source-only against base838c8e496bf4cb5bcecf74801e7402bedababfcf and approved docs/superpowers/plans/2026-09-28-private-raw-capture.md. No builds, tests, CUA, source mutations or runtime claim. Source hashes below bind this review.

## Verdict

Suitable preparatory interface and meaningful initial tests; no objection to the planned compile-only failure observation when root grants the lane. This is NOT Task1 implementation acceptance: public bodies remain todo and private test seams intentionally absent. Missing-symbol compilation must be reported as compile RED, followed later by meaningful behavioral RED after minimal seam wiring. Do not implement product merely to satisfy this review before that sequence.

Public interface matches approved route/identity/limits policy. CapturedRaw has no Clone, path getter, writable handle or generic consumer escape. The module explicitly separates a frozen stream from atomic source snapshot and decoded/render-ready ownership. Cargo/lib changes only wire approved dependencies/module; original decoder behavior is untouched. No false decoder/render or process-memory claim found.

The eight initial tests exercise real future stage ownership: invalid limits before directory creation; separate byte and slot limits; successful close and normal Drop release; cancellation admission; creation failure release; holding pool directory beyond pool-handle drop; NotFound cleanup release; failed explicit close quarantine and one teardown retry; Unix privacy modes. Tests deliberately bypass byte copying using private allocate_stage; they do not yet establish read-bound, digest, sealing or consumer guarantees. Those belong Task2/3.

## Remaining Task1 contract gaps

1. **Drop failure path is not tested.** The injected unlink failure currently enters via stage.close(), where an error can be returned. Add a parallel actual drop(stage) failure contract: capacity remains charged, file remains, one removal attempt, diagnostic captured without panic, next admission exhausted, and final pool teardown gets only the bounded retry. Normal successful Drop coverage cannot catch an implementation that mishandles only destructor errors.
2. **Persistent teardown failure / nonrecursive cleanup is untested.** Existing cleanup injection succeeds on its second call. Add persistent unlink failure and directory-removal failure controls with a private test diagnostic sink. Assert attempt bounds and reported errors, retaining the stage when unlink remains denied. An unrelated sentinel inside the private directory is also a useful explicit remove_dir-not-remove_dir_all control. This covers disabled TempPath/TempDir cleanup rather than relying only on the success-path count.
3. **Concurrent reservation admission is untested.** Sequential held owners prove capacity arithmetic but not atomic competing admission. Use bounded barrier-controlled contenders sharing one pool authority; hold the winning owner until both admission outcomes are observed. Assert exactly the configured winners, ResourceExhausted losers, conservative charge and full release. Avoid sleeps or large allocation.
4. **Pool setup failure cleanup is not covered.** The stage-creation failure test removes the already-created pool directory and proves reservation release; it does not exercise initial private-directory permission/setup failure and secondary cleanup error reporting. A private per-call failing setup operation can prove explicit single cleanup/diagnostic ownership before PoolInner transfer. Keep this distinct from Task2 read/write/sync failures.

These are test-first additions or explicitly remaining Task1 obligations, not evidence of present production bugs in the unimplemented scaffold. No need to broaden this slice to Task2 streaming or Task3 consumer behavior yet. Invalid suffix, source growth, checked streaming, completed-stage digest and read-only sealing remain required later; allocate_stage tests alone cannot establish them. Ensure the eventual public capture uses this same reservation/guard implementation, not a second untested path.

No public callback/diagnostic API, cross-process lock, crash scavenger or original-path fallback should be introduced by these tests. The test-created directory removed/recreated in stage_creation_failure is a controlled fault injection and must not be portrayed as hostile-namespace safety.

## Source hashes

- `crates/raw-decode/Cargo.toml`: `c39bf89a490a737951d3829ef9229b3f3b01045bd470a5a348b68b6d1aa01093`
- `crates/raw-decode/src/lib.rs`: `aec54c6fdf1dc8fbf44d7ae9b4b4dab566279659fb09b584b6f6b2615802a835`
- `crates/raw-decode/src/capture.rs`: `2fdb79a3ca88b44d171452e863925262f19101f2301514d98817a79f17ea3c12`
- `crates/raw-decode/src/capture/tests.rs`: `3ab47e8a22a8255cfd556d0345e903897cd53eb045984e2fb63888920882f295`
