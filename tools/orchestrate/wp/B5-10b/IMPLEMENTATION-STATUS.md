# B5-10b implementation status: one font snapshot for every document compositor; Channels thumbnails after PSD reopen

Branch `wp/B5-10b` (base `wp/B5-10`, 98bb1e2). All gate commands pass (see "Gate"). No app launch was needed.

## 1. One font snapshot everywhere

New `crates/tessera-ffi/src/document/fonts.rs` (declared in document.rs):

* `fonts::compositor(budget) -> Compositor` — `Compositor::new(budget)` plus
  `set_text_renderer(text::shared_text_renderer())` (the snapshot the Type tool lays out with).
* `fonts::install(&mut ResidentRenderer)` — the same snapshot for the resident (Metal) renderer.

B5-10's private `render.rs::text_compositor` is removed; every construction in `document/**` goes through
the helper. Construction sites (after this change):

| Site | Path it serves | Before |
| --- | --- | --- |
| render.rs `Renderer::new` (resident) | viewport frames, read-back | B5-10 inline injection, now `fonts::install` |
| render.rs `Renderer::new` (`Backend::Cpu`) | viewport without Metal | `text_compositor` → `fonts::compositor` |
| render.rs `GpuBackend::cpu` | **styled-document CPU fallback** (B5-07) | `Compositor::new` — **no snapshot (fixed)** |
| render.rs `thumbnail` | layer / mask / composite thumbnails (Channels RGB rows) | `text_compositor` → `fonts::compositor` |
| render.rs `composite_raster` | merge down, flatten | `text_compositor` → `fonts::compositor` |
| io.rs `export_flat` | File ▸ Export flat PNG/JPEG/TIFF | `Compositor::new` — **fixed** |
| filters.rs `FilterState::default` (`comp`) | filter previews, smart-filter bakes, filter detail, raster filter regions | `Compositor::new` — **fixed** |
| filters.rs `native_filtered` | native smart-filter stack render | `Compositor::new` — **fixed** |
| filters.rs `for_output` | smart filters baked for export / merge / flatten | `Compositor::new` — **fixed** |
| tools.rs `sample_color` (`sample_all`) | eyedropper over the composite | `Compositor::new` — **fixed** |

retouch.rs, channels.rs, styles.rs and text.rs construct no compositor (retouch renders through the filter
state's compositor; channel thumbnails read channel samples; the style fallback is `GpuBackend::cpu`).
`text.rs` only had its module comment updated to point at fonts.rs. `grep Compositor::new
crates/tessera-ffi/src` now finds only fonts.rs.

Not reachable from the bridge (engine-owned, NEEDS.md): the PSD writer's composite and text-layer pixels,
the resident renderer's nested smart-object child renderers and CPU stack, `ConvertToPixels` /
`rasterize_layer`, engine merge helpers. They discover the same installed fonts, so output matches for
installed families; a family only in the host snapshot fails there (reproductions in NEEDS.md 1–2).

## 2. Channels thumbnails black after PSD reopen — root cause

Not the PSD reopen and not channels.rs / io.rs. In B5-10's evidence the Channels rows that went black
(359-missing-font-1440.png) are **RGB / Red / Green / Blue**, right after step 359 added a text layer in
"No Such Font Family"; 358-reopened-psd.png (same reopened PSD, before that layer) shows them correctly.
Those four rows come from `composite_thumbnail`, which renders the whole document; with a missing-font
text layer the compositor returns the documented `font unavailable: <family>` error (no substitution),
and the host (`DocumentChannels.componentThumbnail`, `try? doc.backend.compositeThumbnail`) turns the
error into nil images, drawn as empty black wells. Saved alpha / spot channel thumbnails are unaffected.

Reproduced in Rust and pinned as regressions:

* `document_channels_ui::channel_thumbnails_survive_psd_reopen` — alpha + spot channels, save as PSD /
  .tessera-doc at U8, U16 and F32, reopen, `channel_thumbnail` pixel values for both (passes; the reopen
  path was already correct).
* `document_fonts::channels_thumbnails_after_psd_reopen_and_a_missing_font` — PSD with text, alpha and spot
  channels, reopen: composite thumbnail equals the viewport and channel thumbnails are right; adding a
  missing-font layer makes the composite thumbnail fail with the documented error while channel thumbnails
  stay right; hiding the layer restores the composite thumbnail.

Engine: nothing to fix (the error is the documented contract, NEEDS.md 5 notes the product option).
Host follow-up (outside this package's allowed paths, Swift): show the thumbnail error (e.g. a warning
glyph / tooltip with the message) instead of an empty well when `compositeThumbnail` throws; the viewport
frame fails the same way and keeps the previous frame on screen.

## Tests (crates/tessera-ffi/tests)

`document_fonts.rs` (new, 5 cases; the Noto Sans fixture is only in the shared snapshot, so a compositor
without it reports a missing font — cases 1–2 failed before the change, verified):

1. `text_renders_identically_through_every_document_compositor` — viewport vs export_flat PNG, full-size
   composite thumbnail, eyedropper, merge down (+ undo), Gaussian blur preview on the text as a smart
   object (mean colour preserved, ink spread), apply = preview, export of the smart filter, flatten: all
   within 1 8-bit code value (docs/11 §1.3).
2. `styled_text_uses_the_shared_fonts_on_the_cpu_fallback` — drop shadow on fixture text; the session's
   CPU fallback equals a CPU compositor over the fixture fonts (≤ 1 code value).
3. `psd_composite_matches_the_viewport` — Helvetica (the PSD writer is engine-side): the stored PSD
   composite, the reopened viewport and composite thumbnail equal the viewport.
4. `a_missing_font_fails_every_path_with_the_documented_error` — viewport, export_flat, composite and
   layer thumbnails, eyedropper, merge down, flatten, PSD save and a smart-object filter preview all
   return `font unavailable: No Such Family 12345`; the document renders once the layer is hidden.
5. `channels_thumbnails_after_psd_reopen_and_a_missing_font` — see section 2.

`document_channels_ui.rs`: +1 case (`channel_thumbnails_survive_psd_reopen`).

## Gate (this worktree, CARGO_TARGET_DIR=$HOME/.cache/tessera-target/B5-10b)

* `cargo test --locked --release -p typography -p compositor -p psd -p tessera-ffi`: exit 0,
  530 passed, 0 failed, 20 ignored (summed over all test binaries)
  * `--test document_fonts`: `test result: ok. 5 passed; 0 failed; 0 ignored`
  * `--test document_channels_ui`: `test result: ok. 11 passed; 0 failed; 0 ignored`
  * `--test document_text_ui`: `test result: ok. 18 passed; 0 failed; 1 ignored`
* `cargo clippy --locked --release -p tessera-ffi --all-targets -- -D warnings`: exit 0
* `cargo fmt --all -- --check`: exit 0; `git diff --check`: exit 0
* `./build-ffi.sh`: exit 0, regenerated bindings unchanged (no FFI surface change; engine-api unchanged)
* `swift build --jobs 2`: exit 0; `swift test --jobs 2`: `Executed 295 tests, with 0 failures`
* xcodebuild Debug: `** BUILD SUCCEEDED **`
