# M2-17m merge reconciliation

Merge target: main at 5adacead1614734e554910dd3e17d510243d6a4b.
Original branch tip: 819d70d.

Intent of wp/M2-17: retain exact guided-filter moments, fewer dispatches and
transfers, fused creative stages, resident X-Trans, and full-precision checkpoints.
Intent of main: retain managed ICC/proof output including IOSurface presentation,
post-demosaic denoise, depth/lens blur, mask hooks and GPU geometry behavior.

All five conflict hunks are disjoint-intent combinations:

- image-core/src/render.rs, resident selection: retain supports_resident rather
  than duplicating Bayer-only selection. Its shared predicate now also excludes
  active denoise, preserving main's whole-image denoise barrier.
- image-core/src/resident_render.rs, capability predicate: combine main's active
  denoise exclusion with the branch's Bayer/X-Trans support and fused creative
  recipes. Keep can_render_resident and exact f32 cache checkpoints.
- pipeline-gpu/OPERATORS.md, introductory sections: keep both status descriptions.
  M2-17's f32 checkpoint description supersedes historical f16 cache descriptions.
- pipeline-gpu/src/batch.rs, parameter preparation: retain input_layout needed by
  managed output and fused parameters needed by M2-17. Also reconcile the semantic
  interaction outside the marker: fuse only the scene-linear prefix when managed
  Display is present, then dispatch managed output in the same encoder, without
  another host readback. Never apply the legacy sRGB display before the ICC kernel.
- pipeline-gpu/src/resident.rs, run_chain: retain generalized fusion and main's
  managed-display exclusion. Run the scene prefix through fusion, then run managed
  Display on the GPU-resident result. The old special-case Tone/Display fusion is
  superseded by generalized fusion, not by a loss of managed-output behavior.

Nonconflicting main changes were retained, including denoise injection/cache
revision, depth render variant, mask hooks, managed output and IOSurface code.
No CPU oracle or tolerances were changed. No main ref update or checkout occurred.

Validation: workspace release tests passed on the first attempt (733 passed,
0 failed, 17 ignored), without RUST_TEST_THREADS override. Four-package all-target
Clippy with -D warnings and cargo fmt --check passed. Logs are in this directory.
Inherited whitespace in main's generated Swift/C bindings and historical logs
was left untouched; the five conflict resolutions pass the scoped diff check.

The first three-sample benchmark was interrupted by the foreground tool timeout.
Its incomplete log is diagnostic only. A separately labelled background rerun
provides the complete measurements recorded in ../M2-17/validation.md.

No Kanban task ID was provided, so no board lifecycle mutation was possible.
The untracked brief was byte-identical to main's tracked brief and preserved as
brief.original.md before merging to avoid overwriting an untracked user file.
