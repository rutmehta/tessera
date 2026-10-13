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
11. UX-15 (Deliver) is the last package in SPEC §8.
