# WP M1-08 — Export

Read docs/01 §2.22 (Export), docs/08, CONTRACTS.md; use `pipeline_cpu::render` from main for now (the tiled `image-core` Renderer is being built in parallel as M1-06; structure the export so swapping in a `Renderer` trait later is a one-place change). Implement `crates/export` (add to workspace):
- `ExportSettings { format: Jpeg{quality} | Png | Tiff{bits:8|16}, color_space: Srgb | DisplayP3 | Rec2020 | ProPhoto, resize: None | LongEdge(px) | Fit(w,h) | Percent, sharpen_for: None | Screen | Matte | Glossy (simple unsharp mask by preset), metadata: All | CopyrightOnly | None, naming: template with {name} {seq} {date} tokens, output_dir }`.
- `export_one(image, recipe, settings) -> Result<PathBuf>`: render via pipeline-cpu at full size then resize with a Lanczos-3 filter (parallelise rows with rayon), convert to the output profile with `lcms2` (embed the ICC), encode (`jpeg-encoder` or `image` crate; 16-bit TIFF via `tiff` crate), write XMP metadata into the file (JPEG APP1 XMP packet; TIFF tag 700) honouring the metadata option, and write the standard sidecar's rating/label per docs/06 §2.1 mapping.
- `export_batch(items, settings, progress: impl Fn(Progress), cancel: &CancellationToken)` running N images in parallel bounded by available cores, resumable on cancel.
- Tests: JPEG round trip dimensions and embedded ICC present; 16-bit TIFF is 16-bit; naming template; metadata None leaves no XMP packet; batch of 5 fixtures at long edge 1024 completes and reports progress 5 times; cancellation mid-batch leaves no partial files (write temp + rename).
- Bench (ignored): 5 fixtures full-size JPEG q90, print ms per image.
`cargo test -p export --release`, clippy -D warnings, fmt.
