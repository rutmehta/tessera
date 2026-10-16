# Redesign follow-ups (after UX-10 … UX-14, 2026-10-13)

Open items found while merging and installing the first five work packages. Ordered by how
much they would annoy Rut.

1. **Thumbnails of healed / AI-masked photos.** The thumbnail renderer (`tessera-ffi/src/preview.rs`,
   `render_thumbnail`) failed on DSC02488 after UX-13 wrote a heal spot and AI mask groups into its
   recipe; the grid and filmstrip cell stayed blank. `f7f6bb55` makes the loader fall back to the
   embedded preview and log the engine's reason (`log show --predicate 'subsystem == "dev.tessera.app"'`).
   Still to do: make the thumbnail path render exactly what Develop renders (same mask rasters,
   same retouch hook), or render thumbnails of edited photos from the Develop session's own output.
2. **`tessera render` (CLI) refuses recipes with retouch spots** ("no renderer is registered"):
   register `brush::render_retouch` there as develop.rs and preview.rs do.
3. **Triage thresholds** were calibrated on one Sony shoot (`CullTriage.BlurThresholds`: sharpness
   ≤ min(p15, 0.08), or motion ≥ 0.45 with sharpness ≤ min(p30, 0.25)). Validate on a dim indoor
   shoot and on a phone JPEG folder before trusting the counts.
4. **Header exposure in Cull** ("1/640 f/4") needs `ExposureTime` / `FNumber` in the catalog's
   metadata fields; the scan does not store them yet.
5. **Light appearance** keeps the pre-redesign amber/graphite set; a custom accent does not carry
   into Light until the parity pass (SPEC §2 says dark-only for now).
6. **Clear the crowd** is a background-quieting look, not distraction removal: the LaMa/Remove path
   exists only for Document sessions.
7. **"More from how you usually edit"** runs the style-profile agent; its card shows the plain photo
   until a synchronous recipe from the style profile exists.
8. **Loupe "Shortcuts" hint** still peeks under the agent field in Edit.
9. **Two keystrokes-to-the-wrong-app incidents** during automation: UX-12 and UX-14 both found
   System Events targeting the frontmost app. Rule for every later WP: drive only a copy of a
   few RAWs in the scratch directory, never the owner's real folder, and never by frontmost.
10. **Stray sidecar** `DSC02488.ARW.xmp` in the World Cup folder (WB 6300 K, a heal spot, AI masks)
    was written by those runs; the owner decides whether to delete it.
11. **Deliver (UX-15) leftovers.** Instagram is "fit inside 1080 × 1350", not a 4:5 crop: export has
    no crop, so a landscape photo lands at 1080 × 720 (the cell says so). The size estimate is
    bytes-per-megapixel by format and quality, labelled "about"; the engine could report the
    real figure after a dry run. Engine items carry no pixel size (`PhotoItem.pixelWidth` is 0),
    so Deliver reads the file header through ImageIO per photo (cached); the scan should store
    it with the other metadata (see item 4). Evidence was rendered in-process
    (`DeliverEvidenceTests`, `TESSERA_DELIVER_EVIDENCE` + `TESSERA_DELIVER_SHOOT`) because the
    automation shell had no screen-recording permission; a SwiftUI `ScrollView` draws a white
    ground in those renders, so the grid scrolls only when the cells do not fit.

## After ST-1 / ST-2 (2026-10-16)

12. **Engine memory.** ~~On the 8-RAW sample the footprint sits at 2.3–3.5 GB after closing everything (0.7 GB after the scan alone; ~64 MB per opened photo stays behind). Nothing on the Swift side references it (`EngineSessionMemoryTests`); it is the engine's shared renderer/memo cache and per-image residue in `crates/`. Needs a cap and release-on-close in image-core.~~ Done (ST-3). `TESSERA_MEM_TRACE=1` prints the engine's cache table at scan/open/close (`Engine::memory_report()` over FFI); `sample_shoot_memory_table` (`TESSERA_SAMPLE_SHOOT=<dir>`) is the evidence. The holders were the GPU resident memo (511 MB, never released on close), the recycled device buffers (up to 1 GB) and macOS 26 malloc keeping freed large blocks resident (~650 MB; `malloc_zone_pressure_relief` returns 0). Fix: release the image's memo pages and the session's rasters on close, a memo budget of 15 % of RAM (`TESSERA_CACHE_BUDGET_MB`) in place of 512 MB each, and a global allocator that `mmap`s blocks ≥ 128 KiB so they leave the footprint on free. Engine-only, 8-ARW sample: after closing everything 1.70 GB → 0.31 GB; peak with one session open 1.75 → 1.18 GB; no growth across reopening the same photos. With the shell (`EngineSessionMemoryTests`, five fixture RAWs): 304 MB after closing everything, 1.52 GB peak. The engine-side scan retains nothing (9 MB); the 650 MB the Swift test shows after the scan is the shell's own (the test process and its previews).
13. **`index_folder` scans under the catalog lock** (183–686 ms holds under load): the index crate's scan is one `&mut self` pass (stat → EXIF → write). The UI no longer queues behind it for lookups (separate reader), but `list_images`/`roots` answer Busy during it. Split the scan into read-outside / write-inside passes.
14. **AI masks re-segment on session open** ~~for recipes that already carry AI masks (seen as a 2.5 s `AiMaskJob` on the owner's folder). Load the saved rasters instead.~~ Done (ST-3): computed rasters are stored per image under `mask-cache/<image id>/` when segmentation finishes and loaded (off the session lock, "Loaded" in the panel) on the first render that uses them; only a missing raster segments. Rasters created before ST-3 were never stored, so those recipes segment once more and are stored then. Thumbnails (item 1) could load from the same store.
15. **`getHistogram` per frame and the `DevelopSession` per-tick calls stay on the main actor** by design; measure them under the watchdog on a 61 MP file.
16. **`cargo test -p tessera-ffi` alone fails two understanding tests** (`suggestion_jobs_report_progress…`, `accept_is_catalog_only…`: "blue" no longer maps onto an existing Colors > Blue). They pass when the engine crates are tested together, so it is feature unification in the test build; pin the feature in `tessera-ffi`'s dev-dependencies so the single-crate run matches.
17. **UI collisions reported by the owner on the work Mac** are still unlocated: needs a screenshot and the window size; the layout audit passes at 1440×900 and up.
