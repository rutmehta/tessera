# PERF-1 source candidate handoff

Date: 2026-10-01. Branch: `codex/perf-1-styles`. Implementation parent: `e32e6320`. Claude retains all merges; Machine B retains app-side export spans/routing.

Status: **implementation and new tests authored, UNRUN**. Runtime remains held by external batch18. No compilation, Cargo tests, GPU, GUI, benchmark, or numerical baseline capture was launched for this candidate. Direct rustfmt and `git diff --check` are the only executed checks. This is a source review candidate, not GREEN or a speedup claim.

## Preserved baseline evidence

[RED-RESULT.md](RED-RESULT.md) records the preimplementation run at `ad4b7165`: four analytic/context tests passed; source work-count assertions failed as intended (six builds for the raster fixture, two for the live shape, each expected one). Their later style-count assertions were not reached. Candidate assertions now read both counters as a tuple; no candidate counts are measured yet. The run overlapped newly started external compilation and supplies no timing evidence. The earlier report's 14MP / 83s result remains a loaded diagnostic, not a controlled baseline.

## Implemented source scope

Full-level CPU rendering creates one local StylePass when recursive document inspection finds styles and traverses output tiles serially. The pass is forwarded through live-shape/text dispatch, smart child namespaces and smart-filter source construction. Direct tile, region, GPU and resident callers pass no style cache; no app routing changes are included.

Entries use exact `(DocRef.key, state.rev, layer.id)` keys. An entry miss creates a unique isolated source namespace once for its full source traversal. Completed source/plane rasters retain distinct stable sample namespaces, including mip reads. There is no persistent style cache or cross-frame invalidation policy. Existing analytic overlay, A/B/A, nested isolated-group, same-pass smart children and live-shape fixtures are preserved.

A short mutex protects lookup/admission/publication only. The source/style build closure runs without that lock, including recursive styled children. Serial traversal avoids concurrent duplicate producers; this is not a generic parallel single-flight primitive. Errors and final cancellation drop reservation guards, and publication occurs only after the final cancellation check. Cancellation cannot preempt the current style kernel, which has no token API.

Limits are 1 GiB of accounted payload and 256 entries, shared across nested admitted entries. Before building, checked arithmetic reserves an estimate of dense F32 source plus enabled emitted planes; current raster builders use no-halo edge tiles. Publication reconciles every retained tile's actual backing Vec capacity via additive `Tile::allocated_byte_len`. Shared buffers are conservatively counted in each retained reference. Oversized actual capacity refuses publication and returns valid uncached output. Estimated in-flight charges are **not** a hard bound on transient allocator capacity growth. These limits exclude scratch, Arc/Vec metadata, allocator overhead, compositor caches, output tiles, GPU and other concurrent frames; they are not a total-RSS guarantee.

Overbudget/overflow/entry-limit admission falls back to serial uncached rendering and suppresses style admission in that fallback subtree. No ResourceExhausted error or input rejection is added. This policy avoids pinning children under short-lived fallback source keys; it is not required for pixel correctness.

## Authored UNRUN coverage

- Capacity accounting with spare Vec capacity and shared Tile clones.
- Exact-fit reuse, actual-capacity refusal, entry/byte limits, arithmetic overflow, dropped reservation cleanup, uncached admission and nested shared accounting.
- Deterministic build error and cancellation-at-final-publication cleanup plus retry; cancelled warm hits remain Cancelled. Nested build closures inspect accounting to exercise absence of a held cache mutex.
- Two-tile fractional-alpha fixture with a hole/edge, fractional shadow spread/size/offset and noninteger glow size. Every sample is compared bitwise between the cached path, serial uncached path and zero/tiny-budget fallback through the same private production frame traversal. This is reuse/fallback parity only, not an independent kernel oracle.
- Both existing work-count tests now observe `(source builds, style renders)` together, requiring `(1, 1)`.

## Remaining gates

Coordinator source review, compilation and focused tests are still required after runtime release. Capture selected frozen blur/morphology values from the **preimplementation** source, then assert portable tolerances against this candidate; do not bless candidate output as a baseline. Existing independent Gaussian impulse and morphology-band oracles in `styles.rs` remain unchanged and need rerunning. An ignored release benchmark with isolated baseline/candidate runs, golden validation outside timing, memory caveats and stable repetition is still pending. No performance acceptance, final integration or merge is requested by this handoff.

See [SOURCE-PLAN.md](SOURCE-PLAN.md), [CONTEXT-REVIEW.md](CONTEXT-REVIEW.md), and the existing source-review documents for prior rationale. This handoff supersedes their historical implementation-not-started / UNRUN baseline statements without changing the recorded baseline results.
