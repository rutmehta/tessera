# B5-41 — Camera Raw preview submission coalescing

Branch: `wp/B5-41`. Base: `445131fc`. Local commits only.

- `51c38253` — `test(B5-41): expose unbounded Camera Raw preview submissions`
- `b70bee72` — `fix(B5-41): coalesce Camera Raw previews with latest request buffer`
- The following `docs(B5-41):` commit contains this handoff and gate evidence.

## Implementation

The Camera Raw sheet now uses the existing `LatestRequestBuffer<String>` for canvas submissions, following Adaptive Wide Angle's generation/finish pattern. One request owns the submission slot; each subsequent tick replaces the pending JSON. Both drag and final callbacks use this path. An unchanged release leaves the exact last draft pending. The full-resolution apply still reads the current draft directly.

The Rust preview API enqueues rendering and returns; it does not wait for the rendered frame. The sheet retains the submission slot for a 120 ms interval after enqueue, using the prior debounce interval as pacing. This provides progressive first/latest previews during a continuous drag and prevents the fast enqueue API from submitting every UI tick. The latest pending value submits at the next interval, including after release. The engine's existing queue still supersedes older rendered frames.

Submission and `filterPreviewLevel` stay together on MainActor so the viewport query and Before/Cancel/OK remain ordered. The production path updates the level immediately when it submits, preserving the detail note during long drags. Superseded asynchronous completion metadata/errors are rejected. Before, Cancel and OK invalidate the buffer and cancel its task. No detached submission can race after these operations. The existing 1:1 detail buffer and B5-18b/B5-34 `detailPreviewNote` policy remain intact.

An optional async submission closure is the test seam. With it unset, the sheet calls the existing backend preview and level APIs. No Rust, backend protocol, dependency, or generic buffer changes were required.

## Before / after measurements

The engine test synthesizes a filled 6000 × 4000 grey document (24 MP), with a level-2 viewport, and wraps the real backend submit/level calls. Each burst contains 60 changing ticks plus a same-value mouse-up callback. Final exposure is exactly **+3.0 EV** in every case. Counts below are Swift-to-backend submissions, not completed GPU renders.

| Workload | Before submits | After submits | Before elapsed | After elapsed |
| --- | ---: | ---: | ---: | ---: |
| Injected slow backend, 60 final callbacks | 60 | 2 | 1.267 s | 0.655 s |
| Real 24 MP engine, 60 final callbacks | 60 | 2 | 0.618 s | 0.656 s |
| Real 24 MP engine, 60 drag callbacks + release | 1 | 2 | 0.644 s | 0.631 s |

Elapsed measurements include the test's 600 ms settling allowance and scheduling overhead; they are not render latency or FPS benchmarks. Real-engine input-loop times were 6.39 ms before / 16.17 ms after for final callbacks, and 10.08 ms before / 6.30 ms after for drag callbacks. No latency improvement is claimed from these single runs.

The baseline's ordinary drag path was a trailing debounce, so an uninterrupted burst already submitted only once after input stopped. Final callbacks bypassed it. The new drag behavior deliberately submits first plus latest rather than claiming a reduction from that one delayed preview. A further test delivers 60 Amount ticks across separate run-loop turns (1 ms spacing), with a slow first submission: at most three submits, stale error discarded, and the final serialized Amount equals **60.25%** exactly.

The failing-test commit was executed before the fix: the slow test recorded 60 submits and accepted the stale first result (level 7 rather than final level 2); Cancel accepted a late error; the real-engine final-callback burst submitted 60 times. The fix rejects the stale result/error and passes all four added regressions.

Evidence:

- [Failing slow-backend test](evidence/before.log)
- [Failing real-engine count and cancellation tests](evidence/before-engine.log)
- [Final Camera Raw tests and timed measurements](evidence/after.log)

## Gates

Commands used with `PATH="$HOME/.cargo/bin:$PATH"` and `CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-41`:

```sh
cd apps/mac && ./build-ffi.sh && cd ../.. && tools/orchestrate/swift-gate.sh
```

**SWIFT GATE OK** — 895 XCTest tests, 3 skipped, 0 failures; 5 Swift Testing tests passed. No window-capture failure. [Gate output](evidence/swift-gate.log), [FFI build output](evidence/ffi-final.log).

Final focused run using the gate's built binary: **26 Camera Raw tests passed**, including the existing smart-stack detail-note and full-resolution fallback tests. `git diff --check` passed. Rust test/clippy/fmt gates do not apply because no Rust files changed.

No GUI launch or manual screen capture was performed. No Lightroom catalog was opened. `board.json` and `Cargo.lock` are unchanged. All edits and local commits are confined to this worktree; this agent's builds ran serially. The change was not pushed or integrated into main.

## Outcome: DECLINED (Machine A review, 2026-10-01)

Not merged. The baseline drag path already coalesced a continuous drag to one submit via the trailing 120 ms debounce, and release submitted immediately. This branch's first-plus-latest behaviour gives two submits per drag and makes release wait for the in-flight slot (up to 120 ms): a responsiveness regression for no benefit. The "60 final callbacks → 60 submits" measurement is synthetic: in `ValueSlider` only `mouseUp` and the numeric-field commit send `final: true` (once per gesture); drags, arrow/Home/End nudges and option/shift-modified nudges all send `final: false` and go through the debounce. No real input path submitted on every tick. Branch kept as reference only.
