# PERF-1 pending handoff

Date: 2026-10-01. Branch: `codex/perf-1-styles`. Investigated base: `ebae08bb`. Owner: Codex Machine A; Claude retains all merges. Machine B retains app-side export spans/routing.

Status: **minimal RED test scaffolding authored, UNRUN; implementation not started**. See [SOURCE-PLAN.md](SOURCE-PLAN.md), especially the final coordinator-reviewed scope correction, which supersedes optional alternatives earlier in that document.

Confirmed source finding: output-tile style emission repeatedly constructs full-canvas source/effect planes. The report's 14MP / 83s measurement is loaded diagnostic evidence, not a controlled baseline or a measurement taken by this lane.

Context review: see [CONTEXT-REVIEW.md](CONTEXT-REVIEW.md). Live text/shape dispatch reaches emit_styles through render_live_scene and must forward the future pass; current pixel/group tests do not prove styled-text export coverage. Overbudget negative-key bookkeeping and subtree admission suppression are optional policy choices, not pixel-correctness prerequisites. Counters measure calls, not successful completions.

Selected scope: full-level, frame-local bounded source/effect reuse, serial styled traversal, recursive-safe cache access without locks across source rendering, actual allocated/padded tile payload accounting, and serial uncached overbudget fallback preserving accepted inputs. Preserve exact DocRef namespaces and verify nested same-ID document separation. Direct tile calls and app-side paths are unchanged in initial scope.

Evidence and gates:

- Authored `render::effects::perf1_tests::full_level_evaluates_each_styled_source_once`: six-tile synthetic raster, drop shadow plus glow, separate per-compositor cfg(test) `source_raster_build_calls` and `style_render_calls` counters, recorded at source-raster entry and immediately before style rendering. Both counts must equal one; optimizing only one operation cannot satisfy the test. Expected to fail the once-per-frame invariant on current source, but **UNRUN**; no observed RED evidence.
- Authored an independent analytic exact-pixel overlay fixture across six tiles. **UNRUN**; not a frozen blur/morphology oracle. Instrumentation is cfg(test) only and introduces no public API.
- Added **UNRUN** nested isolated-parent/styled-child overlay oracle and sequential A/B/A document-context regression. Both documents intentionally share layer IDs and snapshot revision but differ in source alpha geometry and child overlay color; expected pixels are independently calculated. These new cases are not yet independently reviewed or executed.
- No runtime code implemented.
- No benchmark authored or run; no before/after timing or pixel-parity result.
- The six-tile fixture, APIs and independent analytic overlay oracle were independently source-reviewed; this is not compiler or runtime evidence. All authored tests remain **UNRUN**.
- Directly changed Rust files were formatted with `rustfmt --config skip_children=true` and `git diff --check` passed. No builds, tests, clippy, Swift gates, GUI or catalog access performed for this handoff.
- Compiler **HOLD** persists while external batch16 compiler/Clippy/Swift pipeline owns the lane. No build/test processes may start until the coordinator releases it.

Next action: after lane release, compile and execute the authored PERF-1 tests, record exact RED/fixture evidence, then author the frozen blur/morphology oracle, same-pass nested smart-object collision, cancellation and budget cases and ignored release benchmark before implementing. Runtime cache structural changes require parent review. Required GREEN and performance evidence remain pending. The earlier source-plan commit was documentation only. This test-scaffolding commit is not an executed RED or lane completion claim. No cache, scheduling, or numerical optimization has been implemented.

Commit identity: use `git log -1 --format=%H -- tools/orchestrate/wp/PERF-1` on this branch; parent receives the exact resulting commit hash with this handoff. No merge requested.
