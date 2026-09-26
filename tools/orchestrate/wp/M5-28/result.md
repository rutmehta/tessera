# M5-28 implementation and parent verification

RESULT: PASS

This report supersedes the previous constructor-only report and the sandboxed workers' GPU-verification blockers. Work is uncommitted on `wp/M5-28`. No Kanban task was bound to this session, so no board transition was made.

## Delivered

- `RgbSource::from_raster` accepts profile-tagged planar linear f32 RGB, removes encoded-profile TRCs, applies the working-primary conversion without clipping signed/HDR values, and validates the result. `from_linear_rec2020` supplies an exact-bit boundary for already converted planes.
- Camera Raw CPU evaluation now calls `image_core::Renderer::render_rgb_linear`. Its separate bounded f32 LRU retains Lens, WB, Detail, Tone, Color, Locals, Effects and Geometry checkpoints. Source identity includes exact pixel bits, tile revisions and colour/level/canvas interpretation. Stage settings are chained. Lookup starts at the latest retained checkpoint, so an evicted ancestor does not force recomputation of a cached descendant. Warm/cold results remain exact; WB edits reuse lens analysis and colour edits reuse upstream tone/detail. The filter cache is capped at 256 MiB payload and 64 checkpoints. Backend changes create a fresh RGB memo.
- Original-pixel automatic optics analysis, CA/defringe ordering, WB/vignette gains and final resolved lens warp are preserved on CPU. Tests compare the staged path with the independent scalar Develop reference, including real automatic CA/vignette corrections, profiles, signed/HDR pixels, source/revision edits, slider edits and cache eviction.
- Remaining Camera Raw GPU/geometry/effects exclusions have individual reasons and CPU behavior in `crates/filters/README.md`. Automatic GPU lens/CA estimation, defringe and Upright are still engineering gaps, not fictitious RGB impossibilities. Missing depth/calibration inputs and genuine raw-domain limitations are distinguished. This uses the package's documented-unsupported alternative; it does not claim the entire resident Develop chain is implemented.
- Positive-radius Shadows/Highlights executes resident GPU backdrop-prefix replay, horizontal and vertical edge-aware luminance passes, and ordinary adjustment blending. CPU cached rendering no longer returns Unsupported for these adjustments. Document-wide revision invalidation and expanded group prefixes prevent stale neighbor results.
- HDR Toning is a validated, serializable adjustment with CPU and resident GPU Local Adaptation (radius/strength, gamma, exposure, detail, shadows/highlights, vibrance, saturation and toning curve), frozen Equalize Histogram CDF, Exposure-Gamma and Highlight Compression. PSD export explicitly reports native-only instead of inventing an Adobe tag.

## Important implementation choices and limits

- Shadows/Highlights deliberately changes the native CPU reference from a direct 2D bilateral to a horizontal-then-vertical bilateral. It is not numerically equivalent to the old reference or to Photoshop. CPU and GPU now share this separable formula; tested absolute parity tolerance is 1e-4. This semantic change is documented in COMPOSITOR.md and independently tested.
- Resident spatial execution uses conservative whole-level backing and GPU prefix replay rather than sparse smart-filter-style tile-halo windows. Real neighbors and document-edge replication eliminate tile seams, including L0/L2 and sequential/grouped operators. This is not a constant-memory implementation. Device limits are checked and oversized levels can return ResourceExhausted; local edits conservatively invalidate the spatial level. Existing resident layer-style restrictions remain.
- Histogram equalization stores resolved histogram parameters; its constructor accepts caller-provided counts rather than analyzing each tile independently. Native formulas do not claim proprietary Adobe equivalence.
- Camera Raw's cache budget bounds retained payload, not in-flight scratch or caller-held outputs. Its shared CPU cache serializes evaluations. Remaining resident capability gaps and buffer limits are explicit in the filter README.

## Parent-executed verification

Retained `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M5-28` for every Cargo command.

Executed the exact required gate after both workers finished:

    cargo test -p compositor -p filters -p image-core --release && cargo clippy -p compositor -p filters -p image-core --all-targets -- -D warnings && cargo fmt --check

Exit status: **0**. Full output: `final-gate.log`.

Parsed 80 test summaries: **422 passed, 0 failed, 17 ignored**. Ignored benchmarks were not claimed as executed. Existing LibRaw C++ warnings remain; Rust Clippy passed with warnings denied.

The parent also ran `cargo test -p compositor --release --test m528_resident` independently: exit **0**, recorded in `parent-gpu.log`. The new tests require a real adapter and do not silently skip. They cover both new HDR and positive-radius Shadows/Highlights on GPU, L0/L2 seams, masks, alpha, blend modes, isolated/pass-through/clipping groups, viewport/full-render reuse, sequential local operators and neighboring edits. These tests passed again in the final gate. The sandboxed workers' reports of unavailable adapters are historical and are superseded by this real parent execution.

`git diff --check` passed. Programmatic changed/untracked-path audit found **no paths outside the allowlist**. No build output was placed in the worktree and no commits were made.

## Files to review

- `crates/image-core/src/rgb.rs`, `rgb_render.rs`, `render.rs`; `crates/image-core/tests/rgb_memo.rs`
- `crates/filters/src/camera_raw.rs`; Camera Raw tests and `crates/filters/README.md`
- `crates/compositor/src/adjust/hdr.rs`, `adjust/shadows.rs`, `adjust.rs`, `psd.rs`
- `crates/compositor/src/render/exec.rs`, `render/mod.rs`
- `crates/compositor/src/resident/spatial.rs`, `spatial.wgsl`, `program.rs`, `adjustments.wgsl`, `mod.rs`
- `crates/compositor/tests/hdr_toning.rs`, `m528_resident.rs`, updated M5-26 regression tests

Integration hotspots: `render/exec.rs` and `resident/mod.rs` contain the neighborhood scheduling/cache changes. Reconcile these deliberately when integrating sibling work.
