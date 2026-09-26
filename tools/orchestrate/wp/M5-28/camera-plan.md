Camera Raw implementation plan

Use an image-core Renderer RGB linear entry point with persistent bounded f32 stage checkpoints. Source identity/revision and chained per-stage hashes own invalidation. Retain resolved optics with the lens checkpoint, use the reference's public resolved renderer with neutral controls to isolate private lens operations, then image-core StageOp for detail/tone/color/effects. Geometry uses the same resolved lens and reference composed map. No f16 cache reuse. The adapter shares a bounded renderer and derives identity from actual input samples, tile revisions and document context (the current FilterContext has no layer ID).

Tests first: scalar parity including nonneutral optics and signed HDR, exact warm/cold, downstream-only edits, revisions, budget; adapter parity/alpha/profile tests. Document every resident exclusion truthfully. Run targeted tests and clippy. No commits or dependency changes.
