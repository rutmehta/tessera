# WP M3-05m — Merge main into wp/M3-05 and reconcile

In this worktree (branch wp/M3-05), run `git merge main` and resolve the conflicts so that BOTH sides' behaviour is kept:
- `crates/pipeline-cpu/src/render.rs` and `lib.rs`: main added `render_linear_scaled_with_depth` (lens blur with a caller-supplied depth plane, M3-06) and a `render_linear_impl(..., depth: Option<..>)`; this branch added `render_linear_scaled_with_denoise(..., denoiser: Option<&dyn PostDemosaicDenoise>)`. Unify into one impl taking both optional inputs (`depth` and `denoiser`), keep both public wrappers, keep validation semantics of both (the depth path validates settings without lens_blur).
- `crates/export/src/lib.rs`: main passes `settings.color_space` into `render_full` (M2-15 colour management); this branch adds the `upscale` path with `render_full_float` + `upscale_rgb`. Keep colour management on both paths (upscale in linear float, then convert to the output profile).
- `crates/ml-runtime/models.toml`: keep every model entry from both sides.
Then: `cargo test --workspace --release` must be fully green, clippy -D warnings on pipeline-cpu/export/image-core/ml-enhance/tessera-cli, fmt. Commit the merge on wp/M3-05. Do not touch main.
