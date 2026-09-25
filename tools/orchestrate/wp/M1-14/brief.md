# WP M1-14 — Preview fallback when a raw has no embedded JPEG

Finding from the on-screen test: `fixtures/raw/sample.dng` (Leica M9) has no embedded JPEG, so the grid shows a blank tile. Read crates/previews, crates/tessera-ffi (embedded_preview path), crates/pipeline-cpu (render entry point), crates/raw-decode.
- In `crates/previews`, add `PreviewSource::Rendered`: when `embedded_preview()` is `None` (or the embedded JPEG is smaller than 1/8 of the sensor size), render a preview with `pipeline_cpu::render` at reduced scale (decode CFA, demosaic bilinear for speed, default settings) and store it in the pyramid like an embedded one. Must run off the UI thread through the `jobs` scheduler at `Priority::Preview`; the FFI `embedded_preview` call returns immediately with `None` + a "pending" state and later fires the existing preview-ready callback.
- Also handle EXIF orientation for rendered previews (raw-decode gives orientation 1–8).
- Cache key must include the recipe hash so edited images get re-rendered previews later (default recipe for now).
- Tests: `sample.dng` yields a non-blank preview (mean luminance > 0.02 and stddev > 0.01) within 3 s in release; an image with an embedded JPEG still uses the fast path (assert no pipeline render was invoked via a counter); orientation applied.
- App: no Swift changes should be needed if the callback path works; if `apps/mac` needs a one-line change to refresh the cell on the callback, make it.
`cargo test -p previews -p tessera-ffi --release`, clippy -D warnings, fmt; `(cd apps/mac && swift build)`.
