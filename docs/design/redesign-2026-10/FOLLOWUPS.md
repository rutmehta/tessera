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

## UX-15: collisions located (2026-10-16, screenshots on the sample shoot at 1385 × 868 pt)

Item 17 is located. Evidence: `docs/design/redesign-2026-10/evidence/UX-15/` (`before/` and
`after/`, rendered in-process by `CollisionEvidenceTests` with `TESSERA_UX15_EVIDENCE` and
`TESSERA_UX15_SHOOT` on a copy of the five fixture RAWs at 1385 × 868; see its README) and the
session screenshots described here. All on the 8-ARW sample shoot, window 1385 × 868.

**Fixed by the UX-15 collisions package (2026-10-16); what remains is noted per item.**

18. **Deliver drawn over an open Develop session.** Switching Cull → Edit → Deliver with a photo
    open leaves the Edit panel (Light/Color/Detail/Optics/Effects/Edits), the six Try cards, the
    develop tool strip and the "Rendering…" capsule on screen *under and over* Deliver's own
    columns; Deliver's "Finish saving the photo before leaving this workspace" and "Ready. The
    agent can also do this…" lines print across the Try cards. A mode switch must close or
    suspend the develop chrome first (save-and-close like navigation does), and Deliver shows
    only its columns, its grid and the agent field.
    *Done:* `selectShellMode(.deliver)` with a photo open goes through
    `AppModel.returnToLibraryThenDeliver()` (the `navigateAfterDevelopSave` path: save, close,
    leave Edit, then `enterDeliver`); a save that cannot finish keeps Edit up and says so. The
    views gate the Edit panel, the HUDs and the Try cards on `isPhotoEditing && shellMode == .edit`
    and `DeliverView` on `deliverMode && !isPhotoEditing`. `ShellModeSwitchTests`; evidence
    `after/06-deliver-switching.png` (captured 0.3 s after the switch) and `07-deliver-settled.png`.
19. **Edit never leaves "Rendering…".** The loupe showed the photo at thumbnail size (about
    120 × 80 pt, centred, not scaled to the loupe) with a loose "Rendering…" capsule at the top
    of the panel for over a minute. Cause: the develop open waited on the catalog lock held by
    `flush_drive → Index::roots()` (PF-1, fixed on main). The shell side still needs: the
    embedded preview scaled to fit the loupe immediately, the capsule inside the Edits readout
    line rather than floating, and after 2 s a reason ("Waiting for the engine · 12 s").
    *Done:* `LoupePlacement.fit(magnify:)` lets a preview frame (not an engine frame) scale past
    1:1, so the thumbnail and then the embedded preview fill the loupe at once; the histogram
    strip stays empty while opening and the Edits readout carries "Rendering…", then after 2 s
    `DevelopLoadingReadout.text` ("Rendering… · Waiting for the engine · 12 s", a `TimelineView`
    ticking each second from `AppModel.developLoadingSince`). `DevelopLoadingReadoutTests`;
    evidence `after/04-edit-opening.png` (0.4 s after the switch; the fixtures open in under 2 s,
    so the reason text is covered by the unit test only).
20. **Toast over the quick-action chips.** In Cull the "Opened sample-shoot: 8 images" toast
    sits exactly on the "Keep the pick in every burst · Reject the blurry ones · Find closed
    eyes" chip row above the agent field. The toast belongs above the chips (one row up), or
    the chips hide while a toast shows.
    *Done:* the toast is one row up, `Theme.Space.s` above the agent row (chips included), and
    every mode keeps that band clear (`Theme.Height.toastBand` = 44 pt added to the burst view's
    bottom padding, `CullWorkspace.gridBottomInset` and Deliver's preview padding), so it also
    never sits on the burst view's action buttons (where the in-process render had it) or
    Deliver's status line. `ToastLayoutTests` measures the toast, the chips, the burst actions
    and Deliver's status line through `ShellLayoutProbe`; evidence `after/01-cull-toast.png`.
21. **Bottom progress bar in Cull.** "Measuring sharpness and exposure for culling · 0 / 8 ·
    DSC02497.ARW · Stop" is a full-width bar at the bottom while the Cull column already says
    "Sorting the shoot while it loads: 0 of 8 measured". One place: the column line gets the
    Stop control; the bar goes.
    *Done:* `AssistProgressBar` is gone; the Cull column's line reads "Sorting the shoot while
    it loads: n of m measured" (a face pass names itself) with the Stop button
    (`library.assist.stop`). With the panel hidden (Tab) the pass runs without an indicator.
22. **Inspector in Cull is a wall of prose.** With the panel open: Image, Selection (three rows
    of chips), Assist (two-line paragraph), People, Agent Edit (paragraph + button), Keywords
    (field + "Suggested" + paragraph + "Keyword List" + paragraph), Metadata (eight fields +
    paragraph), Photo Edit (button + paragraph). Keep one short line per section at most and
    move the rest to `.help`; Keywords and Metadata collapsed by default in Cull; drop the
    "Photo Edit" section (Edit is a mode).
    *Done:* Assist, People, Agent Edit, Keywords (Suggested, Keyword List) and Metadata keep one
    short line each and carry the explanations as tooltips; Keywords and Metadata start
    collapsed (`PanelSection` remembers the person's choice in `InspectorPanel.<title>`); the
    Photo Edit section is removed (`library.inspector.editPhoto` dropped from the established
    identifiers). Evidence `after/03-cull-loupe-panel.png`. The Image rows stay: they are values,
    not prose.
23. **Grid view.** Burst 3's two thumbnails stayed blank while bursts 1–2 drew (the strip in
    Burst view had drawn them). The built-preview thumbnails carry a "Smart Preview" text label
    inside the cell; a small glyph in the corner, not text. "review" is a bare lowercase link.
    *Done:* a cell whose request returned without an image (the engine declined, the embedded
    preview was unreadable, the preview event stream closed) stayed blank for good:
    `ThumbnailCell.resumeLoading` only re-asked after a *cancelled* request, never a *finished*
    one. The cell now tracks `delivered` and asks again whenever it is shown without an image
    (`PreviewRequest.isFinished`); the loader gives a pending render one more pass after its
    event stream ends and logs every flight that ends empty under `dev.tessera.app`/`thumbnails`
    (there was no log line for this case before, which is why the owner's screenshot had no
    trace). The exact engine-side reason on the owner's shoot is not reproduced here (the
    fixtures render every thumbnail); the log now names it. The Smart Preview state is a
    chip-sized corner glyph (`BadgeOverlayView.smartPreviewGlyphFrame`, tooltip = the text) and
    the row header has a secondary **Review** button (`SecondaryHeaderButton`).
    `ThumbnailRetryTests`; evidence `after/02-cull-grid.png` (the fixtures carry no Smart
    Previews, so the glyph is covered by the layout test).
24. **Raw error text in menus.** Library ▸ Smart Previews shows
    `TesseraFFI.BridgeError.Failure(message: "…")`: `BridgeError` needs `LocalizedError`
    (`errorDescription` = the message) and every surface shows `localizedDescription`.
    *Done:* the generated binding already conformed, with `errorDescription` =
    `String(reflecting:)`; `crates/tessera-ffi/uniffi.toml` now sets
    `omit_localized_error_conformance = true` (the checked-in `TesseraFFI.swift` is edited to
    what the generator emits with it; the next `build-ffi.sh` produces the same) and
    `TesseraCore/BridgeError+LocalizedError.swift` supplies the conformance with the message.
    Audit: `CullShutdownQueue` showed `String(describing:)`, now `localizedDescription`;
    `EngineCall.isBusy` matches the `Busy` case directly. `BridgeErrorDescriptionTests` also
    scans the sources for a `"\(error)"` shown in a status, toast or `Text`.
25. **Mode segmented control ignores raw clicks** (AX press works): the Cull · Edit · Deliver
    control needs a hit target the size of its label row, and a hover state.
    *Done:* each segment of `SegmentedPicker` is now the full row inside the track (the hit
    target and the fill are the same rectangle) and shows the hover fill with primary ink. The
    raw-click report could not be reproduced in-process (there is no pointer in the harness);
    if it persists on the owner's Mac the next thing to check is the macOS 26 toolbar item's
    own hit testing of the principal item.

## UX-16: owner's second pass on 0.7.0 (2026-10-16, screenshots on the sample shoot)

26. **Burst strip badges do not update after a decision.** After "Kept DSC02693.ARW, rejected 1
    in G3" both frames still show the red X badge and the kept one also shows "Pick"; the cell
    badge must follow the decision at once (the toast and column counts did).
27. **"Sharpness" pill in the loupe footer is a bare meter** (a short bar with no number); the
    owner could not tell what it was. Show the value ("Sharpness 0.12") like Score, or the
    word with a level ("Sharpness low"), and give every pill a `.help`.
28. **Toasts overflow:** "…inal sidecars differ from the offline baseline; both copies reta…" is
    cut at both ends. Toasts wrap to at most two lines inside a max width (`Theme.Width.toast`),
    never clip; long detail goes behind a "Details" affordance.
29. **That conflict should not have fired.** The offline preview for the photo was clean (no
    journaled edits); the sidecars changed because the owner edited the original. Reconnect sync
    must only report a conflict when the journal is dirty; a clean preview whose baseline moved is
    refreshed (rebuilt or re-baselined) silently.
30. **⇧⌘Z in Edit ran the agent's "redo" instead of the develop history:** "Redo finished for 0
    photos; 1 failed · DSC02506.ARW: redo intent cannot be safely scoped; specify warmth, tint,
    exposure or contrast". With a Develop session open, Edit ▸ Redo (and Undo) is the develop
    history; the agent's re-run lives under its own command and never under ⇧⌘Z.
31. **Crop overlay picker (Grid / Thirds) draws nothing** on the loupe while cropping.
32. **Edits history after "Original":** clicking Original works, but no earlier step can be
    clicked again to return; the readout says "0 steps · saved" while six steps are listed. The
    list is a history: any row selects that state, the readout counts the steps in the list, and
    the current row is marked.
33. **Slider response (PL-1, engine):** the owner reports that apart from Exposure, ±100 on
    Shadows, Highlights, Contrast and Vibrance feels like about ±20 in Lightroom. Calibrate the
    native pipeline's response so ±100 spans a comparable range (see PL-1 brief).
34. **Deliver: the Panel toggle opens an empty pane.** With Deliver up and the column hidden,
    clicking the toolbar Panel toggle shows an empty column instead of What · How · Where
    (`DeliverView` shows `DeliverColumn` on `showInspector`; something else is presenting an
    empty pane — the system `.inspector` or the split view's sidebar). Reproduce in-process
    (the Deliver evidence harness) and fix; the owner saw it at 2000 × 1257 after Cull → Edit →
    Deliver.
35. **"Deliv…" truncates in the mode control** at 2000 pt wide: segments take their label's
    intrinsic width (UX-15 item 25 made them full-row buttons; keep the hit target, fix the width).
36. **The window sits under the Dock** (the agent field and the Try cards were behind it in the
    owner's screenshots): on first launch and on zoom the window frame is constrained to
    `NSScreen.visibleFrame`; a saved frame that overlaps the Dock is pulled up on restore.
