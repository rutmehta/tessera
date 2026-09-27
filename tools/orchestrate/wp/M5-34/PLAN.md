# M5-34 implementation plan

Goal: bound CPU live editing cost by changed visible geometry, retaining exact vector coverage.
Spec: brief.md. Execute inline in the existing worktree; no commits.

- [x] Add regression for distant tile reuse after text/shape edits; run red.
- [x] Prepare positioned geometry once per model/transform; cache shaped paragraph/line work and glyph outlines. Bound cache memory and clear font-dependent data when fonts change.
- [x] Cache sparse per-glyph coverage and layer tiles by intersecting geometry, preserving raster arithmetic and tile coordinate system.
- [x] Derive downstream tile identities from intersecting live geometry, retaining conservative revision keys for styles, masks, groups and nonlocal effects. Compute old/new changed geometry bounds for partial updates.
- [x] Test edits, deletion, movement, wrap, Unicode, undo, font replacement, styles, transforms and cache eviction against cold rendering.
- [ ] Run ignored 20MP photo/274px/43-edit CPU frame benchmark at L2 and L3 for typing and shape handles; iterate on measured bottlenecks.
- [x] Run required release tests, clippy and formatting; document measured results and integration limits. Parent's exact gate passes (314 passed, 0 failed, 11 ignored); frame-latency acceptance remains pending an idle host (NEEDS.md).
- [ ] Replace conservative styled-layer full invalidation with halo-bounded damage.
