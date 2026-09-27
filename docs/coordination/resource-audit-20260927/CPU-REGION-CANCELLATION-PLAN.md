# CPU region cancellation API contract

Coordinator source review, 2026-09-27. This is a proposed next A-owned slice,
not implemented or validated evidence. B owns document render wiring.

`document/render.rs::cpu_present` currently iterates covering tiles using the
legacy `render_tile`, then copies them into an IOSurface. Replacing that with a
full-image RGBA call would inflate the request and is not the intended fix.

Add `Compositor::render_region(doc, level, region, cancel)` returning straight
planar full tiles in raster order, only the tiles intersecting the clipped region.
The half-open region is in pixels at the requested level (not level-zero pixels).
Clip signed coordinates to the level extent before converting to tile indices;
empty/outside regions return an empty vector after checking cancellation. Reject
levels above MAX_LEVEL consistently. Individual output tile extents stay equal
to their existing canvas-edge layout, not the cropped region. The caller copies
only its requested pixels as today.

Use a single per-call FilterPass for all requested tiles, with current admission
limits. Actual filtered traversal stays serial to preserve nested Rayon safety
and once-per-key unmasked/masked result retention, just as full-level rendering.
Reuse existing internal tile rendering with caller token; do not duplicate
smart-filter semantics or retain pass state in a public long-lived context.
A single-tile cancellable wrapper may delegate to the same bounded mechanism,
but the document viewport must use the region call so it does not create one
independent full-filter pass per tile.

Check cancellation before coordinate/output allocations, between tiles, before
and after straight-alpha conversion, and before returning success. Partial
completed cache entries may remain reusable; no partial output vector is returned
as success. This does not preempt every non-filter pixel loop or GPU submission.
Do not promise total working-memory admission from the retained-result limits.

Tests: clipped negative/outside/empty rectangles, partial edge tiles and nonzero
pyramid levels match existing render_tile pixels/order; off-region cold root
tiles are not rendered; two covered tiles with an oversized filter result execute
one stack within the pass; pre-cancel invokes no evaluator; deterministic gate
cancel during a stack stops subsequent work and returns Cancelled; the next fresh
request succeeds and active counters unwind. Keep nested Rayon regression tests.

B integration remains separately reviewed: active frame token owned under Signal,
request/stop cancellation independent of the backend render lock, identity-safe
cleanup, generation checks before presentation. Add checks during IOSurface copy
and before publication; an unpublished partially-written surface must never be
presented or reused as a completed frame. Readback takes the live request token;
GPU cancellation is boundary-only until a separate supported route exists.
