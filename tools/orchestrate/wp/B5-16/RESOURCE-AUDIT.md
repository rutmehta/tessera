# Tessera Machine B resource audit — 2026-09-27

## Immediate outcome and scope

The user's laptop became barely usable during the Transform self-test. Paused
`tessera-machine-b-coordinator`, froze the owned app12083 and runner9344, captured
their state, then killed only those owned test processes. Terminated orphaned
`yes`27454 after `lsof` established cwdB5-15/crates/tessera-ffi and stderr in the
old Claude coordinator's taskb93weupjl output. It had run over8hours at91%CPU.
No other user app was closed. Dirty B5-16a gain-map snapshot remains untouched.

A local `~/.local/state/tessera-resource-hold.json` now blocks this self-test
runner before any app launch. Nine lightweight runner tests pass, including the
hold. **No further builds, benchmarks, app tests or automatic queue advancement
on B until a bounded resource plan is established.** A was notified through Git
mailbox7cf0f7e2. No main merge. The interrupted Transform run is not accepted.

Audited product72d8756 and current local B source713de4a; checked the key duplicate
cache and soft GPU budget behavior also exists in fetched origin/main550e09a.
This is a live incident capture plus source audit of scheduling, transform,
CPU caches, GPU resident allocations, viewport/lifetimes, observation and test
orchestration. It is **not** a completed allocation-lifetime/Metal Instruments
study. No new stress load was used to investigate the user's overloaded laptop.

## Measurements and attribution

| Observation | Evidence / limit |
| --- | --- |
| Tessera986%CPU before stop | `ps`, approximately ten CPU cores; not GPU utilization. Rasterized PSD copy was the current self-test step. |
| Tessera7.0GiB footprint, peak7.5GiB | `transform-vmmap.txt`; about6.9GiB writable data swapped out, default malloc ~2.9GiB allocated and3.6GiB owned-unmapped memory. Unmapped ownership cannot be assigned to Metal solely from this report. |
| Rayon workers in full-image stack evaluation | `transform-sample.txt`: save_psd_rasterizing_transforms → render_level → smart/filtered_source → evaluate_transform/TransformOp/seam. Frozen stack captured after SIGSTOP; identifies concurrent call stacks, not a statistical CPU profile. |
| Main thread in AppKit event loop | Same frozen capture; no evidence of a main-thread busy loop at capture time. Does not exclude earlier UI stalls. |
| Host ~43GiB swap; high wired/compressed memory | System totals, not all Tessera. After termination, host still had Resolve~5.7GiB, WindowServer~4.9GiB, Lightroom~3.1GiB plus many other apps. Do not kill those or attribute their memory to Tessera. |
| CPU recovered to48–54%idle in subsequent snapshots | `host-after-stop.txt`; whole host still busy, not proof all responsiveness issues are solved. |
| GPU after Tessera termination | IOAccelerator reported42%device usage,28.53GB allocated-system-memory counter and4.41GB in-use-system-memory counter. Global counters with different meanings, not a Tessera leak measurement. |

## Findings, ordered by urgency

### P1: duplicate full-image work on concurrent tile cache misses

`crates/compositor/src/render/smart_filters.rs:288–415` explicitly allows duplicate
cold computations to avoid a Rayon re-entrancy deadlock. `render/mod.rs:691–723`
feeds tile requests through `par_iter`. Every miss can render the complete child
and run every transform again. Only final publication deduplicates the result.
A mutex or per-key blocking wait added naively would reintroduce the documented
deadlock; this needs orchestration above parallel tile work or a re-entrant-safe
shared evaluation design. The frozen live stacks support this path's involvement.

The `filter_evaluations` metric explicitly excludes discarded duplicates
(lines279–285), so it understates actual work. Count attempted computations,
duplicate discard, temporary bytes, and peak simultaneous evaluations separately.

### P1: cache limits omit working memory and create a size cliff

`smart_filters.rs:199–240` allocates full planar RGBA input, transformed output and
another Raster, with additional old/next stage data at347–375. Cache bytes at390
count only source+result at32bytes/pixel. Temporary allocations, protection masks,
parameter serialization and concurrent duplicates are outside this budget.
At399–411, oversized entries are never retained; overflow clears the entire map.
Repeated tile requests can therefore rerun oversized stacks indefinitely across
the frame. `filters.rs:590` constructs a64MiB compositor for rasterized PSD copies:
32bytes/pixel reaches that budget at2,097,152pixels; a20MP source+result needs640MB
before temporary memory. The captured1.6MP input is below this individual cutoff,
so the cutoff alone is not established as the cause of this specific incident.

`render/mod.rs:117–122` gives separate live, filter and tile caches the same
nominal budget; it is not a total per-document/process memory cap. Nested caches
and active evaluation memory need one admission controller, not larger constants.

### P1: expensive transforms lack useful in-flight cancellation

`transform/seam.rs:152–159` removes seams one at a time; each `find_seam` at242
recomputes a full energy field plus cost/parent arrays, followed by image copying.
The work grows with pixels times removed seams. No cancellation token is passed
into this loop or `evaluate_transform`. `render_level_rgba` creates a fresh token
at `render/mod.rs:729`; checking between tiles does not interrupt a long stack.
`save_psd_rasterizing_transforms` at `document/transform.rs:1093` exposes neither
progress nor cancellation. Swift uses a detached task, which avoids synchronous
main-thread work but does not bound CPU or working memory. Cancellation must
reach the inner kernels, with progress and checks between seams/stages.

### P1: GPU budget is soft and multiplied across renderers

`resident/mod.rs:529–536` defaults to2GiB per renderer. `ensure` at1015–1053 first
tries eviction then explicitly grows beyond the budget. Nested smart children at
827 receive independent default renderers; stack buffers, page pools, CPU fallback
and child renderers have separate accounting. `resident/filters.rs:302` likewise
creates children with the parent's budget. This is a confirmed policy weakness,
not proof that these GPU allocations caused the captured CPU-heavy incident.
Use a shared hard admission budget with controlled fallback/eviction; count
in-flight command-buffer references and transient buffers as well as cache bytes.

### P2: frontend timer lifecycle leak and unnecessary hidden work

`DocumentViewport.swift:512–533` installs a repeating30Hz marching-ants timer.
Its callback captures the view weakly, preventing a strong view-retain cycle, but
the run loop retains the timer and the nil-self branch never invalidates it.
There is no teardown invalidation or hidden/detached-window policy in this class.
Thus losing a view with an active selection can leave a repeating timer behind.
This is a source-proven timer-lifetime issue; its contribution to host load is
unmeasured and cannot explain986%CPU by itself. Add explicit ownership cleanup,
stop on detach/occlusion and restart only when visible with an active selection.

### P2: synchronous model refresh amplifies engine contention

`DocumentController.swift:70–94` synchronously reloads layer/info/history data on
MainActor. Listener callbacks at478–479 schedule a full refresh per notification,
without coalescing. Engine locks under heavy transform work can make UI refresh
wait; repeated updates can rebuild outline/history unnecessarily. The captured
main thread was idle, so this is an audit risk, not a proven cause of this stall.
Use dirty categories, bounded coalescing and immutable snapshots; measure input
latency on a small fixture before increasing workload.

### P1: orchestration protected correctness, not host responsiveness

One heavy process still used~ten cores and7GiB. Runner deadlines900–1800seconds
are far too long as a responsiveness guard; they have no CPU/footprint/pressure
budget. No Rayon/OpenMP ceiling was set. Transform enables its20MP stress section
by default (`TransformSelfTest.swift:554`), and the CPU-intensive PSD copy is
unbounded until the outer deadline. The orphaned `yes` process proves stress
process cleanup was incomplete. Background/nonactivating does not mean low impact.

Required changes: opt-in stress mode, small smoke fixtures by default,2-worker
ceiling for laptop checks, low priority, wall/CPU-time limits, process-group/child
ownership cleanup and a memory-pressure abort. Report aborted/skipped stress as
unverified; never convert it to an acceptance pass. No test runs during a hold.

## Existing protections that should be retained

- Tile cache has a byte-bounded LRU. ThumbnailLoader has explicit cost-bounded
  caches, request cancellation and queue limits; no blanket claim of unbounded
  frontend image caching is supported.
- Document viewport retains a three-IOSurface ring and replaces/detaches it on
  resize; it avoids repeated identical viewport requests. These are useful
  controls, though they do not prove every surface is released on all lifecycles.
- Document listener uses a weak controller; viewport is weak. No clear strong
  cycle was found there in this pass.
- The strict runner caught incomplete tests instead of trusting zero-failure
  summaries. Keep that behavior; add resource controls rather than weakening it.

## Verification still required, without overloading B

1. A owns a bounded compositor regression: tiny image, parallel cold requests,
   count true evaluation attempts and peak active allocations. Include nested
   stacks and a cache smaller than the result; avoid blocking re-entrancy fixes.
2. Add cancellation and memory-admission unit tests. Validate at2workers on A or
   another idle host; no B benchmark needed. Benchmark only after small tests pass.
3. Add a frontend timer teardown regression and repeated open/close snapshot
   check. Then use allocation lifetimes/Metal resource capture on a small fixture
   to distinguish leaks from caches/driver-retained allocations.
4. Only after resource protections exist: short, bounded B responsiveness check
   with explicit headroom; stop on pressure. The heartbeat remains paused.

No claim that all memory leaks are found or fixed. Full runtime GPU/leak closure
is pending these bounded checks. This audit establishes real defects and a safe
sequence to resolve them, rather than rerunning the workload that froze the host.

## Source-only follow-up: A result fbe21798 (2026-09-27)

Reviewed A product `0e58792d` and evidence `c10b4eea`, with coordination main
`bc9a8cc`. No B build, test, app launch or heartbeat restart. The three incident
PIDs are absent and the local resource-hold file remains present. This update
changes documentation only.

### A mitigation review

The pass propagates through nested source rendering, reserves source/result bytes
before cold evaluation, and releases failed reservations through RAII. Persistent
cache hits are pinned in the same pass. Top-level filtered frame tiles execute
serially without holding the pass/cache mutex through nested evaluator calls.
This addresses the reported within-frame oversized-result recomputation path
without adding a blocking per-key wait. A reports six integration and one
reservation tests; B inspected their evidence, did not independently execute them.
The recorded RED-source archive gap and explicit 1/2/4-worker deadlock tests are
retained limitations. This is not approval for main integration or incident closure.

**Residual whole-image mask work:** at A `smart_filters.rs:656–689`, every
`filtered_source` call clones the cached unmasked raster then calls
`edit_region(Rect::of_extent(...))` when a filter mask is enabled.
`Raster::render_region` (`raster.rs:379–421`) materializes replacement tiles for
the entire region before `edit_region` installs them. Thus pass-cache reuse does
not prevent repeated full-image mask blending/allocation for successive output
tiles. The filter-stage attempt counter does not count this work. This behavior
predates A's patch; it is a remaining resource hotspot, not a new correctness
regression. A owns the fix/design: count masked pixels/allocations in a tiny
multi-tile case; preserve distinct masks on objects sharing the unmasked key.
Do not cache final masked results by the unmasked key alone.

Direct tile calls, simultaneous independent frames, transient allocations, GPU
residency and inner transform cancellation remain outside this pass bound, as A
already reports. Default 1 GiB/256 entries is per pass, not a host admission cap.

### Additional B frontend findings

1. **Second orphan-timer path (P2):** `Tools/ToolOverlayView.swift:19–36` has the
   same repeating timer/nil-weak-self issue as `MarchingAntsView`. Its animation
   predicate reads global outline/gesture state, and `draw` calls `updateTimer`
   before checking whether the viewport owns the active document (lines65–67).
   A stale overlay can therefore animate because another document has a selection.
   Neither implementation stops for detached/hidden/occluded windows. These are
   source-proven lifecycle defects, not a measured explanation for ten busy cores.
   Regression requirements: remove view with active selection, release last view
   reference, switch document ownership, hide/unhide, occlude/unocclude; require
   zero recurring ticks while inactive and one timer when active again.

2. **Obsolete outline computation is not coalesced (P2):**
   `Tools/DocumentTools.swift:682–709` enqueues every changed epoch/level on a
   serial dispatch queue. The generation check occurs only after
   `selectionOutline` returns, on the main queue. Newest-result-wins protects UI
   correctness but does not skip obsolete queued engine calls or bound queue
   length. Closures retain the backend until executed. Use one running request
   plus one replaceable newest pending request; invalidation must also handle
   selection clear/document close. A deterministic fake backend blocked on its
   first request should prove a burst executes at most first plus latest. No
   queue growth measurement or live leak claim is made.

3. **Viewport destruction lacks explicit backend release (P2 risk):**
   `DocumentViewportRepresentable` defines make/update but no dismantle hook.
   `attach(nil)` performs detachment, yet no dismantle path invokes it. Removing
   a SwiftUI viewport while its document remains open can leave the backend's
   surface ring registered until document switch/close/replacement. The Rust
   `detach_surfaces` clears that registry (`document.rs:2056`); normal
   `DocumentController.close` already calls it and then closes the session, so
   this is specifically retained-open-document view removal, not every close.
   Add teardown with ownership guards: an obsolete view must not clear a newer
   view's callback/surface registration. A two-view replacement regression must
   accompany a simple dismantle regression. Runtime surface retention remains
   unmeasured.

The existing model/history synchronous refresh concern remains an unmeasured
contention risk. Coalescing those notifications requires event-order tests; it
should not be changed blindly alongside the timer repair. This review introduces
no product changes under the current source-review-only request.

## Timer repair candidate — request bac04a31

Validated exact chat target, expiry and local hold, then published accepted receipt
before editing. Source-only candidate adds `DocumentAnimatedOverlayView`, shared
by MarchingAntsView and ToolOverlayView. It reconciles a single 30 Hz timer on
window moves, hide/unhide and window occlusion notifications, rechecks visibility
and eligibility before each tick, and invalidates a run-loop-retained timer on
its first callback after owner release (without drawing). Selector-based window
observation does not retain the view; registration is replaced on window changes.
The tool overlay additionally requires both the active document and that
document's current viewport, preventing a superseded view from animating.

Five deterministic XCTest source cases use unscheduled timers/manual `fire()`:
identity-preserving repeated update and one restart; eligibility/visibility loss
before a tick; weak owner release; hide/unhide/detach hooks; detached concrete
overlays. **ALL FIVE UNRUN; source not compiled.** No RED/GREEN claim. The user
explicitly requested unrun tests and forbade B execution; source review and
`git diff --check` are the only local verification.

A validation plan: review the exact candidate diff first; on an available A slot,
compile with two workers and run only DocumentOverlayAnimationTests under a
short outer timeout, preserving failures. Tests need no image fixtures, Metal
viewport creation, real timer waits or application window activation. They test
the shared lifecycle with fake visibility plus detached real overlay classes;
real AppKit occlusion notification delivery and visible document switching remain
separate, bounded follow-up checks on A. This is not runtime leak closure.
No outline coalescing, backend surface teardown, engine mask edits or main merge
is included. B hold/paused heartbeat and dirty B5-16a snapshot remain unchanged.

### Timer Swift 6 compilation correction — ae3c2b6a

A's first compile of exact4797b2a failed before any tests: the callback's
non-Sendable Timer parameter was captured by MainActor.assumeIsolated at line32.
Raw evidence is preserved on main6544a1d6 under
`tools/orchestrate/wp/RES-B-TIMERS/evidence/2026-09-27/first-compile/`.
Validated the follow-up's exact B target/expiry and accepted before editing.

Moved weak-self resolution and nil-owner `timer.invalidate()` into the original
Timer callback, outside assumeIsolated. Only the live MainActor view crosses
into the synchronous actor assertion; the Timer parameter does not. Scheduling
remains on the main run loop, and deterministic manual test firing remains on
MainActor. No detached task, unchecked Sendable wrapper, or concurrency-check
suppression was added. Existing five regression cases are unchanged and UNRUN
on B, including the released-owner case that exercises this branch.

Local verification: source diff review and git diff --check only. Compilation
and all test outcomes for the correction remain pending A validation. The
original failed compile is retained, not replaced by a claimed pass.

## Outline backlog source candidate — user continuation

User explicitly requested continued work while the workload hold stays. Added
`LatestRequestBuffer<Value>` with one running slot and one replaceable pending
value. DocumentTools submits only returned requests to the serial engine queue,
and completion starts only the newest pending request. Clear/switch/close
invalidate publication and drop pending references without freeing the running
slot prematurely. Refresh ignores inactive documents; workspace attachment
refreshes the current document. Already-running engine work is not cancelled.

Four deterministic source tests simulate a blocked first request: 100-request
burst collapses to first+latest; clear does not start a concurrent call; close
rejects late output and drops pending work; duplicate completion cannot release
another running slot. All UNRUN; compilation pending A. These cover scheduling
state, not live backend/UI wiring. A should additionally validate selection
clear, document switching and close with a blocked fake backend before accepting
the integration. No B build/test/app/heartbeat restart.

## Viewport teardown source candidate — user continuation

Added SwiftUI dismantle calling explicit viewport detach/workspace release.
Only a document's current viewport can clear its frame callback, saved viewport
state and backend surface registration. Attaching a replacement first detaches
the prior viewport, so late SwiftUI teardown is harmless. Existing ring/current
texture cleanup in attach(nil) is retained. No backend implementation changed.

Three UNRUN XCTest source cases cover owner detach/idempotence, stale owner
cleanup preserving a newer callback, and replacement-before-dismantle ordering.
These use no windows/fixtures but viewport initialization may create a Metal
device/pipeline on A. They verify ownership/callback lifecycle, not measured
IOSurface reclamation. Compilation, test execution and actual surface-lifetime
validation remain pending A; no B loads or heartbeat restart.
