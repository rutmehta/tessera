# tessera headless CLI

`cargo run -p tessera-cli --release -- <command>` (binary: `tessera`).
On macOS worktrees keep `CARGO_TARGET_DIR` outside the checkout.

Global `--app-dir PATH` and `--json` work before or after subcommands. `--app-dir`
overrides `TESSERA_APP_DIR`; when neither is set the default
app directory is `$HOME/Library/Application Support/Tessera`; its rebuildable
catalog is `index.sqlite`. JSON goes to stdout, diagnostics to stderr. Without
`--json`, results are pretty-printed JSON. Exit status: 0 success, 1 operational
failure / failed CoreML audit, 2 clap usage error.

Commands:

- `index DIR`: incremental recursive scan, reporting changed count, catalog total,
  and elapsed milliseconds. Sidecar selection, captions, and keywords are loaded.
- `index prune [--dry-run]`: count missing originals in the selected app directory's
  catalog and remove their image/file rows and search/dependent state. Dry-run
  reports the same counts without changing the index. This does not delete photos
  or sidecars. Inspect `--dry-run` before pruning a real catalog; disconnected
  drives also count as missing.
- `ls [--query TEXT] [--decision keep|reject|undecided]`: all matching images,
  without the catalog API's default 100-row limit. Query uses the index's FTS syntax.
- `cull set IMAGE --decision X|U|P [--grade 1|2|3] [--mark NAME]`:
  IMAGE is an indexed path or image ID. Grade requires P; an empty mark clears it.
  The cull crate writes recipe/XMP selection state and updates the index.
- `cull groups DIR`: scan, then return the cull crate's burst/near-duplicate groups.
- `cull sweep DIR [--below SIGNAL=VALUE] [--above SIGNAL=VALUE]`: read-only
  recommendations from existing scores, never automatic decisions. Defaults:
  sharpness < 0.2, exposure < 0.2, motion_blur > 0.8, noise > 0.8. Missing scores
  are ignored, so an empty result does not mean an image has been evaluated.
- `develop set IMAGE <Basic flags>`: exposure, contrast, highlights, shadows,
  whites, blacks, texture, clarity, dehaze, temperature, tint, vibrance, saturation.
  Only supplied fields change. Temperature/tint switch WB to custom. Writes an
  atomic `.edits/<stem>.json` envelope with edit history and synchronization clock.
  Existing XMP is imported when there is no recipe. RAW/JPEG stem collisions fail.
- `develop show IMAGE`: current `DevelopSettings`, or defaults if no sidecar.
- `render IMAGE --out FILE.png|FILE.jpg [--scale 1/8|1/4|1/2|1]
  [--settings JSON_OR_FILE] [--process auto|native|adobe] [--dcp PROFILE.dcp]`: `auto` (default)
  selects Adobe compatibility for sidecar Adobe PV3–6, otherwise native.
  `native` forces `image_core::Renderer` and tiled display output; `adobe` forces
  the direct `raw-decode` → `pipeline_adobe::render_scaled` compatibility path.
  Settings JSON is a DevelopSettings object and replaces (does not patch) sidecar
  settings, without changing auto's sidecar process selection. Camera orientation
  is applied after rendering the active area. JSON reports the selected process.
  `--dcp` explicitly supplies a parsed camera profile to the Adobe path (not native).
  Profile names in imported recipes never become paths. Without supplied bytes,
  compatibility uses the native camera matrix and an approximate default S-curve.
  See `crates/pipeline-adobe/ADOBE_COMPAT.md` for approximations and missing fields.
- `preview IMAGE --out FILE.jpg [--max 1024]`: camera embedded JPEG fast path,
  oriented and bounded without upscaling; missing/bad embedded JPEG falls back to
  the default RAW renderer. This is a camera preview, not a developed preview.
- `import lrcat FILE --inspect`: read-only import summary.
- `import lrcat FILE --fidelity --reference-dir DIR`: read-only fidelity audit
  against user-supplied Lightroom sRGB JPEG exports named **`<catalog_id>.jpg`**
  (not original stems; virtual copies need distinct exports). Uses imported
  settings and original RAWs through the Adobe compatibility pipeline at 1/4
  scale. References must have matching crop/orientation and either the exact
  quarter-render dimensions or the exact full-render dimensions. Full exports
  are reduced to quarter size with a triangle filter after checking the full
  developed extent (this requires an additional full render). Other dimensions
  are reported as failures, never silently fitted or aligned. Export pixels must
  already be oriented and sRGB; no ICC conversion or image registration is done.
  The `fidelity.images` report includes per-image `mean`, `p95`, and `samples`
  from `pipeline_adobe::fidelity::compare` only for successful comparisons.
  Missing originals/references are `skipped` with reasons; decode/render/size
  errors are `failed` with reasons, not zero scores. Summary counts distinguish
  compared, skipped, and failed; exit 0 means the report was produced, not that
  every image was compared. Can run alone or alongside `--inspect` or
  `--apply --dest NEW_DIR` (inspect/apply remain mutually exclusive). Fidelity
  does not modify the catalog, originals, references, sidecars, or local index.
- `import lrcat FILE --apply --dest NEW_DIR`: lossless import bundle containing
  `library.json`, `import-plan.json`, and `recipes/<catalog-id>.json`. Original RAWs
  and source catalog are untouched. Virtual copies remain distinct. Destination
  must not exist. This persists the import plan; it does not relocate photos or
  merge the imported library into the scanner's path-keyed index.
- `ml models`: registrations from `APP_DIR/models.toml`, using ml-runtime's manifest
  format. Missing manifest means no models. Listing does not download models.
- `ml check`: resolve pinned registrations into `APP_DIR/models/`, run real
  manifest-shaped probes, and report executed node/provider partitions. Requires
  exclusively CoreML execution. CPU fallback and missing registrations fail rather
  than claim success. Zero-input probes do not cover every data-dependent branch.
- `export IMAGE|DIR|--query TEXT --out DIR --format jpeg|png|tiff [--quality 90]
  [--long-edge N|--fit WxH] [--color-space srgb|p3|rec2020|prophoto]
  [--sharpen screen|matte|glossy] [--metadata all|copyright|none]
  [--name '{name}-{seq}'] [--jobs N] [--upscale 2|4]`: export source JPEG/PNG/TIFF or RAWs with
  sidecar develop settings. Directory inputs select immediate image children;
  queries use the existing catalog's FTS index. Sequence follows sorted source
  paths; `{date}` uses RAW capture time when present. Progress is on stderr;
  `--json` returns a summary with output paths on stdout. Cancellation stops
  pending images and removes in-progress temporary files; already committed
  images remain. Outputs are never overwritten. `--jobs` bounds concurrent
  rendering/encoding; decoded inputs are admitted in bounded waves.
  `--upscale` explicitly loads the pinned Real-ESRGAN registration from
  `APP_DIR/models.toml`, resolving weights into `APP_DIR/models/` (may download).
  It runs before resize/output sharpening, shares one CPU model session across
  the batch, and admits one decoded image at a time regardless of `--jobs`.
  CPU execution avoids CoreML dynamic-shape diagnostics corrupting JSON stdout.
  Duplicate output names are checked across the entire SR selection before any
  publication. Cancellation waits for an in-flight inference to return, then
  discards its unpublished output. Without `--upscale`, no SR registry is opened.

Tests use generated JPEGs and synthetic catalogs. The end-to-end RAW workflow
copies all five `fixtures/raw` files into a temporary directory so no fixture
sidecars are changed. It prints a skip message if that fixture set is absent.

Verification:

    cargo test -p tessera-cli --release
    cargo clippy -p tessera-cli --all-targets -- -D warnings
    cargo fmt --check
