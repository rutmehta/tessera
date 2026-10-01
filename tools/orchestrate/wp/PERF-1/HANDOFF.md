# PERF-1 pending handoff

Date: 2026-10-01. Branch: `codex/perf-1-styles`. Investigated base: `ebae08bb`. Owner: Codex Machine A; Claude retains all merges. Machine B retains app-side export spans/routing.

Status: **observed RED at ad4b7165: 4 passed, 2 expected source-count failures; implementation not started**. See [RED-RESULT.md](RED-RESULT.md), which supersedes historical UNRUN statements below. See [SOURCE-PLAN.md](SOURCE-PLAN.md), especially the final coordinator-reviewed scope correction, which supersedes optional alternatives earlier in that document.

Confirmed source finding: output-tile style emission repeatedly constructs full-canvas source/effect planes. The report's 14MP / 83s measurement is loaded diagnostic evidence, not a controlled baseline or a measurement taken by this lane.

Context review: see [CONTEXT-REVIEW.md](CONTEXT-REVIEW.md). Live text/shape dispatch reaches emit_styles through render_live_scene and must forward the future pass; current pixel/group tests do not prove styled-text export coverage. Overbudget negative-key bookkeeping and subtree admission suppression are optional policy choices, not pixel-correctness prerequisites. Counters measure calls, not successful completions.

Selected scope: full-level, frame-local bounded source/effect reuse, serial styled traversal, recursive-safe cache access without locks across source rendering, actual allocated/padded tile payload accounting, and serial uncached overbudget fallback preserving accepted inputs. Preserve exact DocRef namespaces and verify nested same-ID document separation. Direct tile calls and app-side paths are unchanged in initial scope.

Evidence and gates:

- Authored `render::effects::perf1_tests::full_level_evaluates_each_styled_source_once`: six-tile synthetic raster, drop shadow plus glow, separate per-compositor cfg(test) `source_raster_build_calls` and `style_render_calls` counters, recorded at source-raster entry and immediately before style rendering. Both counts must equal one; optimizing only one operation cannot satisfy the test. Expected to fail the once-per-frame invariant on current source, but **UNRUN**; no observed RED evidence.
- Authored an independent analytic exact-pixel overlay fixture across six tiles. **UNRUN**; not a frozen blur/morphology oracle. Instrumentation is cfg(test) only and introduces no public API.
- Added **UNRUN** nested isolated-parent/styled-child overlay oracle and sequential A/B/A document-context regression. Both documents intentionally share layer IDs and snapshot revision but differ in source alpha geometry and child overlay color; expected pixels are independently calculated. Independent source review found the fixture APIs and analytic pixel oracle consistent; see [TEST-SOURCE-REVIEW.md](TEST-SOURCE-REVIEW.md). Both remain UNRUN. The A/B/A case covers cross-call top-level document isolation in a persistent compositor; it does not exercise two child DocRef namespaces within one frame-local pass. Same-pass nested smart-object collision source is now authored separately below; execution remains pending.
- Added **UNRUN** `same_pass_smart_children_keep_identical_ids_in_distinct_contexts`: one full-level render of two identity-transformed smart objects containing nested styled groups. Child namespace keys differ while group/leaf IDs, property/content revisions and state revisions match; alpha geometry and overlay colors differ. Every output channel is checked bitwise against an independent opaque-overlay oracle across two output tiles. This test has not yet received independent source review or compilation. It covers same-pass namespace separation, not budget/cancellation behavior or a measured cache hit.
- Added **UNRUN** `full_level_live_shape_styles_use_one_source_and_exact_overlay_pixels`: a font-independent opaque vector rectangle covering a 257x3 canvas, hidden source fill, and constant normal overlay. Asserts the live/style dispatch predicates, rasterized live-source evidence, independent bit-exact RGBA for every output pixel, and one source build plus one style render across both output tiles. This requires future pass forwarding through render_live_scene; it has not been compiled, executed or independently reviewed. No styled-text or blur performance result is implied.
- No runtime code implemented.
- No benchmark authored or run; no before/after timing or pixel-parity result.
- The six-tile fixture, APIs and independent analytic overlay oracle were independently source-reviewed; this is not compiler or runtime evidence. All authored tests remain **UNRUN**.
- Directly changed Rust files were formatted with `rustfmt --config skip_children=true` and `git diff --check` passed. No builds, tests, clippy, Swift gates, GUI or catalog access performed for this handoff.
- Compiler **HOLD** persists while external batch17 Rust/GPU pipeline (parent PID72750) owns the lane. No build/test processes may start until the coordinator releases it.

Next action: after lane release, compile and execute the authored PERF-1 tests, record exact RED/fixture evidence, then author the frozen blur/morphology oracle, cancellation and budget cases and ignored release benchmark before implementing. Runtime cache structural changes require parent review. Required GREEN and performance evidence remain pending. The earlier source-plan commit was documentation only. This test-scaffolding commit is not an executed RED or lane completion claim. No cache, scheduling, or numerical optimization has been implemented.

Commit identity: use `git log -1 --format=%H -- tools/orchestrate/wp/PERF-1` on this branch; parent receives the exact resulting commit hash with this handoff. No merge requested.

Independent review of same-pass fixture 11c318d3 found no source blocker in namespace construction, identity sampling, bottom-first order or analytic dyadic pixels. See SAME-PASS-SOURCE-REVIEW.md. This is a pixel regression, not cache hit/miss evidence; still UNRUN.

Independent review of live-shape fixture fc680629 found no source blocker; LIVE-SHAPE-SOURCE-REVIEW.md distinguishes live_tiles route guard from the source/style operation counters. Still UNRUN. External batch16 finished and main advanced to b1af2436, but batch17 parent72750 now owns runtime. Runtime-slot request ec6a6548-1ecb-430d-a9fe-512d9b2d0efd is published; receipt unverified.
