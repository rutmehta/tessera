# PERF-1 context review and source confirmation

2026-10-01. Reviewed Luna's source-only note `/Volumes/betterSSD/tmp/perf1-context-review.md` against this branch after test-only commit `21940dde`. No build/test execution. This assessment refines SOURCE-PLAN.md; it does not authorize implementation before observed RED.

## Confirmed scope and identities

Create any future StylePass once in the full-level premultiplied render route shared by full-level export helpers, and carry it through DocRef. Styled full-level traversal should be serial, with direct-tile and region-only entry points remaining uncached in the initial scope. Use exact `(DocRef.key, DocState.rev, layer.id)` context identity; do not collapse distinct nested document namespaces to the parent.

`render/effects.rs::emit_styles` creates a fresh isolated source namespace. On a future successful pinned entry miss, allocate that namespace once, then keep it fixed throughout source_raster's tile traversal. Nested child entries can then reuse their results within that source context. `render/mod.rs::smart_tile` preserves `so.key` for smart-object child documents. Different nested documents can legitimately share layer IDs/revisions; runtime namespaces must distinguish them. No full-canvas lookup may use only a tile-local revision or the layer's property revision.

Do not hold a pass mutex while building source_raster, rendering nested styles, or running style kernels. Reserve/lookup/publish under short locks, release reservations on every error/cancellation, and publish completed entries only after checking cancellation. Serial top-level traversal avoids duplicate cold builds without a per-key wait primitive. All of this remains a design proposal pending RED and implementation review.

## Live-scene question resolved from source

Live text/shape CAN coexist with styles. The reported 14MP fixture itself is styled text; excluding this combination would miss the reported workload.

- `render/mod.rs::render_tile_premultiplied_in_pass` dispatches live scenes to `render_live_scene` before the plain has_styles branch.
- `text_vector/damage.rs:234-248` constructs a new DocRef inside render_live_scene, carrying the current filter pass and cancellation token. A future StylePass must be forwarded here too; passing it only into composite_premult would not cover live text/shape exports.
- `text_vector/damage.rs:308,323` runs the common `job.compile()` / `job.run()` path. `render/exec.rs:186-187,226-227` dispatches styled layers to emit_styles. Thus live dispatch does NOT inherently bypass style computation or reject styled text: it reaches the same eager style path through a different DocRef construction.
- `text_vector/damage.rs:77` excludes styled scenes from warm_live_viewport's scheduling shortcut; this does not disable live style rendering.
- `render/exec.rs:22-35` separately rejects styles in the neighbourhood CPU fallback. That fallback is not the normal full-level live-scene compilation path and remains outside this lane's claims.

Required implementation follow-up: preserve all existing live damage/source behavior while forwarding the same pass through render_live_scene; add a deterministic live-shape fixture (or pinned-font text) before claiming improvement on the actual styled-text fixture. The existing authored pixel-raster/group tests do not cover that routing.

## Admission/fallback review: adopt bounds, avoid unnecessary requirements

Retained source+plane entries remain capped per pass at 1 GiB / 256 entries, counting actual allocated/padded tile payload with checked arithmetic and in-flight reservations. This is not a bound on scratch, other compositor caches, RGBA output, GPU, RSS, or concurrent passes.

Keep serial uncached rendering on admission failure; do not introduce ResourceExhausted for formerly valid inputs. An overbudget source necessarily remains recomputed per output tile in this minimal fallback. A rejected-key set avoids only repeated admission bookkeeping, NOT repeated expensive rendering. Therefore the review's suggestion to "avoid retrying this same miss per output tile" must not be represented as once-per-frame source reuse for overbudget inputs. A negative set would also require a metadata bound and stable context identity; fresh isolated fallback namespaces can otherwise accumulate keys. It is optional, not a correctness prerequisite.

The review proposes allowing existing hits but suppressing new retained entries in an uncached fallback subtree. This is a reasonable optional policy to avoid useless child retention under short-lived isolated namespaces, but it is NOT required for correctness if every child has a distinct exact context key and the same checked pass-wide reservation accounting. Independent child admission cannot exceed the shared cap when accounting is correct. Before adding suppression plumbing, weigh its complexity against the deterministic benefit; document the selected policy and test its accounting. Do not adopt an extra rejection rule merely because the parent entry was too large.

## Instrumentation and test status

The authored counters intentionally measure operation CALLS: source_raster_build_calls at source-raster entry and style_render_calls immediately before styles::render. Their names do not claim successful completion. For the successful synthetic fixture, asserting each equals one protects against repeated source builds even if style results alone are cached, and vice versa. Moving counters exclusively after successful completion is unnecessary for this work invariant and would hide failed attempts. Future failure/cancellation tests should separately assert reservation cleanup and absence of published partial results, rather than interpret call counts as completed work.

Authored but UNRUN: six-tile source/style work invariant, exact analytic overlay fixture, nested styled isolated-group/child analytic oracle, and sequential A/B/A contexts sharing persisted layer IDs and snapshot revision but differing source alpha geometry and overlay color. Same-pass sibling smart objects with equal child IDs, live-scene routing, frozen blur/morphology oracle, budget boundaries/overflow/fallback, and cancellation/error cleanup are still pending. No runtime cache implementation, timing, RED execution, or GREEN evidence exists for PERF-1.
