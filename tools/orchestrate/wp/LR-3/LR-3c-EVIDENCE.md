# LR-3c: caller-owned retouch rendering

This lane starts at `634d3fbc` on `wp/LR-3-retouch`, without rebase. Option A
breaks the dependency problem without changing any dependency edges.
Implementation: `cf85e2ca`. Regression tests: `664423b7`. Final preview/admission
audit and regressions: `53964430`. All are local commits with `Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>` trailers.

## Contract and stage order

`pipeline_cpu::RetouchRenderer` has one method: render ordered retouch operations
into a width/height plus mutable planar RGB buffer. Its blanket implementation
for matching functions lets engine assembly register `brush::render_retouch`
without brush depending on pipeline-cpu. The function uses the existing
`Stroke` clone/heal kernels over an F32 raster; it does not implement a second
healing solver.

`LensContext::retouch`, `image_core::Renderer::with_retouch_renderer`, and
`ExportSettings::retouch` own `Arc<dyn RetouchRenderer>`. There is no global or
static retouch registry. Recipe/backend snapshots retain the Arc. Replacing the
renderer drops the RGB memo so cached retouch output cannot survive a change of
implementation.

Retouch runs after local adjustments and before lens blur, point effects,
geometry and final downsampling. The CPU reference, image-core RGB memo path,
and image-core M2 host path all execute it. Retouch makes M2 active and makes
resident GPU rendering ineligible, using the same host-stage routing as local
adjustments. Kernel errors propagate through the GPU host path. FFI admission
preserves the entire retouch list (the old local-settings sanitizer discarded
it). Detail/loupe renders retain the full image for retouch so a clone source
outside the crop plus margin remains available.

Any nonempty spot list without a renderer raises `EngineError::InvalidArgument`
with name `retouch`, including lists whose operations are all disabled. A
registered renderer skips disabled operations. Unsupported enabled kinds,
non-brush targets, inverted/subtractive components, erasing strokes, and
nonzero operation-level feather return errors; they are not discarded.

## Construction-site audit

| Site | Wiring and callers |
| --- | --- |
| `tessera-ffi/src/backend.rs::Backend::renderer` | Registers brush for CPU/GPU backend calibration and every renderer returned through `Engine::develop_renderer`; session/request snapshots retain it. |
| `tessera-ffi/src/lib.rs::Engine::open` | Registers brush on the general `PreviewStore`; edited CFA and LinearRaw library previews pass its Arc through their CPU render contexts. Unregistered stores reject retouch even on a cache hit. |
| `tessera-ffi/src/lrcat_fidelity.rs::fidelity_renderer` | Registers brush on the independent CPU fidelity renderer, including native/Adobe JPEG retouch (whose old path called reference functions directly). |
| `tessera-ffi/src/smart_preview_thumbnail.rs` | Registers brush on the independent thumbnail renderer. The existing camera-linear proxy admission guard still requires the original for retouch. |
| `tessera-ffi/src/export.rs::ExportOptions::settings` | Supplies the Arc to file/batch export settings. Export selects the CPU scene stage for spots, then preserves output profile/encoding, DNG and enhancement handling. |
| `tessera-ffi/src/export.rs::Engine::render_for_print` | Uses the explicit `render_pixels_with_retouch` context entry point. |
| `export/src/hdr.rs` | Passes the file export's Arc into the scene render before HDR output processing. |
| `tessera-mcp/src/preview.rs::PreviewCache::renderer` | Registers brush for the independently assembled graph renderer; this crate has no FFI dependency. |
| `tessera-mcp/src/preview.rs::retouch_context` | Supplies the Arc to both RGB display and scene-linear preview calls. |
| `tessera-mcp/src/exports.rs` | Registers brush in both file-export settings construction sites. |

Other `Renderer::new` occurrences in the FFI document module are layered-document
compositors or typography renderers, not Develop contexts. The renderer in
`export/src/depth.rs` remains a denoise/depth-only path: retouch combined with AI
masks, RAW denoise or depth is rejected explicitly before that path. Legacy
standalone pipeline/export callers with no supplied renderer fail explicitly.
The managed GPU wrapper has no retouch capability and its existing strict
validation still errors; retouch export bypasses that wrapper. ML caption/embed
preview stores use only the default recipe and do not construct edited Develop
contexts.

Camera-linear smart previews continue to report `original required` for retouch.
This lane does not claim Adobe healing solver parity, support for remove/skin
operations, or support for the rejected export hook combinations.

## Regression coverage

The RED test from `a1d7e5ff`,
`heal_and_clone_spots_render_through_develop_cpu`, is relocated into
`tessera-ffi/tests/lr3_develop_retouch.rs`, where the existing dependency graph
allows brush registration. Its heal+clone recipe and changed-clone-pixel
assertion are preserved. Only the call changes to the explicit caller-owned
context API. The original pipeline-cpu test file now verifies the required
missing-renderer failure using the same recipe. This relocation avoids adding
a test dependency or a fake replacement for brush.

New synthetic tests cover:

- Clone and heal through CPU Develop versus independently applied `Stroke`
  kernels at full resolution (L0), comparing every channel with `f32::to_bits`
  (bit-identical). This does not claim bit parity across preview pyramid levels.
- Graph registration, native RGB memo invalidation, and missing-renderer errors.
- Actual GPU backend admission to the CPU retouch stage, changed destination
  pixels, and propagation of an injected retouch-kernel error.
- Synthetic SQLite catalog import through recipe/history into the actual
  image-core CPU Develop path using the imported process version.
- Unsupported operation errors and disabled-operation identity.
- Pixel/print export, PNG file pixels, HDR file pixels, and missing-renderer
  errors for these entry points.
- The actual FFI backend assembly and both MCP preview dispatch paths.
- FFI admission preserving supported, unsupported and disabled operations;
  library preview rendering and cache-hit rejection without a renderer;
  native/Adobe JPEG fidelity; a real headless Develop session detail surface
  whose clone source lies outside the detail window.

All new fixtures are generated synthetic data. No personal catalog, app launch,
Swift gate, board update, push, or rebase is part of this lane.

## Gates

Every Cargo gate uses:

```sh
export PATH="$HOME/.cargo/bin:$PATH"
export CARGO_TARGET_DIR=$HOME/.cache/tessera-target/LR-3-retouch
export CARGO_BUILD_JOBS=3
export RAYON_NUM_THREADS=3
```

- PASS: final focused retouch integration suite, **9 passed**. This includes the
  headless FFI session, distinct RGB channel bit comparisons, and HDR pixels.
- PASS: catalog/direct-operator tests, **2 passed**; pipeline missing-renderer
  test, **1 passed**; MCP retouch assembly test, **1 passed**.
- PASS: final FFI library unit suite after the admission changes:
  **215 passed, 3 ignored** (includes backend assembly and both JPEG fidelity
  tests). The separate JPEG fidelity run also passed both tests.
- PASS: existing synthetic LinearRaw preview compatibility/cancellation tests,
  **2 passed**.
- PASS: `cargo clippy --locked -p import-lrcat -p engine-api -p pipeline-cpu
  -p brush -p tessera-ffi -p image-core -p export -p tessera-mcp -p previews
  --all-targets -- -D warnings`.
- PASS: `cargo fmt --all --check` and `git diff --check`.
- PASS: `git diff 634d3fbc --exit-code -- Cargo.toml Cargo.lock
  '**/Cargo.toml' '**/Cargo.lock'`: zero manifest/lockfile delta.
- FAIL: `cargo test --locked -p import-lrcat -p engine-api -p pipeline-cpu
  -p brush -p tessera-ffi --no-fail-fast -- --test-threads=3`: **921 passed,
  2 failed, 34 ignored**, across 110 result blocks, exit 101. All retouch
  regressions pass; the failed targets are `develop` and `document_liquify_ui`.
  Supplemental run counts above are not additional unique tests.

The two failures were rerun individually with `--exact --nocapture
--test-threads=1`, the same Cargo/Rayon environment, and no other local test
jobs. Thresholds and backend selection were unchanged. Both retries exited 101:

```sh
cargo test --locked -p tessera-ffi --test document_liquify_ui brush_latency_on_a_20_megapixel_layer -- --exact --nocapture --test-threads=1
cargo test --locked -p tessera-ffi --test develop export_batch_does_not_starve_slider_drag -- --exact --nocapture --test-threads=1
```

| Test | Full gate | Isolated serial retry |
| --- | --- | --- |
| `document_liquify_ui::brush_latency_on_a_20_megapixel_layer` | FAIL: p95 548.6 ms, median 307.0 ms, max 1586.5 ms; p95 limit 250 ms. | FAIL: p95 416.2 ms, median 256.3 ms, max 516.4 ms. |
| `develop::export_batch_does_not_starve_slider_drag` | FAIL: 0/120 frames at required L2 (all L3); render p90 7.8 ms, set-to-frame p90 12.4 ms; 5 exports completed in 111.86 s. | FAIL: 105/120 frames at L2 (87.5%, below required 90% / 108 frames); render p90 5.6 ms, set-to-frame p90 12.2 ms; 5 exports completed in 72.41 s. |

The full Rust gate is **not green**. These failures are in existing performance
checks, but no comparison run on the base commit was performed; this evidence
does not establish that they are unrelated to this change.

Existing LibRaw C/C++ build-script warnings are emitted independently of Rust
clippy; Rust warnings-denied passes. No test thresholds or baseline goldens were
changed. Full logs are retained on Machine B in
`$CARGO_TARGET_DIR/lr3c-evidence/` (not committed as large artifacts).
