# M5-33 candidate items (collected; brief to be written after M5-31/M5-32 land)
- Layer Knocks Out Drop Shadow (Photoshop semantics) — shadow must not show through the empty interior at Fill 0 % (B5-07).
- PatchMatch / Remove cancellation granularity: Cancel must take effect within ~200 ms at any stage (B5-09v step 326: stuck on "Cancelling…" > 2 min on a large removal). Check caf.rs / remove.rs loops and the ONNX path.
- Anything M5-31 and M5-32 report as not done.
- Fonts (from B5-10b, see `git show origin/wp/B5-10b:tools/orchestrate/wp/B5-10b/NEEDS.md`): the PSD writer's composite/text pixels, the resident renderer's nested smart-object child renderers, and ConvertToPixels/rasterize_layer each build a Compositor with system fonts only, so a document using a snapshot-only font fails there. Thread the document's font database through every Compositor construction in crates/compositor.
