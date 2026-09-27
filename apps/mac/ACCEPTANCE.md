# M1-09 / M1-10 acceptance script: culling and developing on the engine

For a computer-use verifier. Run every command from the **repository root** in one terminal session
(the `SCR` variable is reused). Quote paths if the checkout path contains spaces or a colon. Take a
screenshot **of the Tessera window only** at each step marked 📸. Pass criteria: every "Expect" holds.
Record any deviation together with its screenshot.

Notes:
- The app captures single-key culling shortcuts. Before pressing keys, click once on a grid thumbnail so
  the Tessera window is the key window. ⌘ shortcuts go through the menu bar as usual.
- "Cell N" means the N-th thumbnail in reading order (left to right, top to bottom).
- The status bar is the thin row above the filmstrip. Left: `<pos> of <total>   G<g> · <i>/<n>[ · suggested best]   <state>`,
  then a message. Right: `Keep k  Reject r   Basket → <album> b   Auto-advance on|off`.
- Grid cells show: decision pill top-left (REJECT / KEEP / GOOD 2 …), an outlined green **SUGGESTED** pill on the
  group's suggested best frame (groups of 2+ only), the mark chip top-right, the basket-target album name
  bottom-left in blue, and derived status bottom-right (EDITED / EXPORTED / PUBLISHED, and other albums as
  `IN <ALBUM>`). Unedited images show no status pill; the inspector's IMAGE panel shows `Status: Unedited`.
- Everything below works on **scratch copies**. Never open `fixtures/raw` itself: the app writes sidecars
  (`.edits/`, `.xmp`) and `library.json` next to the photos.

## A. Build, data and launch

1. Build the bridge and the app:
   ```sh
   export CARGO_TARGET_DIR="$HOME/.cache/tessera-target/verify"
   (cd apps/mac && ./build-ffi.sh && swift build && swift test && Support/make-app.sh release)
   ```
   Expect: `swift test` reports `Executed 97 tests, with 0 failures` (XCTest, all suites) and the Swift Testing line
   `Test run with 5 tests in 2 suites passed`; the last line reads `Built …/apps/mac/build/Tessera.app`.
   Also run `cargo test -p tessera-ffi -p cull -p image-core -p library --release 2>&1 | grep "test result"`. Expect only `ok.` lines.
2. Create scratch data (a fresh folder each run; do not reuse an old path):
   ```sh
   SCR="$(mktemp -d)"
   export TESSERA_APP_DIR="$SCR/appdir"
   swift apps/mac/Support/make-sample-folder.swift "$SCR/shoot" 40
   cp -RL fixtures/raw "$SCR/raw"
   ```
   Expect: `Wrote 40 JPEGs in 16 bursts to …/shoot`, and `ls "$SCR/raw"` lists 5 RAW files.
   If `fixtures/raw` is missing, skip step 32 and note it in the verdict.
3. Reset app preferences: `defaults delete dev.tessera.app 2>/dev/null; true`.
4. Launch (the app measures real quality signals for the defect sweep in the background on open):
   `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/shoot"` 📸
   Expect:
   - a grid of 40 cells with coloured block-mosaic images; the window subtitle reads `40 images`
   - the status bar message starts `Opened shoot: 40 images (0 RAW), 16 groups (12 with 2+)`; an `Analyzing`
     progress strip may show above the status bar for a few seconds, then disappears
   - captions end with group labels: cells 1–2 read `G1 · 1/2`, `G1 · 2/2`; cell 3 `G2`; cell 4 `G3`;
     cells 5–8 `G4 · 1/4` … `G4 · 4/4`
   - cell 2 (SAMPLE_0002) shows the outlined **SUGGESTED** pill; cells 3 and 4 (single-frame groups) do not
   - the sidebar ALBUMS section lists **Selects** with a small blue `B` tag and count 0
   - the status bar right side reads `Keep 0  Reject 0   Basket → Selects 0   Auto-advance on`.

## M2-29. Develop a rendered JPEG in the loupe

1. **M2-29.1 — JPEG loupe.** On a scratch copy from `$SCR/shoot`, select a JPEG and press E to open the loupe, then D for Develop. 📸 Expect a non-black image, enabled sliders, and no RAW-only error.
2. **M2-29.2 — Edits.** Drag Exposure, Contrast and Tint, then release each slider. 📸 Expect both the drag preview and refined frame to update. Reset and expect the original rendering; set Exposure again and commit.
3. **M2-29.3 — Persistence.** Quit and relaunch with the same `--app-dir` and `--folder`. Reopen the JPEG; expect the Exposure value and brighter frame to persist. Check its recipe JSON for `"source_kind": "rgb"`.
4. **M2-29.4 — Export.** Export Full-size JPEG and 16-bit TIFF into a scratch folder and open both. Expect colour, orientation and edits to agree with the loupe; compare mean pixel brightness of the JPEG export against the source.
5. **M2-29.5 — Other rendered formats.** Index PNG, TIFF and a HEIC generated with `sips -s format heic input.jpg --out output.heic` in the scratch shoot; enter Develop on each. Expect sliders and non-black frames. Decode-level tests cover 16-bit and float TIFF; macOS HEIC decoding requires the default `imageio` Cargo feature.
6. **M2-29.6 — Colour and orientation.** Repeat with an embedded AdobeRGB JPEG and an EXIF-rotated JPEG. Neither may be treated as untagged sRGB or rotated twice.
7. **M2-29.7 — Masks.** On the JPEG, create a linear mask and adjust local Exposure; expect the masked region to change. Try AI Subject; expect a mask or an explicit model-availability error, not a RAW-only refusal.

This is a manual acceptance script; individual expectations are not claims of visual QA unless accompanied by evidence.

## B. Group navigation

5. Click cell 1. Press **⌥→**. Expect: focus (amber border) moves to cell 3, status `G2 · 1/1`.
   Press **⌥→** twice. Expect: cell 5, `G4 · 1/4`. Press **⌥↓**. Expect: cell 6, `G4 · 2/4`.
   Press **⌥↓** three times. Expect: stays on cell 8 (`G4 · 4/4`) and the message reads `Last frame in group`.
   Press **⌥↑**. Expect: cell 7.
6. Press **⌥←**. Expect: cell 4 (`G3`), the first frame of the *previous* group. Press **⌥←** until the message
   reads `First group`. Expect: focus on cell 1.
7. Press **Return** (loupe). 📸 Expect: one large image, the file name top-left, the hint line at the bottom includes
   `K keep best` and `C compare`. Press **→**: status shows `G2`. Press **←**: back to `G1 · 1/2`.
   Press **↓**: `G1 · 2/2 · suggested best`, and the loupe overlay shows
   `SUGGESTED BEST · K keeps it and rejects the rest`. Press **Esc** (grid).

## C. Keep best, reject the rest (one undoable step)

8. With focus in G1 (cell 1 or 2), press **K**. 📸
   Expect: cell 2 shows KEEP + SUGGESTED, cell 1 shows REJECT and is dimmed; focus jumps to cell 3 (next group);
   a toast at the bottom reads `Kept SAMPLE_0002.jpg, rejected 1 in G1` with an amber `Undo ⌘Z` button;
   the status bar shows `Keep 1  Reject 1`. The toast disappears after about 5 s.
9. Press **⌘Z**. Expect: both pills disappear from cells 1–2 in one step; message `Undo: 2 images`;
   `Keep 0  Reject 0`; focus returns to the frame that was focused when you pressed K.
10. Press **⇧⌘Z**. Expect: KEEP/REJECT are back (`Redo: 2 images`). Press **⌘Z** again. Expect: `Keep 0  Reject 0`.
11. Click cell 3 (single-frame group) and press **K**. Expect: nothing changes; message
    `Keep best needs a group of 2 or more frames`.

## D. Defect sweep (review, then apply)

12. Press **⇧⌘D** (or Cull ▸ Defect Sweep…). 📸 Expect a sheet titled **Defect Sweep** over the real signals
    measured on open (sharpness of the displayed preview; the samples have no faces and no clipped highlights):
    - four threshold rows, all checked: `Missed focus below 0.30`, `Soft faces below 0.30`, `Closed eyes below 0.30`,
      `Blown highlights above 0.05`
    - the counter reads `10 candidates · 10 selected` (the grain-free frames measure soft)
    - the first rows are SAMPLE_0001 `Missed focus 0.13 < 0.30`, SAMPLE_0003 `Missed focus 0.13 < 0.30`,
      SAMPLE_0009 `Missed focus 0.14 < 0.30`, each with a checkbox and a thumbnail
    - no grid cell has changed yet (the sweep is review-only).
13. Uncheck **Missed focus**. Expect: `0 candidates` and the empty state `Nothing to reject`. Check it again: `10 candidates`.
    Drag the Missed-focus slider to about 0.42. Expect: `21 candidates` (frames measuring 0.35–0.38 join); drag it back
    to about 0.30 (`10 candidates`).
14. Uncheck the checkbox of the first row (SAMPLE_0001). Expect: `10 candidates · 9 selected` and the default button
    reads **Reject 9 Frames**. Click it.
    Expect: the sheet closes, toast `Rejected 9 frames from the defect sweep`, status `Reject 9`; SAMPLE_0001 is not rejected.
15. Press **⌘Z**. Expect: `Undo: 9 images`, `Reject 0`.

## E. Compare (2-up, synced zoom/pan, choose this)

16. Click cell 5 (SAMPLE_0005, `G4 · 1/4`). Press **C**. 📸 Expect: the toolbar segment reads **Compare**;
    two panes: left `← SAMPLE_0005.jpg` with an amber outline (active), right `→ SAMPLE_0006.jpg`.
17. Scroll (or pinch) over either pane. Expect: **both** images zoom by the same amount. Drag inside a pane: both pan
    together. Press **Z**: both return to fit. Press **Z** again: both jump to 1:1 preview pixels (at least 2×). Press **Z** once more (fit).
18. Press **→**. Expect: the right pane becomes active (amber outline); the inspector shows SAMPLE_0006.
    Press **Return** ("choose this"). Expect: right caption shows `KEEP`; the left pane now shows **SAMPLE_0007**
    (the next undecided frame of the group); message `Kept SAMPLE_0006.jpg. Next challenger: SAMPLE_0007.jpg`.
19. Press **Return** twice more. Expect: after the last press compare closes back to the grid, a toast reads
    `Kept SAMPLE_0006.jpg, rejected SAMPLE_0008.jpg`; cells 5, 7, 8 show REJECT and cell 6 KEEP.
20. Press **⌘Z**. Expect: only cell 8 returns to undecided (each choice is one undo step).

## F. Basket target and albums

21. Click cell 1, press **B**. Expect: a blue `SELECTS` pill bottom-left of cell 1; sidebar **Selects** shows 1;
    status `Basket → Selects 1`; inspector IMAGE `Status: Unedited · in Selects`. B does not advance focus.
22. Choose **Cull ▸ Basket Target ▸ New Album…**, type `Portfolio`, click **Set Target**.
    Expect: status `Basket → Portfolio 0`; cell 1's blue pill is gone and a grey outlined `IN SELECTS` pill appears
    bottom-right; the sidebar lists **Portfolio** (with the `B` tag, count 0) and **Selects** (1).
23. Press **B** on cell 1 and on cell 2 (click each first). Expect: blue `PORTFOLIO` pills; `Basket → Portfolio 2`.
    Right-click **Selects** in the sidebar ▸ **Set as Basket Target**. Expect: `Basket → Selects 1`.

## G. Safe delete

24. Click **All Photos**, click cell 3, press **⌫**. Expect: nothing is removed; message
    `Delete only removes photos from an album. To remove files use Cull ▸ Delete from Disk… (⌘⌫)`.
25. Click **Portfolio** in the sidebar. Expect: the subtitle reads `Portfolio · 2 images`. Click the first cell and
    press **⌫**. 📸 Expect: toast `Removed 1 from “Portfolio”. Files were not deleted.`; the album shows 1 image.
    In the terminal, `ls "$SCR/shoot" | grep -c jpg` still prints `40`.
26. Press **⌘Z**. Expect: the album count in the sidebar is 2 again.
27. Click **All Photos**, click cell 40 (SAMPLE_0040), press **⌘⌫** (Cull ▸ Delete from Disk…).
    Expect a warning sheet `Move “SAMPLE_0040.jpg” to the Trash?` explaining that files and sidecars move to the Trash and
    that ⌘Z cannot undo it. Press **Return**. Expect: the sheet closes (Return is Cancel) and nothing changed.
28. Press **⌘⌫** again and click **Move to Trash**. Expect: toast/message `Moved 1 photo to the Trash`; the folder
    reopens with `39 images`; `ls "$SCR/shoot" | grep -c jpg` prints `39`.

## H. Persistence and history

29. Quit with **⌘Q** and relaunch without the flag: `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/shoot"`.
    Expect: the decisions from steps 18–20 are still shown (cells 5 and 7 REJECT, cell 6 KEEP) and
    `Basket → Selects 1`. Press **⌘Z**: message `Nothing to undo` (undo history belongs to one session).
30. In the terminal: `ls "$SCR/shoot/.edits" | head -3; cat "$SCR/shoot/library.json"`.
    Expect: per-image JSON recipes, and `library.json` with albums `Portfolio` (2 image ids) and `Selects` (1).

## I. RAW fixtures copy and stub performance

31. Quit. `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/raw"`. 📸 Expect `Opened raw: 5 images (5 RAW), 5 groups`,
    and the IMAGE panel shows a real capture date for fuji-raf.RAF (2016), not 1970. Press **X** then **⌘Z**:
    the REJECT pill appears and goes away again.
32. Choose **Debug ▸ Load 20,000 Stub Items** (⇧⌘N) then **Debug ▸ Run Grid Scroll Benchmark** (⇧⌘B); do not touch input
    for 10 s. Expect a status message starting `Scroll benchmark PASS`. Press **⌥→** and **K** on a stub group: decisions
    and undo work on the in-memory stub too.

## J. Develop (Basic panel on the engine)

The loupe renders RAWs with the engine: the develop session writes into IOSurfaces that the Metal loupe presents.
Slider drags are coalesced to one engine call per display frame; the status bar readout `render: L<a> → L<b>, <t> ms`
is the time from the settings change to the finished level in the surface (`L2` = quarter resolution).

33. Engine latency first (no UI):
    `cargo test -p tessera-ffi --release --test develop -- --ignored --nocapture 2>&1 | grep "MP)"`.
    Expect two lines (CPU, then Metal) for the 36 MP NEF at `L2 1845×1231`; the **CPU** line's tone-only `median` and
    `p90` are below 16 ms (reference M4: median 11.7 ms, p90 12.3 ms). Metal is reported for comparison only.
34. Quit Tessera. Turn the readout on and open the RAW copies:
    `defaults write dev.tessera.app ShowRenderReadout -bool true` (in the app: **Debug ▸ Show Render Timing**, ⌥⌘T), then
    `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/raw"`. Click the **nikon-nef.NEF** cell and press **Return**. 📸
    Expect within about a second: the loupe re-renders from the engine (colours change slightly from the camera
    preview); the **HISTOGRAM** panel shows red/green/blue curves with a white luminance outline; IMAGE ▸ Size reads
    `7378 × 4924` (or `4924 × 7378`); BASIC shows `Unedited`, Temperature shows the as-shot estimate in K, and
    Texture…Saturation are enabled (they render since M2); the status bar shows `render: L…, … ms`.
35. Drag the **Exposure** slider slowly to about **+1.00** (drag anywhere on its track; ⌥ drags finely). 📸 mid-drag.
    Expect: the loupe brightens continuously while the mouse moves and the histogram shifts right. While dragging, the
    readout shows level **L2 or coarser** and a time **under 16 ms**. After release it may show one refinement
    (for example `render: L1, …`) and BASIC reads `History: Exposure +1.00`.
36. Press **⌘Z**. Expect: Exposure `+0.00`, the image darkens, message `Undo: Exposure +1.00`. Press **⇧⌘Z**: +1.00 again.
37. Drag **Temperature** a little. Expect: the white balance changes (whole image warmer or cooler); the readout time is
    larger than for Exposure (white balance reruns the colour matrices), still well under a second.
    Choose **Develop ▸ New Snapshot…** (⇧⌘S), name it `Warm`, **Save**. Click **Reset** (BASIC header). Expect all sliders
    at +0 / as-shot and the original look. Choose **Snapshots ▸ Warm**. Expect the warm, bright version back.
38. Press **Esc** (grid). Expect within about a second: the NEF cell shows the edited (bright, warm) thumbnail and an
    `EDITED` pill; IMAGE ▸ Status reads `Edited`.
39. In the terminal:
    `grep -o '"exposure":[^,}]*' "$SCR/raw/.edits/nikon-nef.json" | head -1; grep -c "Exposure2012" "$SCR/raw/nikon-nef.NEF.xmp"`.
    Expect `"exposure":1.0` (the value you set) and `1`.
40. Quit with **⌘Q** and relaunch: `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/raw"`. Click the NEF and press
    **Return**. 📸 Expect: the edit is shown (bright, warm), Exposure reads +1.00, and **⌘Z** steps back through the saved
    history (message `Undo: …`).
41. Optional automated drag: quit, then run
    `apps/mac/build/Tessera.app/Contents/MacOS/Tessera --app-dir "$SCR/appdir" --folder "$SCR/raw" --keys "return" --develop-selftest 2>&1 | grep -m1 develop-selftest`
    and quit the app once the line appears. It drags Exposure 0 → +1.5 on the first cell through the slider path. Expect
    `develop-selftest: <n> tone frames at L2, render median <m> ms, p90 <p> ms` with p90 below 16 ms (reference: median
    6.2 ms, p90 7.9 ms for the 16 MP RAF). This leaves an `Exposure +1.50` edit on that image.

## K. Library: albums, groups, smart albums, filter bar, keywords, metadata

Albums, album groups and smart albums live in `<folder>/library.json`; keywords and IPTC fields are written to each
photo's XMP sidecar. Nothing in this section deletes or moves a photo. Use a **fresh** sample folder (it has two
simulated cameras, `Sim A` on 30 frames and `Sim B` on 10):

```sh
swift apps/mac/Support/make-sample-folder.swift "$SCR/lib" 40
open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/lib"
```

The filter bar is the two rows above the grid: a search field (the saved-search grammar, e.g. `beach rating>=2 NOT
decision:reject`) with the match count, **Clear** and **Save as Smart Album…**, then the facet menus **Decision · Grade ·
Mark · Camera · Lens · Keyword · Date · Album**. Each menu lists its values with live counts (counted over every *other*
active filter). The sidebar has LIBRARY (All Photos, Not in Any Album), FOLDERS, ALBUMS (with a `+` menu) and CULLING.

42. **Create an album.** Choose **Library ▸ New Album…** (⌥⌘N), type `Portfolio`, press **Return**. 📸
    Expect: ALBUMS lists **Portfolio** with count 0 and it is selected; the subtitle reads `Portfolio · 0 images`; the status
    message reads `Created album “Portfolio”`. `grep -c '"Portfolio"' "$SCR/lib/library.json"` prints at least `1`.
43. **Add via the basket.** Right-click **Portfolio** ▸ **Set as Basket Target**. Expect: the blue `B` tag moves to Portfolio and
    the status bar reads `Basket → Portfolio 0`. Click **All Photos**, click cell 1, press **B**, click cell 2, press **B**.
    Expect: `Basket → Portfolio 2`, Portfolio shows 2, **Not in Any Album** shows 38.
44. **Group and nest by drag.** Click the ALBUMS `+` ▸ **New Album Group…**, type `Wedding`, **Return**. Drag **Portfolio** onto
    **Wedding**. 📸 Expect: Portfolio is indented under Wedding (with a disclosure triangle on Wedding); in
    `library.json` the Portfolio album has `"parent"` set to Wedding's `id`. Dropping *between* rows reorders instead of
    nesting (the order is stored in `sidebar_order`); a group cannot be dropped into itself.
45. **Filter bar with live diagnostics.** Click the search field and type `NOT decision:reject wat:x`. 📸
    Expect: `wat:x` is tinted red with a dotted underline and the message `Unsupported field "wat"` appears next to the field;
    **Save as Smart Album…** is disabled; the grid still shows 40 photos. Delete ` wat:x`. Expect: the highlight disappears and
    the count reads `40 matches`. Open **Camera ▾**. Expect: `Sim A 30` and `Sim B 10`. Choose **Sim B**. Expect: `10 matches`, the
    menu title reads `Camera · Sim B`, and **Camera ▾** still lists `Sim A 30` (a facet ignores its own filter).
    Open **Album ▾**. Expect: `Not in any album 10` and `In an album 0` (cells 1–2 are Sim A frames). Close the menu.
46. **Smart album from the filter.** Click **Save as Smart Album…**. 📸 Expect a sheet **New Smart Album**: the conditions show
    `Match all (AND)` containing a nested, red-railed `Group: none (NOT)` with `Decision is reject`, then `Camera is Sim B`;
    the rule text reads `NOT decision:reject AND camera:"Sim B"`; the footer reads `10 photos in this folder match`.
    Click at the end of the rule text and type ` OR rating>=5`. Expect: the conditions switch to a `Match any (OR)` root with
    the AND and NOT groups nested inside it (while the text is still valid), then `rating>=5` is highlighted red with
    `Rating must be an integer in 0..=3` below, and **Create** is disabled. Delete the 13 typed characters; the tree returns
    to the original shape. Name it `Sim B`, click **Create**. Expect: **Sim B** appears in ALBUMS with a hollow square,
    selected, subtitle `Sim B · 10 images`; the filter bar is cleared.
47. **Scoped smart album in a group.** Right-click **Wedding** ▸ **New Smart Album Inside…**. Expect: Location `Wedding` and
    **Only search albums in this group** checked. Replace the rule text with `decision:undecided`. Expect
    `2 photos in this folder match`. Name it `Wedding picks`, **Create**. Expect: it appears under Wedding with a `⌂` mark
    and shows exactly cells 1–2 (Portfolio's photos). Right-click it ▸ **Search Only This Group** (unchecks). Expect: it
    now shows 40 photos; toggle it back to 2.
48. **Keywords, bulk apply.** Click **All Photos**, click cell 1, ⇧-click cell 3. In the inspector's KEYWORDS panel click
    `Add keywords…`, type `beach, sunset`, press **Return**. 📸 Expect: chips `beach ×` and `sunset ×`; the keyword list shows
    `✓ beach 3` and `✓ sunset 3`; status `Added 2 keywords to 3 photos`. In the terminal
    `grep -c "beach" "$SCR/lib/SAMPLE_0003.jpg.xmp"` prints at least `1`. Click **New…** in the keyword list, type `Places`,
    **Return**; right-click **beach** ▸ **Move Into** ▸ **Places**. Expect: beach is indented under Places. Type
    `keyword:Places` in the search field. Expect `3 matches` (a parent matches its children). Clear the filter (**Clear**).
49. **IPTC edit persists to XMP.** Click cell 1 only. In METADATA type Title `Harbour at dawn` and press **Return**, Caption
    `fishing boats` **Return**, Copyright `© 2026 Test` **Return**. Expect status `Saved metadata to 1 XMP sidecar`.
    `grep -o "Harbour at dawn" "$SCR/lib/SAMPLE_0001.jpg.xmp"` prints the title. Type `boats` in the search field. Expect
    `1 match`. Quit (⌘Q), relaunch with the same command, click cell 1. Expect: Title, Caption, Copyright and the keywords
    are shown again; decisions and albums are unchanged.
50. **Safe delete everywhere.** Click **Sim B**, click a cell, press **⌫**. Expect: nothing is removed and the message reads
    `Sim B is a saved search: change its rule to change what it shows. Nothing was removed.` Right-click **Wedding** ▸
    **Delete Group…**. Expect an alert saying its items move up one level and no photos or files are deleted; **Return**
    cancels. Repeat and click **Delete**. Expect: Portfolio and Wedding picks are now top-level, Portfolio still has 2 photos,
    and `ls "$SCR/lib" | grep -c "jpg$"` prints `40`. Right-click **Places** in the keyword list ▸ **Delete from Keyword List**.
    Expect: `beach` moves to the top level and the photos keep it (`grep -c beach "$SCR/lib/SAMPLE_0001.jpg.xmp"` ≥ 1).
51. **Derived status.** Click **Not in Any Album**. Expect `Not in Any Album · 38 images`, and cells 1–2 are not shown.

## Verdict

PASS when steps 1–40 and 42–51 meet their expectations (step 32's first part may be skipped only if fixtures are missing, step 31;

## L. Develop panels (M2-13)

The panels below BASIC (Tone Curve, HSL / Color, Color Grading, Detail, Effects, Crop & Straighten, Presets,
Snapshots, History) start collapsed; click a header to open it (the state is remembered). They drive the same develop
session as BASIC: drags are coalesced per display frame and each release is one undo step. Heavy panels (curves, HSL,
grading, effects, crop) run the whole-level M2 operator chain; while dragging, the session renders at the coarsest
level that keeps frames under 16 ms (the readout shows e.g. `render: L5, 8 ms`) and refines on release.

42. Engine panel latency (no UI):
    `cargo test -p tessera-ffi --release --test develop -- --ignored --nocapture bench_panel 2>&1 | grep -E "panels|  [a-z]"`.
    Expect a block per backend listing tone exposure, parametric curve, point curve, hsl, grading, sharpening,
    vignette and straighten, each with an `L<n>` and a median under 16 ms after the level adapts (the level for
    curve/HSL/grading/effects/crop is coarser than the screen level, e.g. `L5`/`L6` for the 36 MP NEF).
43. Relaunch on the RAW copies (`open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/raw"`), select
    **sony-arw.ARW** and press **Return**. Open **TONE CURVE**. 📸 Expect the parametric curve over the luminance
    histogram, three split triangles under it and Highlights/Lights/Darks/Shadows sliders. Drag inside the curve's
    upper-middle area upwards: the Lights region highlights, the curve bows up, the **Lights** slider follows and the
    loupe brightens the upper mid-tones. Release: HISTORY (below) lists `Curve Lights +…`.
44. Click **Point**, choose **R**. Click the middle of the curve to add a point, drag it up. Expect the red curve bends
    (never below the previous point, never above the next: the editor keeps the curve monotone) and the image turns
    redder. With the point selected press **↑** three times: it nudges up; the burst becomes one history step
    `Point Curve (Red)` after a pause. Double-click the point: it is removed. **Curve Presets ▸ Strong Contrast** on
    **RGB**: an S-curve and a punchier image.
    With a point selected, arrow keys must move the point without moving the loupe selection; its amber focus outline
    stays visible until **Esc** blurs the editor.
45. Open **HSL / COLOR** ▸ **Saturation**. Drag **Blue** to −100: the sky greys. Click the target button (◎) and
    drag **up** on the cube's orange face in the loupe (cursor ↕): the Orange (and a little Red/Yellow) saturation
    sliders rise together, and the status/History read `Orange Saturation +…`. Press **Esc** to disarm. There is no
    B&W mix (the recipe schema has no field for it yet).
    Click or Tab to a slider: expect an amber focus outline. **→** nudges one step, **⇧→** ten,
    **⌥→** a fine tenth step; **Home/End** reach the limits and **Return/Esc** commit and blur.
    None of these keys move the loupe/grid selection while the slider is focused. Click the search field and type
    `xup`: expect text input, not reject/undecided/keep decisions. Blur the field and confirm loupe arrows navigate again.
46. Open **COLOR GRADING**. In **3-Way**, drag the Shadows wheel puck towards blue (lower left) and the Highlights
    puck towards orange: shadows cool, highlights warm. The wheels show the engine's OkLab hues (the puck colour is
    the colour added). Move **Balance** and **Blending**: the split moves / softens. Double-click a wheel: it resets.
    The Shadows/Midtones/Highlights/Global tabs show one large wheel with Hue/Saturation/Luminance sliders.
47. Open **DETAIL**. 📸 Expect a 1:1 preview of the image centre (`1:1 · x, y` label). Drag inside it to pan; click the
    target button and click a detailed area in the loupe: the preview moves there. Raise **Amount** to 120: the
    preview sharpens. Hold **⌥** and drag **Masking**: the loupe shows a black-and-white edge mask (white =
    sharpened) that shrinks to the edges as Masking rises; releasing ⌥/the mouse returns to the photo.
    **Luminance** 40 visibly smooths noise in the preview.
48. Open **EFFECTS**. Vignette **Amount** −60: dark corners; switch **Style** to Paint Overlay and back. Grain
    **Amount** 40: visible grain in the 1:1 preview / at 1:1.
49. Press **R** (or **CROP & STRAIGHTEN ▸ Crop & Straighten**). 📸 Expect the whole photo with a white crop box,
    thirds grid and handles, the outside dimmed. Choose **Aspect ▸ 3 : 2**, drag a corner inwards (the ratio holds),
    drag inside the box to move it, drag **outside** the box to rotate: the image turns under the fixed box and the
    box shrinks to stay inside the photo (Constrain to image), the readout shows `… × …  +2.40°`. Press **O** to cycle
    overlays, **X** to swap to portrait. Click the level button and draw along a slanted horizon: the angle levels
    it. Press **Return**: the loupe shows the cropped, straightened picture filling the view; History lists
    `Crop & Straighten +…°`. **⌘Z** restores the full frame; **Esc** in the tool discards changes.
50. **PRESETS ▸ Save Preset…**, name `Look`, leave Crop unticked, **Save**. Select the NEF, press Return and click
    **Look** in PRESETS: the NEF takes the curve/HSL/grading/effects look but not the crop (one history step
    `Preset: Look`). The file exists: `ls "$SCR/appdir/Presets/Look.json"`.
51. **SNAPSHOTS ▸ New Snapshot…** `Graded`, Save; the list shows `Graded`. Change anything, click `Graded`: back.
52. **HISTORY**: newest first, the current step highlighted, `Original` at the bottom. Click an older step: the image
    and all sliders go back to it; later steps stay listed (dimmed) until a new edit. Click the newest step again.
    Untick the checkbox of the `Blue Saturation −100` step: the sky's colour returns and a step
    `Turn Off Blue Saturation −100` appears (itself not toggleable); tick it again to turn it back on.
53. Quit and relaunch; press Return on the ARW. Expect the crop, curve, HSL, grading, detail and effects restored;
    the grid thumbnail shows the cropped edit.
54. Optional automated pass: `apps/mac/build/Tessera.app/Contents/MacOS/Tessera --app-dir "$SCR/appdir" --folder "$SCR/raw" --develop-panels-selftest 2>&1 | grep -m1 develop-panels-selftest`.
    It opens the first photo in the loupe and drags one control of each panel through the slider path; expect a line
    `develop-panels-selftest: tone curve … L… median … ms; hsl …; grading …; detail …; vignette …; grain …`.

## Verdict

PASS when steps 1–40 and 42–53 meet their expectations (step 32's first part may be skipped only if fixtures are missing, step 31;
steps 33–41 need the RAW fixtures).
Report the command outputs from steps 1–2 and 33, the 📸 screenshots, the benchmark line and any readout values. Afterwards you may delete
`$SCR` and reset preferences with `defaults delete dev.tessera.app`.

## M. Masks and local adjustments (M2-14)

Masking works on RAWs in the loupe. **M** (or the **Masks** button at the loupe's top right) shows the mask toolbar
above the photo and opens the **MASKS** panel under BASIC. Every mask edit drives the same develop session as the
sliders: drags are coalesced per display frame and each release is one undo step. AI masks (Subject, Sky, Background,
People, Objects) run on this Mac: the first use loads the pinned segmentation weights (U²-Net and MobileSAM, about
220 MB) into `$SCR/appdir/models/cache`, downloading them when missing, so the first AI mask
needs the network and takes a while; later ones take a few seconds. To use weights fetched ahead of time
(`python3 tools/segment_models.py fetch --registry-cache "$SCR/segment-registry"`, see crates/ml-segment/README.md),
launch with `open -n --env TESSERA_SEGMENT_MODELS="$SCR/segment-registry" apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" …`.
The Sky mask is the documented phase-one heuristic (top-connected blue sky with the subject removed): blue sky is
selected, grey clouds only partly, and it is not a semantic sky network.

55. Engine and app tests (no UI): `cargo test -p tessera-ffi --release --test masks 2>&1 | grep "test result"` and
    `(cd apps/mac && swift test --filter MaskingTests 2>&1 | grep Executed)`. Expect `test result: ok. 3 passed; 0 failed; 1 ignored` and
    `Executed 8 tests, with 0 failures`. (The `subject_mask_on_the_canon_fixture_with_cached_models` test only runs
    the real models when `TESSERA_SEGMENT_MODELS` or the M3-04 cache exists; otherwise it prints `SKIP offline`.)
56. Relaunch on the RAW copies (`open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/raw"`), select
    **canon-cr3.CR3** (the tomato on the wooden table) and press **Return**; wait for the develop frame. Press **M**.
    📸 Expect the mask toolbar centred at the top of the loupe (brush, linear, radial, colour range, luminance range |
    Subject, Sky, Background, People, Objects | eye and overlay-colour dot | Done) and the MASKS panel open with
    `No masks.` and a **Masking** switch that is on.
57. **Subject mask on the tomato.** Click **Subject** in the toolbar. Expect a progress bar under the toolbar
    (`Preparing image`, `Loading segmentation models`, `Segmenting`) and a new `Mask 1` row with a subject icon and a
    spinner; then the status bar reads `Subject mask ready`. 📸 With the overlay on (default, red at 50 %), the tomato
    (and its stem) is tinted red and the table is not; the row's thumbnail shows a white blob on black. Drag
    **Exposure** in the panel to +1.00: only the tomato brightens, the table stays; on release HISTORY lists
    `Mask 1: Exposure +1.00`. Press **O**: the overlay hides (the brighter tomato stays); **⇧O** switches the overlay
    colour to green and shows it again. Press **X**: the mask inverts (now the table brightens and is tinted), the row
    reads `inverted`; press **X** again.
58. **Brush + local exposure.** Click the brush tool. The HUD shows Size/Feather/Flow; move the pointer over the photo:
    a circle (and a dashed inner feather circle) follows it; **]** grows it, **[** shrinks it (⇧ changes feather).
    Paint a stroke across the lower table. Because the selected Mask 1 has no brush, the stroke starts a new
    `Mask 2` (brush icon) and becomes selected; the overlay follows the stroke within a frame or two. HISTORY shows one
    `Brush Stroke` step per stroke. Drag its **Exposure** to −1.00: only the painted band darkens. Hold **⌥** and
    paint over half the band: the cursor shows a minus and that part is erased (step `Brush Erase`); the darkening
    there disappears.
59. **Gradients and ranges.** Select the **linear** tool and drag from the top edge to the middle of the photo: three
    lines follow (full effect, midpoint, none) and a `Mask 3` appears; set **Temp** −40: the top turns blue fading out
    towards the middle. Drag its end knob: the gradient changes and history adds one `Edit Linear Gradient` step. In
    MASKS choose **Subtract ▸ Color Range** and click the tomato: the tomato is excluded from the gradient (component
    list shows `Linear Gradient` then `− Color Range`). ⇧-click another red spot adds a sample
    (`Color Range (2 samples)`).
60. **Undo.** Press **⌘Z** repeatedly: the steps come off in reverse order (the colour-range subtraction, the
    gradient edit, Temp, the gradient, the erase, the brush exposure, the stroke, …) and the MASKS list follows each
    step; **⌘⇧Z** redoes them. Quit and relaunch on the same folder, open the CR3 in the loupe and press **M**: the masks,
    their sliders and the rendered result are restored (the Subject raster is recomputed from the model cache).
61. **Sky on the Fuji RAF.** Select **fuji-raf.RAF** (the harbour with white houses), press **Return**, **M**, then
    click **Sky**. Expect the blue sky at the top tinted, the houses, rocks and water not (clouds partly, see above).
    Set **Dehaze** +40 and **Exposure** −0.50: the sky deepens while the foreground is unchanged. Choose **People**
    and drag a box around a house's front: an `Object`-style `Person` mask appears (the promptable model segments
    the boxed thing; it is a whole-person proxy, not part parsing). Choose **Objects** and click the right house: an
    `Object` mask selecting that house.
62. Optional automated pass: `apps/mac/build/Tessera.app/Contents/MacOS/Tessera --app-dir "$SCR/appdir" --folder "$SCR/raw" --masks-selftest 2>&1 | grep -m1 masks-selftest`.
    It opens the first photo, creates a linear gradient by dragging it, drags its local Exposure, paints a brush
    stroke and drags the stroke's Saturation, each through the per-frame path; expect a line
    `masks-selftest: linear gradient … median … ms; local exposure …; brush …; brush saturation …; masks 2; backend …`.
    Local adjustments render on the whole-level chain like the heavy panels: during a drag the session drops to the
    level that keeps frames near 16 ms (e.g. `L5`) and refines on release. Engine-only numbers:
    `cargo test -p tessera-ffi --release --test masks -- --ignored --nocapture bench_mask` prints local exposure,
    gradient handle and brush batch medians (expect each under 16 ms at its `L<n>`).

## Verdict (masks)

PASS when steps 55–61 meet their expectations. Steps 57 and 61 need the segmentation weights (network on first use,
or `TESSERA_SEGMENT_MODELS`); if neither is available, record the `Loading segmentation models` failure message shown
on the component (it offers **Retry**) and judge the remaining steps.

## N. Lightroom Classic import (M2-13b)

File ▸ Import Lightroom Catalog… walks a `.lrcat` through summary → mapping → fidelity preview → import → report
(docs/05 §3, docs/06 §2.1). This section uses the synthetic catalog from `crates/import-lrcat` (real Lightroom
catalogs are not in the repository). It records its photo root as `/Volumes/Old Drive/Photos/`, a drive that is
not mounted, so the photos must be **relocated**; it has 6 photos (one of them, `lost-01.jpg`, missing on disk),
1 virtual copy, stars/flags/colour labels, a keyword tree, collections, a smart collection Tessera cannot run,
and a `Previews.lrdata` cache. Its "Lightroom previews" are approximations written by the fixture, not Adobe
renders, so the ΔE values below only show that the comparison works.

63. **Fixture and tests.**
    ```sh
    cargo run --release -p tessera-cli --bin tessera -- --app-dir "$SCR/lr-cli" import lrcat --make-fixture "$SCR/lr"
    (cd "$SCR/lr/Catalog" && find . -type f | sort | xargs shasum) > "$SCR/lr-catalog.sha"
    cargo test -p tessera-ffi -p import-lrcat --release 2>&1 | grep "test result"
    (cd apps/mac && swift test --filter LightroomImportTests 2>&1 | grep Executed)
    ```
    Expect: JSON with `catalog` (`…/lr/Catalog/Fixture.lrcat`), `photos`, `previews` and
    `"moved_root": "/Volumes/Old Drive/Photos/"`; only `ok.` lines (the `lrcat` suite: `5 passed`); `Executed 8 tests,
    with 0 failures`.
64. **Summary.** `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/shoot"`. Choose **File ▸ Import Lightroom
    Catalog…** (⇧⌘I), click **Choose Catalog…**, press ⇧⌘G, paste the `catalog` path, **Return**, **Choose**. 📸
    Expect the sheet **Import Lightroom Catalog** with the step line `Catalog › Summary › Mapping › Fidelity › Report`
    (Summary bold) and: Photos 6, Virtual copies 1, Folders 3, With develop edits 4, Keywords 7, Collections 2,
    Collection sets 1, Smart collections 2, Stacks 1, Faces 1, Lightroom previews 6, Disk space needed ≈ 140 KB.
    **Not fully supported (8)** lists, each with a reason: Catalog (optional tables not in this catalog, 2),
    Develop settings (`crs:FutureKnob: unsupported property; source preserved`, ceremony-01.jpg), Virtual copies
    (`ceremony-01.jpg (Black & White)`), Stacks, Faces (portrait-01.jpg), History (3), Smart collections (`Blue label`:
    `unsupported field "labelColor"`), Keywords (`“Paris” appears 2 times…`). No lock warning is shown.
    (Optional: close the sheet, `touch "$SCR/lr/Catalog/Fixture.lrcat.lock"`, reopen the catalog: an amber lock line
    appears; `rm` the file afterwards.)
65. **Mapping before relocation.** Click **Continue**. 📸 Expect: Library folder `/Volumes/Old Drive/Photos`; the root
    row `/Volumes/Old Drive/Photos/` with an amber folder icon, `0/6 found` and **Locate…**; the folder table has three
    rows marked `not found`; the footer reads `0 photos will be imported · 6 missing · 0 skipped · 1 virtual copy kept in
    the bundle`. **Selection mapping** reads, top to bottom: `Rejected → Reject 1`, `Picked, 5 stars → Keep · Grade 3
    (Best) 1`, `Unflagged, 4 stars → Keep · Grade 2 (Good) 1`, `Unflagged, 3 stars → Keep · Grade 2 (Good) 1`,
    `Unflagged, 2 stars → Keep · Grade 1 (Keep) 1`, `Unflagged, no stars → Undecided 1`, then
    `4 Keep · 1 Reject · 1 Undecided · 3 marked`. **Colour labels → marks** lists `Client 1 photo` and `Red 2 photos`, both
    `Keep “…”`. **Keyword hierarchy** shows Places ▸ NYC (New York) 2, Paris 0; People ▸ Alice 1; Trips ▸ Paris 1 with an
    amber `merged` tag.
66. **Relocate the moved drive.** Click **Locate…**, press ⇧⌘G, paste `$SCR/lr/Photos` (expanded), **Return**, **Locate**.
    📸 Expect: a green check, `→ …/lr/Photos`, `5/6 found`, **Change…** and **Reset**; the library folder follows to
    `…/lr/Photos`; the folder table shows `Photos/2026` (0), `Photos/2026/portraits` (3 photos, Missing 1 in red) and
    `Photos/2026/wedding` (3 photos, Copies 1); the footer reads `5 photos will be imported · 1 missing · 0 skipped · 1
    virtual copy kept in the bundle`. Nothing has been written yet: `ls -a "$SCR/lr/Photos/2026/wedding"` lists only the
    three `.jpg` files.
67. **Mark names.** Set **Red** to **Needs Retouch** and **Client** to **No mark**. Expect the selection summary to read
    `4 Keep · 1 Reject · 1 Undecided · 2 marked`.
68. **Fidelity preview.** Click **Preview Fidelity**. 📸 Expect (after a moment) the caption `Rendered with Tessera's
    native pipeline (the Adobe-compatible renderer is not available yet)…`, 6 pairs labelled `Lightroom` | `Tessera`,
    sorted **Largest difference** first: `portrait-01.jpg` with a red badge ≈ `ΔE 5.5 · p95 12.7`, the others amber or
    green (mean ΔE ≈ 1.7–2.7), including `ceremony-01.jpg (Black & White)` rendered in greyscale. The checkbox reads
    `Looks different (1)`; tick it: only portrait-01.jpg remains. Choose sort **Name**: pairs are alphabetical.
    (Exact values depend on the native pipeline; the order and the single "looks different" pair are the check.)
69. **Import (non-modal) and report.** Click **Import 5 Photos**. The sheet closes; the import finishes within a second and
    the sheet returns on **Report**. 📸 Expect `Imported 5 photos into Photos.`; Photos written 5, Resumed 0, Albums 2,
    Album groups 1, Smart albums 2, Keywords 6, Skipped 1, Virtual copies (bundle) 1; Skipped lists `lost-01.jpg` —
    `original not found (relocate its folder if the drive moved)`; `Full report: …/lr/Photos/import-report.md`. Behind the
    sheet the window has opened **Photos** (`5 images`), the status message reads `Imported 5 photos from Fixture.lrcat`
    and the status bar `Keep 3  Reject 1`. Click **Done**. Expect ALBUMS: **Wedding** ▸ **Selects** 2, **Four stars and up**
    (smart), **Client review** 2, **Blue label** (smart); ceremony-01 and portrait-01 carry mark 6 (Needs Retouch) chips;
    ceremony-02 is REJECT.
70. **What was written, and what was not.**
    ```sh
    grep -E "^## |Photos with edits|Relocated folders|lost-01|looks different" "$SCR/lr/Photos/import-report.md"
    grep -c '"lightroom"' "$SCR/lr/Photos/library.json"
    grep -o "Places|NYC" "$SCR/lr/Photos/2026/wedding/ceremony-01.jpg.xmp"
    ls "$SCR/lr/Photos/.tessera-import"/*/
    (cd "$SCR/lr/Catalog" && find . -type f | sort | xargs shasum) | diff - "$SCR/lr-catalog.sha" && echo catalog unchanged
    ```
    Expect: the report headings `## Imported`, `## Skipped (2)`, `## Not fully supported (8)`, `## Fidelity preview (native
    renderer)`, the lines for 5 written photos, the relocation `/Volumes/Old Drive/Photos/` → `…/lr/Photos`, lost-01 and
    `portrait-01.jpg … looks different`; `2` (the two imported albums are tagged with their catalog); `Places|NYC`;
    `import-plan.json  state.json`; `catalog unchanged` (the catalog and `Previews.lrdata` were only read).
71. **Re-import resumes, nothing is duplicated.** ⇧⌘I, choose the same catalog, Continue, Locate the same folder, set the
    same marks as in step 67, Preview Fidelity, **Import 5 Photos**. Expect the report `Photos written 0`, `Resumed 5`,
    `Albums 0` and ALBUMS unchanged (one Selects, one Client review). (With different settings the importer rewrites its
    own sidecars instead, `Photos written 5`; albums are still not duplicated.)
72. **Cancel and resume, while the window stays usable.** Create a second fixture and slow the import down (test aid):
    ```sh
    cargo run --release -p tessera-cli --bin tessera -- --app-dir "$SCR/lr-cli" import lrcat --make-fixture "$SCR/lr2"
    open -n --env TESSERA_LRCAT_IMPORT_DELAY_MS=3000 apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/shoot" \
      --import-lrcat "$SCR/lr2/Catalog/Fixture.lrcat"
    ```
    The sheet opens on Summary. Continue, Locate `$SCR/lr2/Photos`, Preview Fidelity, **Import 5 Photos**. 📸 Expect a strip
    above the status bar: `Lightroom import · Writing edits`, a progress bar, `1 / 5`, a file name and **Cancel Import**;
    the grid behind it still scrolls and accepts clicks. Click **Cancel Import**. Expect the sheet to return with
    `Import cancelled. Resume to continue where it stopped; finished photos are skipped.`, Albums 0, and **Resume Import**;
    `grep Status "$SCR/lr2/Photos/import-report.md"` shows `cancelled` and `ls "$SCR/lr2/Photos"` has no `library.json`.
    Click **Resume Import**: the strip returns; when it finishes the report reads `Imported 5 photos…` with Resumed equal
    to the number written before the cancel, and `$SCR/lr2/Photos/library.json` exists.

## Verdict (Lightroom import)

PASS when steps 63–72 meet their expectations. Record the fidelity values seen in step 68 (they are informational).

## O. Export, soft proofing and print (M2-20)

File ▸ Export… (⇧⌘E) exports the selection or the current album with a preset and editable settings; it runs
without a sheet (progress strip with Cancel) and ends in a results toast. Develop ▸ Soft Proofing (S in the loupe)
simulates a printer profile in the loupe, with a gamut warning (⇧S). File ▸ Print… (⌘P) lays out single pages,
contact sheets or custom cells, renders every photo with the engine at the print resolution, and prints, saves a
PDF or saves JPEG pages. Exports of full RAWs render on the CPU reference pipeline: allow up to a minute per photo
on large files (the Web preset renders at half size and is faster).

73. **Tests.**
    ```sh
    cargo test -p tessera-ffi -p export -p color-mgmt --release 2>&1 | grep "test result"
    (cd apps/mac && swift test --filter ExportPrintTests 2>&1 | grep Executed)
    ```
    Expect only `ok.` lines (the tessera-ffi `export` suite: `8 passed`; export `orientation`: `2 passed`;
    color-mgmt `printer`: `2 passed`) and `Executed 9 tests, with 0 failures`.
74. **Scratch RAWs.**
    ```sh
    mkdir -p "$SCR/raw3" && cp -L fixtures/raw/nikon-nef.NEF fixtures/raw/sony-arw.ARW fixtures/raw/canon-cr3.CR3 "$SCR/raw3/"
    open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir" --folder "$SCR/raw3"
    ```
    Expect `Opened raw3: 3 images (3 RAW)`.
75. **Export sheet.** Click a cell, press **⌘A**, choose **File ▸ Export…** (⇧⌘E). 📸 Expect the sheet **Export** with
    `Selected photos (3)` top right, Preset **Web 2048 sRGB** and the summary `2048 px long edge · JPEG 85 · sRGB · 72 dpi`;
    sections Location (folder `…/Pictures/Tessera Export`, `If a file exists: Add a number`, `After export: show in
    Finder`), File Naming (template `{name}`, buttons `{name}` `{seq}` `{date}`, `Example: <first photo>.jpg, …`), File
    Settings (JPEG selected, Quality 85, Colour space sRGB), Image Sizing (Long edge 2048 pixels, Resolution 72, Upscale
    Off), Output (Sharpen for Screen, Metadata All metadata). The button reads **Export 3 Photos**.
76. **Presets and fields.** Open the Preset menu: `Custom`, then **Web 2048 sRGB, Full-size JPEG, 16-bit TIFF ProPhoto,
    Print 300 dpi**. Choose **16-bit TIFF ProPhoto**: the summary reads `Full size · TIFF 16-bit · ProPhoto RGB · 300 dpi`
    and File Settings shows TIFF with Bit depth 16-bit. Choose **Print 300 dpi**: Long edge `12` **inches**, summary
    starts `12 in long edge · JPEG 95`. Choose **Web 2048 sRGB** again. Click `{seq}` after the template: the example
    becomes `<name>1.jpg, …`; replace the template with `{nme}`: the example turns red, `Unknown token {nme} …`, and the
    Export button is disabled. Set the template back to `{name}`. Change Quality: the preset menu shows **Custom**.
    Choose **Web 2048 sRGB** once more.
77. **Export (non-modal).** Click **Choose…**, press ⇧⌘G, paste `$SCR/web` (expanded; create it with **New Folder** if
    needed), **Choose**. Click **Export 3 Photos**. 📸 Expect the sheet to close and a strip above the status bar:
    `Exporting · Selected photos`, a progress bar, `0 / 3` … `2 / 3`, the current file name and **Cancel Export**; the
    grid still scrolls and accepts clicks. When it finishes: a toast `Exported 3 photos to web in … s`, the status
    message says the same, and the three cells show the **EXPORTED** status pill.
78. **Files and dimensions.**
    ```sh
    ls "$SCR/web"
    for f in "$SCR"/web/*.jpg; do sips -g pixelWidth -g pixelHeight -g dpiWidth "$f" | tail -3 | tr '\n' ' '; echo; done
    ls "$SCR/appdir/ExportPresets"
    ```
    Expect `canon-cr3.jpg nikon-nef.jpg sony-arw.jpg` (each with a `.jpg.xmp` metadata sidecar); sizes
    `2048 × 2048` (the CR3 fixture is square), `2048 × 1367` and `2048 × 1364`, all `dpiWidth: 72`; the presets folder
    lists four `.json` files.
79. **Conflicts, cancel and failures.** ⇧⌘E, **Export 3 Photos** again. Expect `canon-cr3-2.jpg` etc. (nothing is
    overwritten). ⇧⌘E once more and click **Cancel Export** while the strip shows `0 / 3` or `1 / 3`: the toast reads
    `Export cancelled: <n> photos written to web`, and no partial files remain (`ls -A "$SCR/web" | grep -c '^\.tmp'`
    prints 0).
    Now set **If a file exists** to **Skip the photo** and export again: the toast reads `Exported 0 photos to web; 3
    failed` and lists `nikon-nef.NEF: nikon-nef.jpg already exists` (one line per photo) in red, with **Dismiss**.
80. **Soft proofing.** Click **nikon-nef.NEF**, press **Return** and wait for the develop render. Press **S**. 📸 Expect
    the loupe's top-right to read `SOFT PROOF · <profile>` (the first installed printer profile, e.g. `Generic CMYK
    Profile`) and the picture to look duller (the simulated print). Expand the inspector's **SOFT PROOFING** panel: the
    toggle is on and the status reads `Proofing <profile> · <n>% of colours out of gamut`. Press **⇧S**: saturated
    colours that the printer cannot reproduce (the sky, the pink cloud) turn **magenta**, and the badge adds
    `GAMUT WARNING`; change the warning colour in the panel: the overlay follows. Tick **Simulate paper and ink**: white
    turns slightly grey. Press **S** again: the normal rendering returns. The History panel gained no entry, and
    **Develop ▸ Soft Proofing** is unchecked.
81. **Print to PDF: contact sheet.** Press **Esc**, **⌘A**, **File ▸ Print…** (⌘P). 📸 Expect the sheet **Print ·
    Selection · 3 photos** with a page preview on the left, `Paper <name> · 8.50 × 11.00 in · Portrait` (or A4) and
    **Page Setup…**, the Layout segments **Single image | Contact sheet | Custom cells**, margins in inches, Rotate to fit,
    Resolution, Print sharpening, JPEG pages at, and Colour handling. Choose **Contact sheet**: Rows 5, Columns 4,
    `20 per page · 1 page`, and the preview shows three thumbnails with their file names. Set Rows **1** and Columns
    **2**: `2 per page · 2 pages`; the arrows under the preview step through `Page 1 of 2` and `Page 2 of 2`. Click
    **Save as PDF…**, save as `$SCR/sheet.pdf`. Expect the strip `Saving PDF · rendering for print`, `1 / 3` …, then the
    toast `Saved 2 pages to sheet.pdf`.
    ```sh
    swift apps/mac/Support/pdf-page-count.swift "$SCR/sheet.pdf"
    sips -s format png "$SCR/sheet.pdf" --out "$SCR/sheet.png" && open "$SCR/sheet.png"
    ```
    Expect `2 pages, 8.5 × 11.0 in` (or `8.3 × 11.7 in` on A4) and a first page with two developed photos turned to fit
    their cells, each captioned with its file name.
82. **Print to file (JPEG) and application-managed colour.** ⌘P, set **JPEG pages at** 150 dpi, **Save as JPEG…** as
    `$SCR/pages.jpg`. Expect the toast `Saved 2 JPEG pages at 150 dpi` and
    `sips -g pixelWidth -g dpiWidth "$SCR/pages-1.jpg"` → `1275` (Letter; `1240` on A4) and `150`. ⌘P again, choose
    **Tessera manages colour**, Profile `Generic CMYK Profile (CMYK)`, Intent Perceptual, **Save as PDF…** as
    `$SCR/cmyk.pdf`: the PDF is written (`swift apps/mac/Support/pdf-page-count.swift "$SCR/cmyk.pdf"` → `2 pages`).
    **Print…** opens the system print dialog as a sheet on the main window (its PDF menu works too); click Cancel.
83. **Headless variant (optional).** Quit, then
    ```sh
    apps/mac/build/Tessera.app/Contents/MacOS/Tessera --app-dir "$SCR/appdir-st" --folder "$SCR/raw3" \
      --export-selftest "$SCR/web-st" --print-pdf-selftest "$SCR/st.pdf" 2>&1 | grep selftest
    ```
    Expect `export-selftest: 3 exported, 0 failed, cancelled no, …` and `print-selftest: ok, 1 page(s) expected → …/st.pdf`.

## Verdict (export, soft proof, print)

PASS when steps 73–83 meet their expectations (step 83 is optional). Record the export time of step 77.

## P. HDR / EDR presentation (M2-22)

On an EDR-capable screen (Liquid Retina XDR, Pro Display XDR, or an external HDR display with HDR on in System
Settings ▸ Displays), the inspector's **HDR** panel switches the loupe to RGBA16F display-linear frames: highlights
brighter than SDR white use the screen's EDR headroom. The **Headroom** slider runs from 0 EV (the SDR tone curve)
to the screen's potential (`log2` of `maximumPotentialExtendedDynamicRangeColorComponentValue`, 4.0 EV on XDR
panels); the engine tone-maps for `min(2^EV, current headroom)`, where the current headroom follows the display
brightness. SDR screens keep the RGBA8 path byte for byte; the HDR setting is still stored in the recipe (and XMP
`crs:HDREditMode` / `crs:HDRMaxValue`). Exports, previews, the 1:1 detail crop and prints stay SDR.

**First record the screen:** save and run
```sh
cat > "$SCR/edr.swift" <<'SWIFT'
import AppKit
for s in NSScreen.screens { print(s.localizedName, "now", s.maximumExtendedDynamicRangeColorComponentValue,
                                  "max", s.maximumPotentialExtendedDynamicRangeColorComponentValue) }
SWIFT
swift "$SCR/edr.swift"
```
and write the output into the verdict. `max 1.0` means an SDR screen: do steps 84–85 and 88–89, and step 86 only
through the forced variant (step 87).

84. **Tests.**
    ```sh
    cargo test -p tessera-ffi -p pipeline-gpu -p color-mgmt -p pipeline-cpu --release 2>&1 | grep "test result"
    cargo test -p pipeline-gpu --release --test hdr_surface 2>&1 | grep "test result"
    cargo test -p tessera-ffi --release --test hdr 2>&1 | grep "test result"
    (cd apps/mac && swift test --filter EDRPresentationTests 2>&1 | grep Executed)
    ```
    Expect only `ok.` lines; `hdr_surface`: `4 passed` (includes the SDR fingerprint test: SDR output bit-identical
    to the pre-M2-22 tree); `hdr`: `3 passed; 0 failed; 1 ignored`; `Executed 6 tests, with 0 failures`.
85. **Float-path frame time.**
    `cargo test -p tessera-ffi --release --test hdr -- --ignored --nocapture bench 2>&1 | grep -E "RGBA"`. Expect four
    lines (Metal and CPU × RGBA8 SDR / RGBA16F EDR) at L2 of the NEF; the Metal `RGBA16F EDR` median and p90 stay
    below **12 ms** and within ~1 ms of the `RGBA8 SDR` line. Record the numbers.
86. **HDR in the loupe (EDR screen).** Open the NEF of step 74 in the loupe (click, **Return**) and wait for the
    render. Expand **HDR** in the inspector. 📸 Expect the toggle off and the status `EDR <now>× now, <max>× max`.
    Turn **HDR (extended dynamic range)** on. Expect the Headroom slider enabled at its maximum (e.g. `4.0 EV`), the
    status adding `· showing <n>× (RGBA16F)`, the colour readout under the loupe ending `engine HDR <n>×`, and
    specular highlights / bright sky visibly brighter than the white of the UI around the loupe. Drag Headroom to
    `0.0 EV`: the picture returns to the SDR look (nothing brighter than UI white); drag back up: highlights brighten
    smoothly while dragging. The History panel gains `HDR On` and one `HDR Headroom …` step; ⌘Z steps back.
    Turn HDR off: the readout ends `engine SDR`.
87. **Forced EDR on an SDR screen (optional).** Quit, then
    ```sh
    open -n --env TESSERA_EDR_OVERRIDE=4,16 --stderr "$SCR/hdr.log" apps/mac/build/Tessera.app \
      --args --app-dir "$SCR/appdir-hdr" --folder "$SCR/raw3" --keys return --hdr-selftest
    sleep 20; grep hdr-selftest "$SCR/hdr.log"
    ```
    Expect `hdr-selftest: EDR 4.0× now, 16.0× max; ring RGBA16F; engine headroom 4.0×; frame peak <between 1 and 4>;
    <n> headroom frames at L<l>, render median <12 ms …`. (On an SDR panel values above 1.0 clip, so the picture
    looks like SDR with brighter highlights clipped.) Without the variable on an SDR screen the line reads
    `hdr-selftest: SDR display; ring RGBA8; HDR stays in the recipe (hdr=on), loupe SDR`.
88. **SDR screen fallback.** On an SDR screen (or with the window moved to one), the HDR panel reads `SDR display`,
    the Headroom slider is disabled, turning HDR on reads `SDR display: HDR is kept in the recipe, the loupe shows
    SDR.` and the picture does not change.
89. **Soft proof over EDR.** With HDR on (step 86), press **S**: the proof shows the SDR print simulation (no
    highlights brighter than UI white); **S** again restores the EDR picture.

## Verdict (HDR / EDR)

PASS when steps 84–85 and 88–89 meet their expectations, and step 86 on an EDR screen (or step 87 on an SDR-only
machine). Record the screen's headroom from the first command and the frame times of step 85.

## Q. Assisted culling: real signals, suggestions, face strip and people (M3-11)

The culler measures real signals (ml-quality on the displayed preview) when a folder opens, learns from the
photographer's keep / reject decisions (per library), and in *automated* mode pre-fills decisions outside its
thresholds as translucent, outlined pills that change nothing until confirmed (Y) or dismissed (N). The sample
generator's `--defects` flag blurs every fifth frame and blows out the top third of every seventh. The samples have
no faces, so the hidden `--seed-faces` aid writes two synthetic people (person A in every frame, out of focus on the
blurred frames and with closed eyes on SAMPLE_0006, 0012, 0018, 0024, 0030, 0036; person B in every second group);
Cull ▸ Analyze Faces runs the real YuNet/SFace models instead (they download once, about 40 MB).

90. Quit Tessera. Create the shoot and launch:
    ```sh
    swift apps/mac/Support/make-sample-folder.swift "$SCR/cull" 40 --defects
    open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-cull" --folder "$SCR/cull" --seed-faces --fake-planner
    ```
    Expect `Wrote 40 JPEGs in 16 bursts to …/cull (8 blurred, 6 with blown highlights)` and a status message ending
    `synthetic faces seeded`. Press **⇧⌘D**. 📸 Expect `25 candidates · 25 selected`, with rows such as SAMPLE_0003
    `Missed focus 0.00 < 0.30 · Soft faces 0.18 < 0.30`, SAMPLE_0004 `Blown highlights 0.33 > 0.05` and SAMPLE_0006
    `Closed eyes 0.12 < 0.30`. Click **Cancel**.
91. **Assist.** Click **Assist** in the toolbar (or Cull ▸ Assist, ⌥⌘A). 📸 Expect: the toolbar toggle turns on with a
    small menu chevron beside it; the grid re-sorts by keep confidence; frames carry outlined `Keep?` pills near the
    top of the grid and `Reject?` pills on the blurred and closed-eyes frames at the end; the status message reads
    `Assist on: 32 suggested decisions · Y confirms all · N dismisses`; the counts stay `Keep 0  Reject 0`
    (identifiers `toolbar-assist`, `toolbar-assist-menu`). The inspector's **Assist** panel shows `Keep likelihood`
    with a bar and up to four signed terms; on a `Reject?` frame with closed eyes the first term is `Eyes closed -8.00`
    (`assist-pkeep`, `assist-explanation`), a `Reject?` chip, **Dismiss** and **Confirm All**, and
    `32 suggested · learner has 0 confirmed labels…` (`assist-status`).
92. **Dismiss.** Click the last cell of the grid (a `Reject?` frame) and press **N**. Expect: its pill disappears, the
    message reads `Dismissed 1 suggestion`, the inspector reads `31 suggested`, and nothing was decided.
93. **Confirm all.** Press **Y**. Expect: toast `Confirmed 31 suggestions: 15 keep, 16 reject` with Undo; status
    `Keep 15  Reject 16`; the dismissed frame stays undecided; the inspector reports `learner has 31 confirmed labels`.
    Press **⌘Z**. Expect `Undo: 31 images` and `Keep 0  Reject 0` in one step. Press **X** on any frame: the learner
    count goes up by one (manual decisions teach it too); **⌘Z** again.
94. **Assisted mode.** In the inspector's Assist panel choose **Assisted**. Expect: every pill disappears (no
    pre-filled decisions), the grid keeps its confidence order and the keep likelihood still shows. Choose **Automated**
    again. Cull ▸ **Sort by Keep Confidence** off: the grid returns to capture order (cell 1 = SAMPLE_0001).
95. **Face strip.** With the capture order, click cell 3 (SAMPLE_0003, a blurred frame) and press **Return**. 📸 Expect a
    `Faces` row under the loupe (`face-strip`) with two close-ups: the first with a red focus dot and an open-eye glyph,
    the second with a yellow dot and an eye-with-warning glyph; the legend on the right reads `Sharp  Soft  Missed  Eyes`.
    Hover the first: `Person 1: missed focus, eyes open. Click to zoom.`
96. **Zoom and per-person filter.** Click the first face (`face-chip-0`). Expect a popover (`face-zoom`) with the face
    large, `Person 1 in 40 frames`, chips `Focus 0.18 · missed focus` and `Eyes 0.86 · eyes open`, and two buttons. Click
    **…with eyes closed** (`face-filter-eyes-closed`). Expect: the loupe/grid shows only 6 frames (SAMPLE_0006, 0012, 0018,
    0024, 0030, 0036), the status bar shows an accent `Person 1 · eyes closed ⊗` (`status-person-filter`) and the message
    `Person 1 with eyes closed: 6 frames`. Click the status-bar chip: all 40 frames return. Expand the inspector's
    **People** panel: `Person 1  40 frames` and `Person 2  19 frames`, each with **Frames** and **Eyes closed**.
97. Press **Esc**, turn **Assist** off. Expect `Assist off` and no pills.

## R. Auto Edit and the agent review queue (M3-11)

The agent makes a base edit with the engine's own controls (no generated pixels); each step is an ordinary recipe
step in an "Agent base edit" history group with a one-line rationale. `--fake-planner` preselects the **Scripted test
planner** (the engine's FakePlanner with a fixed three-step script: exposure toward mid-grey, contrast + clarity,
vibrance), so no API key or network is needed.

98. **Settings ▸ AI** (⌘,). 📸 Expect an **AI** pane: Planner (default provider), Anthropic and OpenAI (model, API key
    field with **Save** / **Remove**, `Stored in your login Keychain, never in Tessera's files or logs.`), Ollama,
    Auto edit guardrails, Assisted culling thresholds and Style profile (questionnaire sliders, **Save Answers**,
    **Learn from My Edits**). Type `sk-test-verifier-0000` into the OpenAI key field and click **Save**. Expect
    `Key in Keychain: sk-t…0000`. Then:
    ```sh
    security find-generic-password -s dev.tessera.app.ai -a openai-api-key -w
    grep -c sk-test "$SCR/appdir-cull/ai-preferences.json" 2>/dev/null || true
    ```
    Expect the key from `security` (macOS may ask whether `security` may read Tessera's item: click Allow), and `0`
    (or no file) from grep: keys never reach preferences. Click **Remove**;
    `security …` then reports that the item could not be found. Close Settings.
99. **Auto Edit.** In the grid (capture order) click cell 1 and ⇧-click cell 4 (SAMPLE_0001–0004). Press **⇧⌘A**
    (or the toolbar's **Auto Edit**). 📸 Expect a sheet **Auto Edit**: Provider `Scripted test planner`
    (`autoedit-provider`), Photos `Selection (4)` selected among `Current view (40)` and `Whole shoot (40)`
    (`autoedit-scope`), both consistency toggles on, guardrails (masks on, crop off, skin retouch off and disabled,
    visual critic off, 3 rounds, 120 s), footer `Style profile: questionnaire only (Settings ▸ AI)` and **Edit 4 Photos**
    (`autoedit-start`). Choose **Anthropic**: the footer warns `Add an Anthropic API key in Settings ▸ AI` and the
    button is disabled. Choose the scripted planner again and click **Edit 4 Photos**.
100. Expect a progress strip `Auto edit · Scripted test planner` (`autoedit-progress`) with `n / 4`, the phase and a
     **Cancel** button, then (within about 30 s) the **Agent Review** sheet (`agent-review-list`): subtitle
     `4 to review · planned by scripted planner`, rows sorted least confident first, each with a confidence chip
     (`Low`, `Medium` or `High` with a percentage), the first rationale (`Exposure … EV because mean luminance measured … against a
     0.18 mid-grey target`, with `constrained to style-profile scene/person consensus` for the burst and person), the
     steps line, and **Show**, **Accept**, **Redo…**, **Revert**. The toolbar shows **Review 4**; the four cells show
     the `Edited` status pill.
101. Click **Accept** on the first row: chip `Accepted`, subtitle `3 to review · 1 accepted`, status
     `Accepted SAMPLE_… · style profile now has 1 sample`. Click **Revert** on the second row: chip `Reverted`, status
     `Reverted the agent's edit of … (one step in its history)`. Click **Redo…** on the third row, type
     `warmer, keep the sky` and press **Redo**: a `Redo “warmer, keep the sky”` progress strip, then that row's
     rationale reads `Redo “warmer, keep the sky”: temperature +400 K; other controls untouched`. Click **Done**.
102. With one of the edited photos focused, the inspector's **Agent Edit** panel shows `AI-assisted, non-generative
     edits` (`agent-provenance`), Confidence, Review status, Stopped, each step's controls and rationale, **Accept**,
     **Revert** and a `Redo with instruction…` field.
103. **Develop: the agent group.** Quit Tessera, then
     ```sh
     mkdir "$SCR/raw1" && cp "$SCR/raw/nikon-nef.NEF" "$SCR/raw1/"
     open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-raw1" --folder "$SCR/raw1" --fake-planner
     ```
     Press **Return** and wait for the render, then **⇧⌘A** ▸ **Edit 1 Photo**. After about 40 s the review sheet lists
     `nikon-nef.NEF`; click **Done**. The loupe re-renders about half a stop darker. Expand **History** in the inspector.
     📸 Expect an `AI  Agent base edit  100 %` section (`agent-group`) with an **Amount** slider at `100 %`
     (`agent-group-amount`) and three checked steps: `Exposure` (`Exposure -0.40 EV because mean luminance measured 0.32
     against a 0.18 mid-grey target`), `Clarity, Contrast` and `Vibrance`, each with its rationale
     (`agent-step-rationale`), and **Redo with Instruction…** (`agent-group-redo`).
104. Drag **Amount** to about 60 %. Expect the loupe to follow while dragging; on release the readout shows `60 %`, the
     status reads `Agent base edit: 60 %` and the history list gains `Agent base edit 60%`. Press **⌘Z**: back to 100 %.
105. Uncheck the **Exposure** step. Expect the picture to brighten, a new history step `Turn Off Exposure`, and the other
     two steps still applied. Drag Amount to about 50 % again: the fade applies to the remaining steps only.
106. Click **Redo with Instruction…**, type `cooler`, press **Redo**. Expect a progress strip, the develop session to
     close and reopen, and a second section `Agent redo: cooler` with one step `Temperature`
     (`Redo “cooler”: temperature -400 K; other controls untouched`). The first group keeps its amount and toggles.
107. **Tests.**
     ```sh
     cargo test -p tessera-ffi --release --test assist 2>&1 | grep "test result"
     cargo test -p tessera-ffi --release --lib group_amount 2>&1 | grep "test result"
     cargo test -p cull -p style-profile -p agent --release 2>&1 | grep "test result"
     (cd apps/mac && swift test --filter "AgentFadeTests|AgentReviewQueueTests|AISettingsTests|AssistBridgeTests" 2>&1 | grep "Executed")
     ```
     Expect `6 passed`, `1 passed`, only `ok.` lines, and `Executed 10 tests, with 0 failures`.

## Verdict (assisted culling and agent review)

PASS when steps 90–107 meet their expectations. Real face analysis (Cull ▸ Analyze Faces) needs a network the first
time; record whether it was tried and its message.

## Appendix: accessibility identifiers (M3-11)

| Identifier | Element |
| --- | --- |
| `toolbar-assist` · `toolbar-assist-menu` | Toolbar Assist toggle and its mode / sort menu |
| `toolbar-auto-edit` · `toolbar-agent-review` | Toolbar Auto Edit and Review n |
| `analysis-progress` | Background analysis progress strip |
| `assist-panel-toggle` · `assist-pkeep` · `assist-explanation` · `assist-status` · `assist-confirm-all` | Inspector ▸ Assist |
| `face-strip` · `face-chip-<n>` · `face-zoom` · `face-filter-person` · `face-filter-eyes-closed` | Loupe face strip and zoom popover |
| `people-clear-filter` · `people-eyes-closed-<person>` · `status-person-filter` | People panel and the status-bar person filter |
| `autoedit-provider` · `autoedit-scope` · `autoedit-start` · `autoedit-blocker` · `autoedit-progress` · `autoedit-cancel` | Auto Edit sheet and run |
| `agent-review-list` · `agent-review-row` · `agent-review-confidence` · `agent-review-accept` · `agent-review-redo` · `agent-review-revert` · `agent-review-instruction` | Agent Review sheet |
| `agent-provenance` | Inspector ▸ Agent Edit |
| `agent-group` · `agent-group-amount` · `agent-group-amount-readout` · `agent-step-toggle-<id>` · `agent-step-rationale` · `agent-group-redo` · `agent-group-instruction` | History ▸ agent group |
| `ai-default-provider` · `ai-key-<provider>` · `ai-key-status-<provider>` · `ai-profile-status` | Settings ▸ AI |

## S. Keyword suggestions, captions / alt text and text in images (M3-15)

Suggestions, captions and OCR run on this Mac (`ml-caption`: SigLIP keywords, Florence-2 captions and text) as
background jobs; results are cached in the catalog and reused until the model changes. Nothing is applied until you
accept it: suggestions become keywords only on click, generated captions only on **Save**. The hidden
`--fake-captioner` aid swaps in deterministic test models that name each frame's dominant colour (keywords
`photograph`, `<colour>`, `gradient`, `abstract`, `texture` at 93 / 81 / 62 / 41 / 22 %; caption
`A <colour> gradient photograph.`; text `TEST CARD <COLOUR>`), so no weights or network are needed. Without it the real
models need `python3 tools/fetch_siglip.py --cache APP_DIR/models/cache` and `tools/fetch_florence.py` (same cache).

108. Quit Tessera. Create the shoot and launch:
     ```sh
     swift apps/mac/Support/make-sample-folder.swift "$SCR/words" 12
     open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-words" --folder "$SCR/words" --fake-captioner
     ```
     Expand the inspector's **Keywords** and **Metadata** panels. With cell 1 (SAMPLE_0001, pink) focused, Keywords shows a
     **Suggested** sub-header with **Suggest** (`keyword-suggest-selection`) and the hint `Suggestions appear here…`.
109. **Suggest.** Click **Suggest** (or Library ▸ Suggest Keywords for Selection, ⌥⌘K). Expect a brief
     `Keyword suggestions` progress strip (`understanding-progress`, with **Stop**, `understanding-cancel`), then the
     status `Keyword suggestions: 1 photo` and 📸 five dashed-outline chips (`keyword-suggestions`), highest first:
     `+ photograph`, `+ pink`, `+ gradient`, `+ abstract`, `+ texture`, each with an ✕ and a confidence bar along its
     bottom edge (the three at or above 50 % in primary ink). Hover `pink`: `pink: 81 % confidence · new keyword under
     “Suggested” · click to accept…`. Below: `Accept all ≥ 50 %`, a threshold slider (`keyword-suggestion-threshold`) and
     **Accept 3** (`keyword-suggestions-accept-all`). Click **Suggest** again: `Keyword suggestions: already up to date`.
110. **Accept one.** Click the `pink` chip (`keyword-suggestion-pink`). Expect: status `Added “Suggested › pink” to the
     catalog (XMP off)`; the chip leaves Suggested; `pink` appears as an applied keyword chip and in the Keyword List as
     `Suggested` ▸ `pink` (count 1). `SAMPLE_0001.jpg.xmp` does not mention `pink` (catalog only, the default).
     Type `keyword:pink` in the filter bar: `1 match`. Clear.
111. **Mapping.** Keyword List ▸ **New…** `Colors`, then right-click `pink` ▸ Move Into ▸ `Colors`. Select cell 2
     (SAMPLE_0002, pink) and **Suggest**. The `pink` chip now shows ↳ and its tooltip reads `adds “Colors › pink”`.
     Click it: status `Added “Colors › pink” …`.
112. **Reject and threshold.** On cell 2 click ✕ on `texture` (`keyword-suggestion-reject-texture`): it disappears and
     stays gone after **Suggest** (cached). Drag the threshold to 40 %: **Accept 3** (photograph, gradient, abstract).
     ⇧-click any chip: all three are accepted at once (`Added 3 suggested keywords …`).
113. **Selection.** Select cells 1–12 (⌘A) and press ⌥⌘K. Expect the strip to count `n / 10` (cells 1 and 2 are cached), then
     merged chips: a colour chip carries a small count (for example `pink 1`) when it is not suggested for every selected photo;
     accepting one tags only the photos it was suggested for.
114. **XMP opt-in.** Settings ▸ AI (⌘,) ▸ **Keywords, captions and text** 📸: `Suggest keywords for new photos`
     (`ai-auto-suggest`), `Write accepted suggestions to XMP sidecars` (`ai-write-suggested-xmp`) and
     `Test models (--fake-captioner)…` (`ai-caption-models`). Turn on XMP writing, accept `gradient` on cell 4. Then
     `grep -c "Suggested|gradient" "$SCR/words/SAMPLE_0004.jpg.xmp"` prints `1`.
115. **Generate caption.** Focus cell 1 only. Metadata shows **Caption**, **Alt text** (`iptc-altText`) and **Generate**
     (`metadata-generate-caption`; disabled with several photos selected: `Select one photo…`). Click **Generate**.
     📸 Expect Caption `A pink gradient photograph.` and Alt text `An abstract image of smooth pink tones with soft
     light.` filled but unsaved, **Save** (`caption-draft-save`), **Discard** (`caption-draft-discard`) and
     `Generated on this Mac. Edit the fields, then Save.`. Edit the caption to `Pink test frame.` and click **Save**:
     status `Saved caption and alt text to SAMPLE_0001.jpg`; the XMP has `dc:description` `Pink test frame.` and
     `Iptc4xmpCore:AltTextAccessibility`. **Discard** on another photo leaves its XMP untouched.
116. **Text in image.** Click **Detect Text** (`ocr-detect`). Expect under **Text in Image** a read-only block
     `TEST CARD PINK` (`ocr-text`) and **Find Photos with This Text** (`ocr-find`). Click it: the filter reads
     `text:"TEST CARD PINK"` and shows `1 match`. Select all, Library ▸ Detect Text in Selection, then search
     `text:"test card"`: all 12 match; `gradient` matches the frames with generated captions.
117. **Auto-suggest.** Turn on `Suggest keywords for new photos`, quit, add a frame
     (`cp "$SCR/words/SAMPLE_0003.jpg" "$SCR/words/NEW.jpg"`) and relaunch with the same arguments: a keyword strip runs
     for the one uncached photo only (`n / 1`).
118. **Tests.**
     ```sh
     cargo test -p tessera-ffi --release --test understanding 2>&1 | grep "test result"
     cargo test -p ml-caption -p library -p index -p sidecar --release 2>&1 | grep "test result"
     (cd apps/mac && swift test --filter "SuggestionChipTests|SearchTermTests|ThemeLintTests" 2>&1 | grep "Executed")
     ```
     Expect `5 passed`, only `ok.` lines, and `Executed 8 tests, with 0 failures`.

## Verdict (keywords, captions and text)

PASS when steps 108–118 meet their expectations. Real models (without `--fake-captioner`) need the fetch scripts once;
record whether they were tried and the timing of a 12-photo caption job.

## Appendix: accessibility identifiers (M3-15)

| Identifier | Element |
| --- | --- |
| `keyword-suggest-selection` · `keyword-suggestions` · `keyword-suggestion-<keyword>` · `keyword-suggestion-reject-<keyword>` · `keyword-suggestion-threshold` · `keyword-suggestions-accept-all` | Keywords ▸ Suggested |
| `iptc-altText` · `metadata-generate-caption` · `caption-draft-save` · `caption-draft-discard` | Metadata ▸ Caption and alt text |
| `ocr-detect` · `ocr-text` · `ocr-find` | Metadata ▸ Text in Image |
| `understanding-progress` · `understanding-cancel` | Status strip: suggestion / caption / text job |
| `ai-auto-suggest` · `ai-write-suggested-xmp` · `ai-caption-models` | Settings ▸ AI ▸ Keywords, captions and text |

## T. Tethered capture (M3-12b)

**File ▸ Tethered Capture…** docks a non-modal **Tethered Capture** panel under the filter bar: the camera (connect /
disconnect, battery and frames left when the camera reports them), the session (name, folder, file-name template
with a live example), **Capture** (⇧⌘T) and an interval timer, and an **Incoming** strip of the latest frames with focus
and eyes badges. The engine downloads each frame into a private staging folder, renames it into the session folder,
indexes it, extracts a preview and scores it before the frame appears; the app then files it into the session album
and (with **Show newest in loupe**) opens it in the loupe so X / P / 1–3 decide it at once. The hidden
`--fake-tether <folder>` aid replaces the camera with a **test camera** that "shoots" that folder's images in file-name
order on Capture and every `--fake-tether-interval <s>` seconds (default 6; `0` = only on Capture), so no camera is
needed. `--tether` opens the panel at launch; the test aid `--tether-connect` also connects once the folder has loaded.
Put these value-less flags **last** on the command line: macOS treats a path left over after an unpaired flag as a
document to open, and then the main window does not appear.

119. Quit Tessera. Create a shoot and a test camera card, then launch:
     ```sh
     swift apps/mac/Support/make-sample-folder.swift "$SCR/studio" 6
     swift apps/mac/Support/make-sample-folder.swift "$SCR/card" 8 --defects
     open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-tether" --folder "$SCR/studio" \
       --fake-tether "$SCR/card" --fake-tether-interval 0
     ```
     Choose **File ▸ Tethered Capture…**. 📸 Expect the panel (`tether-panel`) at the top of the grid area: title
     `Tethered Capture` with an outlined `Test camera` chip; a **Camera** row (`tether-device-list`) listing
     `Test camera (card)` with `80 %` and `8 left` (`tether-device-readouts`) and **Connect** (`tether-connect`);
     **Session** `Tether <today>` (`tether-session-name`) with the folder `…/studio/Tether <today>` (`tether-folder`) and
     **Choose…**; **File names** `{sequence}_{original}.{ext}` (`tether-naming`) with the example
     `DSC01234.ARW → 0001_DSC01234.ARW` (`tether-naming-example`) and a **+** token menu (`tether-naming-token`).
120. **Naming.** Click into the template and change it to `{sequence}.tif`. Expect the field outline to turn red and the
     example line to read `End the name with .{ext} so the file keeps its format`; **Connect** is disabled. Change it to
     `studio_{sequence}.{ext}`: the example reads `DSC01234.ARW → studio_0001.ARW`. Open **+** ▸ `Original name
     {original}`: the template becomes `studio_{sequence}_{original}.{ext}`.
121. **Connect.** Click **Connect**. 📸 Expect: the header reads `● Connected: Test camera (card)  80 %  8 left` with
     **Disconnect** (`tether-disconnect`); the session and naming fields are dimmed (fixed for the session); an **Albums**
     column with `Tether <today> captures` (`tether-album`) and `Session` (`tether-smart-album`); a capture row with an
     accent **Capture** button (`tether-capture`) and `⇧⌘T`, `Every 10 s`, `10 frames`, **Start Interval**, a checked
     `Show newest in loupe` (`tether-auto-advance`) and `0 frames` (`tether-summary`); an **Incoming** strip
     (`tether-incoming`) with the hint `Press Capture (⇧⌘T); the test camera also fires on its own timer.` The sidebar gains
     a group `Tether <today>` with the album `Tether <today> captures` (selected, count 0) and a smart album `Session` with
     the scope mark; the status bar reads `Tethered to Test camera (card) · saving to Tether <today> (test camera)`.
122. **Capture.** Press **⇧⌘T** (or click **Capture**). Expect a `Downloading` placeholder tile (`tether-pending`) for a
     moment, then tile `0001` (`tether-frame-1`) with the frame, a focus dot and no eye glyph (the samples have no faces;
     without face models the tooltip says `faces not analysed: …`). The app switches to the **Loupe** on
     `studio_0001_SAMPLE_0001.jpg`, the album count becomes 1, the header reads `7 left`, and the status bar message reads
     `Tether: studio_0001_SAMPLE_0001.jpg · 1 frame`. `ls "$SCR/studio/Tether "*` lists exactly that file (nothing is
     left in a staging folder; the card still has its 8 originals).
123. **Cull as they arrive.** Press **⇧⌘T** twice more (about a second apart). Expect tiles `0003`, `0002`, `0001`
     (newest left), the loupe on `studio_0003_SAMPLE_0003.jpg` (a blurred frame: its tile's dot is red, `Missed`), and
     `3 frames`. Press **X**. 📸 Expect a filled `Reject` chip on tile `0003` and the tile dimmed; the status bar shows
     `Reject 1`. Click tile `0001`: the loupe/grid focuses it (accent ring on the tile); press **P**: the tile shows `Keep`.
     Double-click tile `0002`: it opens in the loupe.
124. **Session smart album.** Click **Session** in the panel (or the sidebar). Expect the grid/loupe source
     `Session · 2 images` (the rejected frame is excluded: the rule is `decision!=reject`, scoped to the session group).
     Right-click **Session** in the sidebar ▸ **Edit Smart Album…**: the rule reads `decision!=reject` and `Only search albums in this
     group` is on. Cancel, and click `Tether <today> captures` again (all 3 frames, arrival order).
125. **Interval.** Choose `Every 2 s` and `5 frames` and click **Start Interval**. Expect one frame at once, then one
     every 2 s; the button reads **Stop Interval** (`tether-interval-toggle`) with `4 left`, `3 left`… (`tether-interval-status`);
     the pickers are disabled. After the fifth frame the interval stops by itself. The card then has no frames left:
     press **⇧⌘T** and expect an inline error (`tether-error`) `Capture: the test camera has no frames left in its folder`.
126. **Disconnect.** Click **Disconnect**. Expect `Tether session ended: 8 frames` in the status bar, the Camera row
     back with **Connect**, and the incoming strip kept. Close the panel with ✕ (`tether-close`).
127. **No camera.** Quit, then launch without the test camera:
     `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-tether" --folder "$SCR/studio" --tether`. With no
     camera attached, expect `Looking for cameras…` briefly, then the warning `No camera found. Connect it over USB, switch it
     on and set it to PC / tether mode.` (`tether-no-camera`) and **Refresh** (`tether-refresh`); nothing else changes.
     (With a real camera attached its name appears with **Connect**; record the model if you try it.)
128. **Timer.** Quit and relaunch step 119's command with `--fake-tether-interval 3` and `--tether-connect` appended (last).
     Expect the panel to connect by itself and a new frame every 3 s without pressing anything (as if the photographer
     used the camera's own shutter button), each opening in the loupe. Uncheck **Show newest in loupe**: frames keep
     arriving in the strip and the album while the loupe stays on the frame you are deciding.
129. **Tests.**
     ```sh
     cargo test -p tether -p tessera-ffi --release --test tether --lib 2>&1 | grep "test result"
     (cd apps/mac && swift test --filter "TetherNamingTests|IncomingStripTests|TetherBridgeTests|ThemeLintTests" 2>&1 | grep "Executed")
     ```
     Expect only `ok.` lines (the FFI `tether` suite: `2 passed`; the tether crate's fake camera: 3 tests) and
     `Executed 10 tests, with 0 failures`.

## Verdict (tethered capture)

PASS when steps 119–129 meet their expectations. A physical camera needs hardware (ImageCaptureCore, one camera with
remote capture); if one was available, record the model, whether Connect, Capture and the physical shutter worked, and
the battery readout.

## Appendix: accessibility identifiers (M3-12b)

| Identifier | Element |
| --- | --- |
| `tether-panel` · `tether-close` · `tether-error` · `tether-notice` | Tethered Capture panel, its close button and inline messages |
| `tether-device-list` · `tether-device-<n>` · `tether-device-readouts` · `tether-refresh` · `tether-no-camera` · `tether-connect` · `tether-connected` · `tether-disconnect` | Camera row |
| `tether-session-name` · `tether-folder` · `tether-folder-choose` · `tether-naming` · `tether-naming-token` · `tether-naming-example` | Session and file names |
| `tether-album` · `tether-smart-album` | Session album and scoped smart album |
| `tether-capture` · `tether-interval-seconds` · `tether-interval-count` · `tether-interval-toggle` · `tether-interval-status` · `tether-auto-advance` · `tether-summary` | Capture row |
| `tether-incoming` · `tether-frame-<sequence>` · `tether-pending` | Incoming strip |

## U. People view: naming, merge / split, Person facet (M2-40)

**Library ▸ People** in the sidebar (or Library ▸ Show People, ⌥⌘P) replaces the grid with a grid of person tiles:
the representative face (the engine's medoid, the most central member; the sharpest member when there is none), the
name, or an inline name field for unnamed clusters, the photo count and an outlined `Confirmed` chip when every face is
confirmed. Named people come first, then by photo count. Opening the view runs the incremental clustering job
(`refresh_people(force: false)`) off the main thread; Analyze Faces runs it again. Tiles come from `people()` alone
(names, face and confirmed counts, medoid); the detail view reads its faces with one `person_members` call. Every edit
goes through the engine's people calls and the tiles reload from `people(refresh: true)`. While the People view (or a
person's detail) is showing, Edit ▸ Undo / Redo replay the engine's people history (M2-44).

**Fixture.** The sample folder has no faces, so these steps use the hidden `--seed-faces` aid: *generated* descriptors
for two synthetic people (person A in all 40 frames, person B in the 19 frames of odd groups), not real detections.
Real faces need Cull ▸ Analyze Faces (YuNet/SFace, downloaded once). With fewer than 1,024 faces clustering is exact,
so the approximation footnote (step 139) needs a larger real library.

130. Quit Tessera and launch on a fresh shoot:
     ```sh
     swift apps/mac/Support/make-sample-folder.swift "$SCR/people" 40
     open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-people" --folder "$SCR/people" --seed-faces
     ```
     Expect the status message to end `synthetic faces seeded` and the sidebar's Library section to list **People**
     (`sidebar-people`) with the count `2`.
131. **People view.** Click **People**. 📸 Expect: the filter bar hides; a 32 pt bar `People  2 · 0 named` (`people-count`,
     with **Refit**, `people-refit`); a grid (`people-grid`) of two tiles (`person-tile-<id>`), the 40-photo person first,
     each with a face crop, an `Unnamed` name field (`person-name-field-<id>`) and `40 photos` / `19 photos`. No
     `Confirmed` chip yet. The toolbar shows **Merge** (`toolbar-people-merge`), disabled.
132. **Name.** Click into the first tile's field, type `Ada` and press **Return**. Expect: the tile shows `Ada`
     (`person-name-<id>`) and stays first; status `Named Ada`; the header reads `2 · 1 named`. No `.xmp` file mentions
     `Ada` (`grep -l Ada "$SCR/people"/*.xmp` prints nothing): by default names stay in the library.
133. **Name suggestions.** If the second tile shows a suggestion button (`person-suggestions-<id>`, only when its faces
     resemble a named person), its menu lists `Looks like  Ada  NN % similar`; choosing it *merges* the cluster into Ada.
     With the seeded fixture the two people are dissimilar, so no button is expected; record what was shown.
134. **Detail, confirm.** Double-click **Ada**. 📸 Expect the detail view: back chevron (`person-detail-back`), the name field
     (`person-detail-name`) reading `Ada`, `40 photos · 40 faces · 0 confirmed` (`person-detail-counts`), **Show Photos**
     (`person-show-photos`), **Confirm All** (`person-confirm-all`) and **Split** (`person-split`, disabled); 40 face chips
     (`face-member-<item>-<ordinal>`) with the file name under each and a seal button (`face-confirm-<item>-<ordinal>`);
     on the right a **Move to** column (`person-move-targets`) with the other person (`person-move-target-<id>`). Click the
     first chip's seal: it fills in keep ink and the counts read `1 confirmed`. Click **Confirm All**: `40 confirmed`, and
     back in the grid (**Esc** or the chevron) Ada carries the `Confirmed` chip (`person-confirmed-<id>`).
135. **Split.** Double-click **Ada**, click the chips of SAMPLE_0001 and SAMPLE_0002 (accent ring, `2 selected`), then
     **Split**. Expect `Split 2 faces into a new person`; the detail reads `38 photos · 38 faces · 38 confirmed`; the grid
     (Esc) has three tiles, the new one `Unnamed · 2 photos` without a `Confirmed` chip (split resets confirmation).
136. **Drag to reassign.** Double-click the new 2-photo person. Drag one face chip onto **Ada** in the Move to column (the
     row highlights in the accent while targeted). Expect `Moved the face to Ada (unconfirmed)`; the person now has 1
     face. Right-click the remaining chip ▸ **Move To** ▸ **Ada**: the person disappears and the grid is back to two
     tiles, Ada with 40 photos but no longer fully confirmed (`38/40 confirmed`).
137. **Person facet.** Name the 19-photo person `Ben` (Return in its field). Click **All Photos**. In the filter bar open
     **Person** (`facetPerson`). Expect `Ada    40` and `Ben    19`. Tick **Ben**: the grid shows 19 frames, the facet
     reads `Person · Ben` and the match count `19 matches`. Tick **Ada** too: `Person · 2` and 40 frames (any of the
     chosen people). Keep two of Ben's frames (P), then open **Decision** ▸ tick **Keep**: the grid shows only those 2
     (the facet intersects every other facet) and the Person menu counts `Ben    2`. **Clear** resets every facet,
     Person included. From a tile's context menu in People, **Show Photos** opens All Photos with the Person facet set
     to that person.
138. **Face strip names.** Open SAMPLE_0002 in the loupe (Return). Hover the first face chip: `Ada: sharp, eyes open.
     Click to zoom; right-click to name.` Right-click it ▸ **Rename Ada…** (`face-name-person`): a sheet with a name
     field; type `Ada L.` and click **Name**: the tooltip, the People tile and the Person facet follow. On an unnamed
     person the item reads **Name…**. **Show in People** opens that person's detail view.
139. **XMP opt-in.** Settings (⌘,) ▸ **Library** 📸: `Write face regions to XMP` (`people-setting-write-regions`) and
     `Add person keywords` (`people-setting-person-keywords`, disabled until the first is on), with a hint. Turn both on
     and rename Ben to `Ben K.` in People. Expect `Named Ben K. · face regions written to XMP`, and
     `grep -l "Ben K." "$SCR/people"/*.xmp | wc -l` prints `19`; each contains `mwg-rs:Regions` and a `Ben K.` keyword.
     Earlier keywords are not removed.
140. **Merge.** In People click **Ada L.**, ⌘-click **Ben K.** (both tinted, `2 selected` in the header) and click
     **Merge** in the toolbar. Expect `Merged 2 people into Ada L.` (a named person first, then the most photos, wins
     and keeps its name) and one tile, `Ada L. · 40 photos`. The approximation footnote (`people-approximate-note`,
     `Clustered from a sample of N faces`, N the job's reported sample size, at most 1,024) appears under the grid only
     after a clustering job over more than 1,024 eligible faces; record whether a large library was tried.
140a. **Undo / Redo (M2-44).** Still in People, open the **Edit** menu 📸: it reads **Undo Merge People** (the engine's
     description of the last people edit). Press **⌘Z**: status `Undo Merge People`, the grid is back to two tiles,
     `Ada L.` (40 photos) and `Ben K.` (19 photos), and Edit now offers **Redo Merge People**. Press **⇧⌘Z**: one tile
     again, status `Redo Merge People`. Double-click the tile, click one face's seal (confirm), then **⌘Z** in the
     detail view: the seal empties again and the Edit menu reads **Undo Merge People**. Undo also restores names (and,
     with the XMP opt-in, the sidecar bytes). Click **All Photos**: Edit reads plain **Undo** and ⌘Z addresses culling,
     not people edits. The people history is per session (up to 32 edits); a new edit clears Redo.
141. **Tests.**
     ```sh
     (cd apps/mac && swift test --filter "PeopleModelTests|PeopleBridgeTests|PeopleUndoMenuTests|ThemeLintTests" 2>&1 | grep "Executed")
     ```
     Expect `Executed 17 tests, with 0 failures`: the view model against a stubbed engine (naming and opt-ins,
     suggestions, merge, split, reassign / confirm, the Person facet's intersection, the off-main refresh and the
     approximation note with the job's sample size, errors, in-place library updates, medoid tiles without an
     assignment scan, one-call detail members, undo / redo with the engine's descriptions), Edit ▸ Undo / Redo routing
     in the People view, and the same calls through a real engine session.

## Verdict (People view)

PASS when steps 130–141 (with 140a) meet their expectations. Record whether real faces (Analyze Faces) and a library with more than
1,024 faces were tried.

## Appendix: accessibility identifiers (M2-40)

| Identifier | Element |
| --- | --- |
| `sidebar-people` | Sidebar ▸ Library ▸ People |
| `people-view` · `people-grid` · `people-count` · `people-refit` · `people-refreshing` · `people-analyze-faces` · `people-approximate-note` | People view, its header bar, empty state and approximation footnote |
| `toolbar-people-merge` | Toolbar Merge (People view only) |
| `person-tile-<id>` · `person-name-<id>` · `person-name-field-<id>` · `person-suggestions-<id>` · `person-confirmed-<id>` | Person tile: name, name field, name suggestions menu, confirmed chip |
| `person-detail-back` · `person-detail-name` · `person-detail-counts` · `person-show-photos` · `person-confirm-all` · `person-split` · `person-faces` | Person detail view |
| `face-member-<item>-<ordinal>` · `face-confirm-<item>-<ordinal>` | Face chip in the detail view and its confirm toggle |
| `person-move-targets` · `person-move-target-<id>` | Detail ▸ Move to (drop targets) |
| `facetPerson` | Filter bar ▸ Person |
| `face-name-person` | Loupe face strip ▸ context menu ▸ Name… / Rename… |
| `people-setting-write-regions` · `people-setting-person-keywords` | Settings ▸ Library |

## V. Document mode: layered documents (B5-02)

**Layers** (the fourth view-mode segment) is Tessera's layered editor: a viewport, and Properties, Layers and History
panels in the inspector. Since B5-03 documents run on the engine's `DocumentSession` (`EngineDocumentBackend`): the
engine of the open folder, or a standalone engine in the app-support directory when no folder is open. With
`--stub-library` they run on the **stub backend** (`StubDocumentBackend`): a new document opens with six sample layers
(Paper, Landscape with a soft elliptical mask, Vignette clipped to it, and a Grade group holding Curves 1 and
Hue/Saturation 1), rendered on the CPU. Part 1 runs on the stub; part 2 repeats the key steps on the real engine.
`--new-document` (test aid) creates a document at launch; `--open-document <file>` opens one.

### Part 1: over the stub backend

130. Quit Tessera. Build and launch:
     ```sh
     (cd apps/mac && swift build && Support/make-app.sh)
     open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-doc" --stub 200 --stub-library
     ```
     Choose **File ▸ New Document…** (⌘N). 📸 Expect a sheet `New Document` with Preset, Width `2400`, Height `1600`,
     Bit depth `8-bit | 16-bit | 32-bit float`, Colour profile `sRGB IEC61966-2.1`, the footer note `Stub backend: the
     document opens with sample layers` and an accent **Create** (`document.new.create`). Click **Create**.
131. 📸 Expect: the toolbar's view control on **Layers**, a tab `Untitled-1` (`document.tabs.0`) in the toolbar, the window
     subtitle `6 layers`; the viewport (`document.viewport`) shows a dusk landscape inside an ellipse on warm paper, a
     checkerboard in the transparent margin around the paper, and the canvas surround beyond it; the zoom chip
     (`document.zoomHUD`) briefly shows the fit zoom. The inspector (B5-16) shows the sub-tabs **Stack · Properties ·
     Channels** with Stack chosen, and **History** below (`Opened`, highlighted; `0 states · Zero KB`); ⌃2 shows
     **Properties** (`Grade`, `Group · Pass Through`), ⌃1 returns to Stack. The Layers outline (Stack) lists, top to
     bottom: `Grade` (folder glyph, expanded) with `Hue/Saturation 1` and `Curves 1` indented, `Vignette` with the clipping
     glyph and a drop glyph, `Landscape` with a link glyph and a white ellipse mask thumbnail, `Paper` with a lock glyph.
     The status bar reads `2400 × 1600 px · 8-bit · sRGB IEC61966-2.1 | <zoom> | Move (V)`.
132. **Zoom and pan.** Press ⌘1: the chip reads `100 %` and the landscape fills the view at full size. ⌘= (View ▸ Zoom In) twice →
     `300 %` (pixels turn crisp: nearest-neighbour from 200 %), ⌘− → `200 %`, ⌘0 → the fit zoom. Hold ⌥ and drag right in
     the viewport: the zoom grows around the point you pressed (scrubby zoom). Pinch on a trackpad: zooms about the
     pointer. Two-finger scroll pans; hold Space and drag: the cursor is a closed hand and the canvas follows.
     Expect the image to stay sharp after each gesture settles and the status bar zoom to match the chip.
133. **Layers and live sliders.** Click `Landscape`. Properties shows `Pixel`, Bounds `96, 96 · 2208 × 1408 px` and Mask
     `On · linked`. Drag **Opacity** (`document.layers.opacity`) to about 40 %: the landscape fades live while dragging;
     on release History gains one row `Opacity 40 %` (`document.history.row.1`) and the tab shows the dirty dot. Choose
     **Multiply** from the blend pop-up (`document.layers.blendMode`: modes in six groups with dividers): History gains
     `Blend Mode`. Press ⌘Z twice: opacity and mode return; ⇧⌘Z once re-applies the opacity. Click `Opened` in History:
     the document returns to how it opened.
134. **Adjustments.** Click `Curves 1`: Properties shows a channel control (RGB / Red / Green / Blue) and the curve editor
     (`document.properties.curves.editor`) with an S-curve. Drag the upper point up: the picture brightens live, one
     `Curves` history row on release. Click **Layers ▸ New ▸ Adjustment Layer ▸ Levels**: `Levels 1` appears above the
     selected layer and is selected; drag **Gamma** (`document.properties.levels.gamma`) to 2.00: midtones lift live.
     Switch the channel to **Blue** and drag Output white down: the image turns yellow. Add **Hue/Saturation** from the
     footer's adjustment menu (`document.layers.addAdjustment`), tick **Colorize**: the image becomes monochrome in one hue.
     Repeat quickly for Exposure, Posterize (4 levels = visible banding), Threshold (black and white), Channel Mixer
     (Monochrome) and Invert (`Invert has no settings.`).
135. **Fills.** **Layer ▸ New ▸ Fill Layer ▸ Gradient**: a black-to-white gradient covers the canvas; Properties shows
     Linear / Radial, a gradient preview, two stops with colour wells and positions, **Add Stop** and **Reverse**. Click
     **Reverse**: white-to-black. Set the layer's blend mode to **Soft Light**. **Layer ▸ New ▸ Fill Layer ▸ Solid Color**,
     pick a colour in its well (`document.properties.fill.color`): the canvas takes that colour; one `Solid Color` history
     row a moment after you stop picking. Delete it with ⌫ (the Layers outline focused) or the trash button.
136. **Structure.** Select `Vignette` and `Landscape` (⌘-click) and press ⌘G: one history row `Group Layers`, a new
     `Group 1` holds both. ⇧⌘G: they return. ⌘J on `Paper`: `Paper copy` above it. ⌘E (Merge Down) on `Paper copy`: it
     merges into `Paper`. Drag `Curves 1` out of `Grade` to the top of the list: one `Move Layer` history row, the row
     animates to its new place; drag it back into `Grade`. Double-click a layer name, type `Sky`, Return: renamed. Click an
     eye: the layer hides (name dimmed); ⌥-click an eye: only that layer shows. Right-click a row: the menu mirrors the
     Layer menu (Rename, Duplicate, Delete, Group, Ungroup, Merge Down, Flatten, clipping, mask items). Lock buttons
     (`document.layers.lock.*`) toggle the lock glyph on the row.
137. **Selection and masks.** Press **M**, drag a rectangle over the sun: marching ants run around it and the status bar
     shows `Selection W × H`. Select `Vignette` and **Layer ▸ Layer Mask ▸ From Selection**: its mask thumbnail appears;
     the vignette now shows only inside the rectangle. ⇧-click that mask thumbnail: a red cross, the mask is off. ⌘D:
     the ants disappear. **V** returns to Move (clicking the canvas with Move explains that moving pixels arrives later).
138. **Panels and screen modes.** Press **Tab**: sidebar and inspector hide; Tab again restores them. Press **F**: the
     window goes full screen; F: panels hide too; F: back to standard. Culling keys do nothing here: press **X**, **P**,
     **1**, **G**, **E** and the arrows — the document, the mode and the library selection stay as they are.
139. **History and snapshots.** Click **New Snapshot…** (`document.history.newSnapshot`), keep `Snapshot 1`, Save: it is
     listed under Snapshots. Make two edits, click **Restore** on the snapshot: the document returns to it and History
     gains `Snapshot “Snapshot 1”` (undoable). The memory line reads `<n> states · <size>`.
140. **Save, reopen, close.** ⌘S on the new document opens the **Save As** sheet (B5-06: Tessera's own sheet, so it can
     be scripted): the name field (`document.saveAs.name`) already has the keyboard and reads `Untitled-1.tessera-doc`;
     select all, type `Poster.tessera-doc`, click **Choose…** (`document.saveAs.choose`) and pick `$SCR`, then **Save**
     (`document.saveAs.save`, or Return). The Format pop-up (`document.saveAs.format`) follows the name's extension. The tab title
     becomes `Poster.tessera-doc`, the dirty dot goes. Choose **Save As…** with a `.psd` name: the status bar reads
     `Save As: Saving as PSD / PSB needs the engine (B5-03); save as .tessera-doc`. **File ▸ Export Flat…** (⇧⌘E): format
     PNG / JPEG / TIFF, quality for JPEG, colour space; export `Poster.png` into `$SCR` and check it opens in Preview with
     transparent margins (JPEG: white). Make one edit and press ⌘W: `Do you want to save the changes made to
     “Poster.tessera-doc”?` with Save…, Cancel, Don’t Save; choose Don’t Save: the tab closes and the viewport shows the
     `No document` empty state with New Document… and Open Document…. **File ▸ Open Document…** (⇧⌘O) `Poster.tessera-doc`:
     the saved layers come back. In Finder, choose Open With ▸ Tessera on `Poster.tessera-doc` (with the app running):
     it becomes the current tab (a document already open is not opened twice).
141. **Several documents and Edit in Layers.** **File ▸ Open Folder…** `$SCR/shoot` (step 2; stub items have no files to
     edit), select a photo in the grid, press ⌘E (**Library ▸ Edit in Layers**): a new tab named after the photo with one pixel layer (on the stub, JPEGs open as decoded; RAW files use
     macOS's own rendering). Switch between tabs: each keeps its zoom and position. Open `Poster.png` (Open Document…):
     one pixel layer named `Poster`.
142. **Tests.**
     ```sh
     (cd apps/mac && swift test --filter "Document|ThemeLint" 2>&1 | grep "Executed" | tail -1)
     ```
     Expect `Executed <n> tests, with 0 failures` (the count grows with every work package; do not compare it). The
     filter runs the XCTest suites DocumentAdjustmentAnalysisTests, DocumentAdjustmentJSONTests,
     DocumentAdjustmentModelTests, DocumentBlendModeTests, DocumentFiltersTests, DocumentKeyMapTests,
     DocumentKeyRoutingTests, DocumentOutlineTests, DocumentToolsTests, DocumentViewportMathTests,
     EngineDocumentBackendTests, StubDocumentBackendTests and ThemeLintTests (and any later `Document…` suite);
     `grep "Test Suite '.*' failed"` prints nothing. At B5-06 the count was 101.

### Part 2: over the real engine (B5-03)

Work on a copy of the fixture: `mkdir -p "$SCR/shoot" && cp fixtures/raw/sample.dng "$SCR/shoot/"` (the app writes
sidecars next to photos; never point it at `fixtures/raw`). Turn on **Debug ▸ Show Render Timing** for the readout.

143. **Blank document.** Launch without `--stub-library`:
     `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-doc" --folder "$SCR/shoot"`. ⌘N, **Create**: the
     sheet has no stub footer; one transparent pixel layer `Layer 1` (checkerboard over the whole canvas), selected,
     Properties `Pixel`, History `Opened` only, subtitle `1 layer`, status bar without `(stub backend: sample layers)`.
144. **Edit in Layers.** In the grid select `sample.dng`, press ⌘E (also right after step 143: click **Grid** in the
     toolbar first; ⌘E is Edit in Layers outside document mode and Merge Down inside it). The status bar reads `Edit sample.dng in Layers…`
     while the engine develops it (about 1.5–3 s), then a tab `sample` with one pixel layer `sample`, Properties
     `Bounds 0, 0 · 5212 × 3468 px`, status bar `5,212 × 3,468 px · 16-bit · sRGB IEC61966-2.1` and
     `render: L1 <w> × <h>, <ms>` (📸 `evidence/engine-01-edit-in-layers.png`).
145. **Adjustment layer.** Footer adjustment menu ▸ **Exposure**: a layer `Exposure 1` (engine names are numbered per
     kind, as in Photoshop: `Levels 1`, `Hue/Saturation 1`, `Color Fill 1`) above `sample`, History `New Layer Exposure 1`.
     Drag Exposure to +1.00: the photo brightens live; one `Exposure` row on release (📸 `engine-02-adjustment.png`).
146. **Opacity drag.** Select `sample`, drag **Opacity** to 40 %: the photo fades over the checkerboard live and the
     readout stays in single-digit milliseconds (budget < 16 ms at the viewport level); on release one row `Opacity 40 %`
     (📸 `engine-03-opacity-drag.png`). The layer thumbnail does not re-render during the drag (opacity is not part of it).
147. **Undo.** ⌘Z: opacity returns to 100 % (📸 `engine-04-undo.png`); ⇧⌘Z re-applies it. Click `Opened`: the document
     returns to one layer. **M** and a marquee drag: the ants hug the dragged rectangle exactly (bounds are pixel-exact),
     `Selection W × H` in the status bar, and History gains `Rectangular Marquee`; ⌘D adds `Deselect`.
148. **Save and reopen.** ⌘S → Save As `SelfTest.tessera-doc`: the dirty dot goes (📸 `engine-05-saved.png`). ⌘W, then
     **Open Document…** it: the same layers and the Exposure settings come back (📸 `engine-06-reopened.png`).
149. **Export Flat** (⇧⌘E) PNG, sRGB: a 5212 × 3468 PNG (📸 `engine-07-exported.png`). **Save As…** `SelfTest.psd`: the
     save succeeds (the engine writes PSD/PSB); close and open the PSD: `Exposure 1` (an adjustment layer) over `sample`,
     History `Opened` (📸 `engine-08-psd-open.png`). A document with fill layers cannot be saved as PSD yet (the status
     bar shows the engine's message).
150. **Scripted run.** The same flow through the controller calls the UI makes, with step markers and the frame timing
     of the listener, then quit:
     ```sh
     apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" --app-dir "$SCR/appdir-doc" \
       --document-selftest "$SCR/doc-out" 2>&1 | grep document-selftest
     ```
     Expect every `check … ok`, `opacity drag: frames 61, render median <16 ms`, and `done, 0 failure(s)`.
     `TESSERA_DOC_FRAME_LOG=1` prints every frame (`doc-frame: epoch … L1 … render … ms`) during manual drags.

### Part 3: filters, Image ▸ Adjustments and smart filters (B5-05)

Same scratch copy as part 2 (`$SCR/shoot/sample.dng`). In the grid select `sample.dng`, ⌘E (Edit in Layers).

151. **Filter menu.** The menu bar has **Image** and **Filter** in document mode. Filter lists **Last Filter** (⌃F,
     disabled until a filter was applied), **Convert for Smart Filters**, then Blur, Sharpen, Noise, Distort, Stylize,
     Render and Other, built from the engine's `list_filters()` (Gaussian Blur…, Box Blur…, Motion Blur…, Radial Blur…,
     Surface Blur…, Unsharp Mask…, Smart Sharpen…, Add Noise…, Reduce Noise…, Median…, Dust & Scratches…, Pinch…,
     Spherize…, Twirl…, Wave…, Ripple…, Polar Coordinates…, Emboss…, Find Edges, Solarize…, Clouds…, Difference
     Clouds…, High Pass…, Offset…). With an adjustment layer selected the groups are disabled.
152. **Gaussian Blur preview.** Filter ▸ Blur ▸ **Gaussian Blur…**: a dialog `Gaussian Blur`, `Layer “sample”`, a 1:1
     detail pane (drag it to move) and **Radius**. Drag Radius: the canvas blurs live at the viewport level and the
     detail pane follows; History does not change and the document stays as it was. Untick **Preview**: the canvas shows
     the original, the pane still shows the filter. **Reset** returns Radius to 2.0 px
     (📸 `tools/orchestrate/wp/B5-05/evidence/filters-01-gaussian-dialog.png`).
153. **Apply and undo.** Radius 12, **OK**: the status bar reads `Applying Gaussian Blur…`, then
     `Gaussian Blur applied (<s> s)`; History gains one row `Gaussian Blur` (📸 `filters-02-gaussian-applied.png`).
     ⌘Z restores the sharp photo (📸 `filters-03-gaussian-undone.png`). ⌃F applies Gaussian Blur 12 px again without a
     dialog; ⌘Z. With a marquee selection only the selection is filtered.
154. **Levels via Image ▸ Adjustments.** Image ▸ Adjustments ▸ **Levels…** (⌘L): the Levels editor of the Properties
     panel in a sheet; moving Input black / Gamma previews live on the canvas (📸 `filters-04-levels-dialog.png`).
     **OK**: one History row `Levels`, the pixels of `sample` change, no adjustment layer is added
     (📸 `filters-05-levels-applied.png`). Invert (⌘I) applies at once.
155. **Smart filter.** Filter ▸ **Convert for Smart Filters**: `sample` becomes a smart object (Properties `Smart
     Object`, History `Convert to Smart Object`). Filter ▸ Blur ▸ Gaussian Blur…, Radius 10, OK: History `Gaussian Blur`,
     and a row **Gaussian Blur** appears under `sample` with an eye, a white mask thumbnail and a blending-options button
     (📸 `filters-06-smart-filter-on.png`). Click its eye: the photo is sharp again, History `Disable Smart Filter`
     (📸 `filters-07-smart-filter-off.png`); click again: blurred, `Enable Smart Filter`
     (📸 `filters-08-smart-filter-on-again.png`). Double-click the row: the dialog reopens with Radius 10 and previews the
     re-edit; OK records `Edit Smart Filter`. The blending-options button sets mode and opacity (`Smart Filter Blending
     Options`). Save As `.tessera-doc`, close, reopen: the smart filter row and the blurred look come back; Export Flat
     bakes the smart filter at full resolution.
156. **Scripted run.**
     ```sh
     apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" --app-dir "$SCR/appdir-filters" \
       --filter-selftest "$SCR/filter-out" 2>&1 | grep filter-selftest
     ```
     Expect every `check … ok`, `gaussian preview latency (value → frame, viewport 5212 × 3468 at L1): n 12, median …`
     and `done, 0 failure(s)`.

### Part 4: the M5-26 / M5-28 adjustment layers (B5-06)

Same scratch copy (`$SCR/shoot/sample.dng`), engine backend. Select `sample.dng` in the grid, ⌘E. The Layer ▸ New ▸
Adjustment Layer menu and the Layers footer's adjustment menu (`document.layers.addAdjustment`) list, in Photoshop's
order and groups: Brightness/Contrast, Levels, Curves, Exposure | Vibrance, Hue/Saturation, Color Balance, Black & White,
Photo Filter, Channel Mixer, Color Lookup | Invert, Posterize, Threshold, Gradient Map, Selective Color | then the kinds
Photoshop offers only as Image ▸ Adjustments commands, here native adjustment layers: Shadows/Highlights, HDR Toning,
Desaturate, Match Color, Replace Color, Equalize, Auto. Image ▸ Adjustments lists Photoshop's five groups (Desaturate
⇧⌘U, Equalize and Invert apply at once), followed by Image ▸ Auto Tone (⇧⌘L), Auto Contrast (⌥⇧⌘L), Auto Color (⇧⌘B).

161. **Color Balance.** Footer adjustment menu ▸ **Color Balance**: `Color Balance 1` above `sample`, History
     `New Layer Color Balance 1`; Properties shows Shadows | Midtones | Highlights (`document.properties.colorBalance.tone`,
     on Midtones), three sliders Cyan – Red, Magenta – Green, Yellow – Blue (`….colorBalance.red|green|blue`) and
     **Preserve Luminosity** ticked. Drag Cyan – Red to +60: the photo warms live; one `Color Balance` row on release
     (📸 `tools/orchestrate/wp/B5-06/evidence/adj-01-color-balance.png`). Switch to Highlights, drag Yellow – Blue to
     +40: the highlights turn blue. ⌘Z twice: both return; ⇧⌘Z re-applies one.
162. **Gradient Map.** Footer menu ▸ **Gradient Map**: the photo maps black → white (a monochrome image). Properties
     shows the gradient preview, two stops with colour wells and positions (`document.properties.gradientMap.stop.<i>.*`),
     **Add Stop**, Method Perceptual | Linear | Classic (`….gradientMap.method`), **Dither** and **Reverse**. Pick a dark
     blue for stop 1 and an orange for stop 2: the picture turns duotone (📸 `adj-02-gradient-map.png`); tick Reverse: the
     mapping inverts; drag stop 2's position to 60 %: live, one `Gradient Map` row on release. ⌘Z: the position returns.
163. **Black & White.** Footer menu ▸ **Black & White**: the photo turns gray with Photoshop's default mix (Reds 40,
     Yellows 60, Greens 40, Cyans 60, Blues 20, Magentas 80, `….blackWhite.reds` …). Drag **Yellows** to 150: the warm
     wall brightens live; one `Black & White` row on release. **Auto** (`….blackWhite.auto`) sets a mix measured from the
     image; tick **Tint** (`….blackWhite.tint`): a sepia tint with a colour well (📸 `adj-03-black-white.png`). ⌘Z: the tint
     goes.
164. **Save and reopen.** ⌘S → the Save As sheet, `Adjustments.tessera-doc` into `$SCR`, Save. ⌘W, **Open Document…** it:
     `Black & White 1`, `Gradient Map 1` and `Color Balance 1` come back over `sample` with the same settings and the same
     look (📸 `adj-04-reopened.png`). Saving as PSD keeps Color Balance, Gradient Map and Black & White as Photoshop
     adjustment layers (`blnc`, `grdm`, `blwh`); Shadows/Highlights, HDR Toning, Desaturate, Equalize, Auto, Match Color and
     Replace Color are native-only and need a `.tessera-doc` (the status bar shows the engine's message).
165. **The other editors.** Each new layer's Properties editor drags live and records one row on release: Brightness /
     Contrast (**Use Legacy**), Vibrance, Photo Filter (Filter pop-up with the 20 engine swatches, colour, Density,
     Preserve Luminosity), Selective Color (Colors pop-up Reds … Blacks, Cyan / Magenta / Yellow / Black, Relative |
     Absolute), Desaturate (no settings), Equalize and Auto (measured from the image below the layer when added; **Analyze
     Again**; Auto's Tone | Contrast | Color and Shadows / Highlights Clip %, saved with the document), Match Color (Source pop-up of pixel layers, Luminance, Color
     Intensity, Fade, Neutralize, saved with the document), Replace Color (colour well, **Use Foreground** after picking with the Eyedropper,
     Fuzziness, Hue / Saturation / Lightness), Color Lookup (**Load 3D LUT…** for `.cube` / `.3dl`; the file name and
     the **Dither** checkbox are saved with the document, as are the samples), Shadows/Highlights (Amount / Tone / Radius for shadows and
     highlights, Color, Midtone, Black / White Clip) and HDR Toning (Method pop-up; Local Adaptation: Radius, Strength,
     Gamma, Exposure, Detail, Shadow, Highlight, Vibrance, Saturation and a toning curve).

## Verdict (document mode)

PASS when steps 130–142 (stub), 143–150 (engine), 151–156 (filters) and 161–165 (new adjustment layers) meet their expectations. Record the stub render time on a
large window (drag Opacity on `Landscape` at 100 %) as an observation; the stub renders on the CPU and is not held to the
engine's budget.

## W. Layered editor tools: painting, selections, transform (B5-04)

Engine backend (not `--stub-library`), a scratch copy of `fixtures/raw/sample.dng` in `$SCR/shoot`, opened with
Library ▸ Edit in Layers (⌘E). The tools palette sits on the left of the canvas, the options bar across the top.

151. **Palette and keys.** V M L W B E S J G C T I H Z select Move, Rectangular Marquee, Lasso, Quick Selection, Brush,
     Eraser, Clone Stamp, Healing Brush, Gradient (placeholder), Crop (placeholder), Type (placeholder), Eyedropper,
     Hand, Zoom; ⇧M / ⇧L / ⇧W cycle Elliptical Marquee, Polygonal / Magnetic Lasso, Magic Wand / Object Selection;
     right-click a palette slot lists its group. The options bar follows the tool. X swaps and D resets the swatches.
152. **Brush stroke visible and undoable.** B, foreground red (click the foreground swatch), Size 80. Drag across the
     photo: the stroke follows the pointer while dragging, the outline circle shows the brush size, and History gains
     one `Brush Tool` row (📸 `tools-2-brush-stroke.png`). ⌘Z removes it (📸 `tools-3-brush-undo.png`), ⇧⌘Z restores
     it. With Debug ▸ Show Render Timing the status bar shows `Brush Tool: N frames, median … ms` and the render readout.
     [ ] resize the brush, ⇧[ ⇧] change hardness, 1–0 set opacity (4 then 5 = 45 %); ⌃-drag (or ⌥-right-drag) shows the
     HUD: right = larger, up = harder. With a tablet, pressure narrows the stroke (Size pressure is on by default).
     ⇧-click draws a straight line from the last stroke. Symmetry ▸ Vertical mirrors about the centre with a guide.
153. **Eraser to transparency.** E, Size 200, drag over the photo: the checkerboard shows through; one `Eraser` row
     (📸 `tools-4-eraser.png`).
154. **Clone and heal.** S, ⌥-click a source, paint elsewhere: the source crosshair follows the brush and the pixels are
     copied (aligned); one `Clone Stamp` row. J does the same, blended into the surroundings (`Healing Brush`).
155. **Wand selection outline.** ⇧W until Magic Wand, Tolerance 24, click the wall: marching ants follow the region's
     outline (not its bounding box) and the status bar shows `Selection W × H`; one `Magic Wand` row
     (📸 `tools-5-wand.png`). ⇧-click adds, ⌥-click subtracts, ⇧⌥ intersects (also the four options-bar buttons).
     Marquee, ellipse, lasso, polygonal (click points, double-click or Return closes, Esc cancels) and magnetic lasso
     (edge-snapping path while moving) combine the same way; ⌫ clears the selected pixels of a pixel layer.
156. **Subject selection.** Select ▸ Subject (first run loads the on-device model): the subject's outline appears; one
     `Select Subject` row (📸 `tools-6-subject.png`). Select ▸ Sky, Color Range…, Inverse (⇧⌘I), Modify ▸ Expand… /
     Contract… / Border… / Smooth… / Feather…, Select and Mask… (⌥⌘R: Overlay / On Black / On White preview, sliders
     update live, OK records one `Refine Edge` row, Cancel restores), Save Selection… / Load Selection ▸ work.
157. **Transform commit.** Select the photo layer, ⌘T: a box with eight handles and the reference point. Drag a corner
     (⇧ keeps proportions, ⌥ scales about the centre), drag outside the box to rotate (⇧ snaps 15°), drag inside to move;
     the options bar shows X, Y, W %, H %, angle and skew and accepts typed values (📸 `tools-7-transform-preview.png`).
     Return commits one `Free Transform` row (📸 `tools-8-transform-commit.png`), Esc cancels. The Move tool (V) drag
     moves the layer (one `Free Transform` row); Edit ▸ Transform ▸ Flip / Rotate apply directly.
158. **PSD save and reopen.** File ▸ Save As… `ToolsSelfTest.psd`, close, File ▸ Open Document… the PSD: the same
     layers, with the painted, erased and transformed pixels (📸 `tools-9-psd-reopen.png`).
159. **Brushes and colour panels.** With a painting tool the inspector shows Color (foreground / background wells) and
     Brushes (Hard / Medium / Soft Round, Chalk, Square and imported tips with previews, Import Brushes… for `.abr`,
     size / hardness / spacing / angle / roundness). The options bar's brush button opens the same preset list.
160. **Scripted run.**
     ```sh
     apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" --app-dir "$SCR/appdir" \
       --tools-selftest "$SCR/out" 2>&1 | grep tools-selftest
     ```
     Expect every `check … ok`, `brush stroke: … frame render … median <16 ms`, and `done, 0 failure(s)`.

## Verdict (layered editor tools)

PASS when steps 151–160 meet their expectations and the brush frames' median render time is under 16 ms.

## Appendix: accessibility identifiers (B5-02)

| Identifier | Element |
| --- | --- |
| `document.viewport` · `document.zoomHUD` · `document.tool.move` · `document.tool.marquee` | Viewport, zoom chip and tool bar |
| `document.tabs` · `document.tabs.<n>` · `document.tabs.<n>.dirty` · `document.tabs.<n>.close` · `document.tabs.new` | Document tabs |
| `document.layers` · `document.layers.outline` · `document.layers.row.<index>` · `document.layers.row.<index>.visibility` · `.name` · `.thumbnail` · `.mask` · `.maskLink` | Layers outline and rows (`<index>` = outline row, top = 0) |
| `document.layers.blendMode` · `document.layers.opacity` · `document.layers.fill` · `document.layers.lock.{transparency,pixels,position,all}` · `document.layers.filter` | Layers header |
| `document.layers.add` · `document.layers.addMask` · `document.layers.addAdjustment` · `document.layers.group` · `document.layers.delete` | Layers footer |
| `document.properties` · `document.properties.name` · `.kind` · `.bounds` · `.groupMode` · `.channel` · `.levels.*` · `.curves.editor` · `.curves.reset` · `.hueSaturation.*` · `.exposure.*` · `.posterize.levels` · `.threshold.level` · `.channelMixer.*` · `.fill.*` · `.editContents` | Properties panel |
| `document.history` · `document.history.row.<index>` (0 = Opened) · `document.history.snapshot.<n>` · `document.history.snapshot.<n>.restore` · `document.history.newSnapshot` · `document.history.memory` | History panel |
| `document.inspector.tabs` · `document.inspector.stack` · `document.properties` · `document.channels` · `document.inspector.shortcut.{stack,properties,channels}` · `document.history.toggle` · `document.history.resize` · `document.tabs.overflow` | Inspector sub-tabs, their contents, ⌃1–⌃3, History header and resize handle, tab overflow (B5-16) |
| `document.new.*` · `document.export.*` · `document.empty.new` · `document.empty.open` · `document.status.*` | Sheets, empty state, status bar |
| `document.tools` · `document.tool.<tool>` · `document.colors` · `document.optionsBar` · `document.option.*` · `document.transform.commit` · `document.colorPanel` · `document.color.{foreground,background}` · `document.brushes` · `document.brushes.import` · `document.brush.*` · `document.status.stroke` | Tools palette, options bar, Color and Brushes (B5-04) |
| `document.filter.<id>.<key>` (e.g. `document.filter.gaussian_blur.radius`, `.dial` for angles) · `document.filter.<id>.detail` · `.preview` · `.reset` · `.cancel` · `.ok` | Filter dialogs (B5-05) |
| `document.adjustment.<kind>` · `.preview` · `.reset` · `.cancel` · `.ok` | Image ▸ Adjustments sheets (the editor inside keeps its `document.properties.*` identifiers) |
| `document.saveAs.name` (first responder when the sheet opens) · `document.saveAs.format` · `document.saveAs.folder` · `document.saveAs.choose` · `document.saveAs.save` · `document.saveAs.cancel` | File ▸ Save As… sheet (B5-06) |
| `document.properties.<kind>.<control>`: `brightnessContrast.{brightness,contrast,legacy}` · `vibrance.{vibrance,saturation}` · `colorBalance.{tone,red,green,blue,preserveLuminosity}` · `blackWhite.{auto,default,reds,yellows,greens,cyans,blues,magentas,tint,tintColor}` · `photoFilter.{preset,color,density,preserveLuminosity}` · `gradientMap.{gradientPreview,stop.<i>.{color,position,remove},addStop,method,dither,reverse}` · `selectiveColor.{colors,cyan,magenta,yellow,black,method}` · `equalize.analyze` · `auto.{mode,shadowClip,highlightClip,black,white,gamma,analyze}` · `matchColor.{source,luminance,colorIntensity,fade,neutralize}` · `replaceColor.{color,useForeground,fuzziness,hue,saturation,lightness}` · `colorLookup.{file,load,reset,dither}` · `shadowsHighlights.{shadowsAmount,shadowsTone,shadowsRadius,highlightsAmount,highlightsTone,highlightsRadius,color,midtone,blackClip,whiteClip}` · `hdrToning.{method,radius,strength,gamma,exposure,detail,shadows,highlights,vibrance,saturation,curve,analyze}` | Properties editors of the M5-26 / M5-28 adjustments (B5-06) |
| `document.layers.smartFilter.<layer>.<index>` · `.visibility` · `.mask` · `.name` · `.blending` · `document.smartFilter.blending.{mode,opacity,ok}` | Smart filter rows and their blending options |

## X. Develop: AI Denoise, Transform / Upright, Lens Blur (M2-48, M2-51)

Engine backend, a scratch copy of `fixtures/raw` in `$SCR/raw` (step 2). Select **sony-arw.ARW** and press **Return**
(loupe). The three controls live in DETAIL (AI Denoise, top of Noise Reduction), TRANSFORM (between Detail and Effects)
and LENS BLUR (after Effects). Since M2-49 / M2-51 the viewport draws Upright/Transform, and AI Denoise and Lens Blur are
enabled: each acquires its pinned model on first use with inline progress (section AC). Remaining engine gaps, shown
disabled with the reason: the Lens Blur Refine brushes and Constrain Crop. Model weights may be absent on a test Mac;
then the expected result is the inline failure with its reason (tools/orchestrate/wp/M2-51/REPORT.md). Screenshots:
`tools/orchestrate/wp/M2-48/evidence/`, `tools/orchestrate/wp/M2-51/evidence/`.

170. **Detail ▸ AI Denoise.** Open DETAIL. 📸 Expect under **Noise Reduction** an enabled **AI Denoise** checkbox, a dimmed
     **Amount** slider and the hint `First use downloads the denoise model; the loupe refines when it is ready.` Tick
     AI Denoise: the checkbox stays ticked while an inline bar under it reads `AI Denoise model: Queued…` then
     `Downloading … of …` (or goes straight to Ready from the cache). On **Ready** the History lists `AI Denoise On`,
     Amount enables and the loupe refines. If the model cannot be acquired (e.g. the CFA artifact is not packaged on this
     Mac) the box unticks, nothing is recorded, and a warning line `AI Denoise model: <reason>` with **Retry** appears. The
     classic Luminance / Detail / Contrast and Color / Color Detail / Smoothness sliders below are unchanged (Luminance 40
     smooths the 1:1 preview; History `Luminance NR 40`). Unticking a ticked AI Denoise records `AI Denoise Off`.
171. **Upright buttons.** Open TRANSFORM. 📸 Expect an **Upright** sub-header with a borderless **Reset**, a six-segment bar
     (Off · Auto · Guided · Level · Vertical · Full; the chosen one shows its name, the others their icon, help tags name
     each), a one-line description of the chosen mode, then **Transform** with its own **Reset**, the sliders Vertical,
     Horizontal, Rotate, Aspect, Scale (100%), Offset X, Offset Y and a dimmed **Constrain Crop** checkbox with the warning
     `Constrain Crop is not rendered by the engine yet…`. Click **Auto**: History lists `Upright: Auto` and the loupe
     redraws the corrected photo (no "does not draw" warning). Click **Vertical**, then **Off**: one history step each
     (`Upright: Vertical`, `Upright: Off`), the loupe following each.
172. **Manual transform.** Drag **Vertical** to +20 and release: exactly one History row `Transform Vertical +20` (not one
     per drag frame) and the loupe shows the keystone. Drag **Rotate** (one decimal, `°`) and **Scale** (`%`, 50–150);
     double-click a slider to reset it. Click the Transform **Reset**: all seven sliders return to neutral in one step
     `Reset Transform`; Upright is untouched. **Constrain Crop** cannot be ticked (engine gap); on a recipe that already
     has it on it is enabled so it can be unticked (`Constrain Crop Off`). ⌘Z undoes each step.
173. **Guided Upright.** Set Vertical to +20 first. Click **Guided** (the loupe must be showing the photo). Expect the loupe
     to switch to the **uncorrected** photo (no keystone, no crop) while the tool is armed, a hint at the bottom of the
     loupe `Guided Upright · draw 2 more guides …`, the panel line `0 of 4 guides` with **Clear** and **Done**, the hint
     `The loupe shows the uncorrected photo while you place guides…`, and a crosshair cursor over the photo. Drag along
     a vertical edge: a white guide with square ends appears (dashed while drawing). Nothing is recorded yet. Draw a
     second guide: History lists `Upright: Guided (2 guides)` and the hint reads `… 2 of 4 guides …`. 📸 Drag an end of
     the first guide (cursor ✋): on release one step `Upright: Guided (2 guides)`. Click a guide's line: it turns amber
     (selected); press **⌫**: it disappears and, with one guide left, History records `Upright: Off`. A fifth guide is
     refused with a status message. **Return** (or **Done**/**Esc**) leaves the tool and the loupe returns to the
     corrected picture; culling keys do nothing while it is armed, ⌘Z still works. Reopening Guided shows the stored
     guides on the uncorrected photo, where they were drawn.
174. **Lens Blur.** Open LENS BLUR. 📸 Expect an enabled **Apply** checkbox, a dimmed **Blur Amount** slider, a **Bokeh**
     pop-up (Circle · Bubble · 5-Blade · Hexagon · Octagon · Ring · Cat Eye · Oval), the **Focal Range** strip (dark scope
     well, near → far ramp, `Near` / `Far` labels, the in-focus band with two handles), a **Visualize Depth** checkbox and a
     **Subject** button (dimmed until Apply), and a **Refine** row with Focus / Blur brush buttons, a `Later` chip and the
     warning `Focus / Blur refine brushes come later…` (always dimmed: no engine brush). Tick **Apply**: the depth weights
     are acquired first (inline `Depth model: …` bar); on Ready History lists `Lens Blur On`, `Estimating depth…` shows,
     then the depth histogram is drawn in the strip. Without weights (downloads off or offline) Apply stays unticked
     and a warning `Depth model: <reason>` line with **Retry** appears. See section AC for the depth tools.
175. **Keyboard focus (M2-27 rules).** Tab to a Transform slider: amber focus outline, arrows nudge it without moving the
     loupe selection, Return/Esc commit and blur (one history step per burst).
176. **Export applies Upright.** With Upright **Auto** set, export the ARW (File ▸ Export…, PNG, long edge 640) and
     compare with an export at **Off**: the Auto file's verticals are corrected (the Swift test
     `TransformSessionTests.testUprightAutoChangesTheRenderedFrame` asserts the frames differ). The loupe matches the
     export.

## Verdict (AI Denoise, Transform, Lens Blur)

PASS when steps 170–176 meet their expectations. Record missing model weights (an inline failure with its reason) and
the remaining engine gaps (Refine brushes, Constrain Crop) as known limitations, not failures.

## Appendix: accessibility identifiers (M2-48, M2-51)

| Identifier | Element |
| --- | --- |
| `detail-ai-denoise` · `detail-ai-denoise-amount` · `detail-ai-denoise-model` · `detail-ai-denoise-model-retry` · `detail-ai-denoise-ignored` | Detail ▸ Noise Reduction ▸ AI Denoise toggle, Amount, model progress / failure row and its Retry, "kept but not drawn" line |
| `transform-upright` · `transform-upright-{off,auto,guided,level,vertical,full}` · `transform-upright-reset` | Transform ▸ Upright bar, its six buttons and group Reset |
| `transform-guides-count` · `transform-guides-clear` · `transform-guides-done` · `transform-guides-uncorrected` | Guided Upright status row (while the loupe tool is armed); the "uncorrected photo" hint |
| `transform-vertical` · `transform-horizontal` · `transform-rotate` · `transform-aspect` · `transform-scale` · `transform-offset-x` · `transform-offset-y` · `transform-reset` | Transform ▸ manual sliders and group Reset |
| `transform-constrain-crop` · `transform-constrain-crop-unavailable` · `transform-preview-note` | Constrain Crop checkbox, its engine-gap hint; the "kept but not drawn" warning |
| `lensblur-apply` · `lensblur-amount` · `lensblur-bokeh` · `lensblur-focal-range` | Lens Blur ▸ Apply, Blur Amount, Bokeh pop-up, Focal Range strip (with the depth histogram) |
| `lensblur-model` · `lensblur-busy` · `lensblur-error` | Depth / subject model progress row, "Estimating depth…" / "Finding the subject…", inline error |
| `lensblur-visualize-depth` · `lensblur-subject` | Visualize Depth checkbox, Subject button |
| `lensblur-refine-focus` · `lensblur-refine-blur` · `lensblur-refine-unavailable` | Refine brushes (disabled: engine gap) and the reason |

## Y. Persistent alpha and spot channels (B5-08)

Engine backend (not `--stub-library`), a scratch copy of `fixtures/raw/sample.dng` in `$SCR/shoot`, opened with
Library ▸ Edit in Layers (⌘E). Choose the inspector's **Channels** sub-tab (⌃3; B5-16, formerly a section between
Layers and History).

168. **Panel.** Channels lists RGB, Red, Green, Blue (read-only: lock glyph, thumbnails from the composite) and nothing
     else for a new document. Hiding RGB turns the canvas to the ink backdrop; hiding only Green shows the composite
     without green; hiding two components shows the remaining one as grey. Showing RGB again restores the photo.
169. **Save a selection.** Rectangular Marquee over the top of the photo, Select ▸ Save Selection…: the sheet offers
     Channel `New`, Name `Alpha 1` (type `Sky`) and the single operation `New Channel`; OK adds a `Sky` row with a
     black / white thumbnail and one History row `Save Selection`. Choosing an existing channel in the sheet
     disables Name and offers Replace / Add to / Subtract from / Intersect with Channel.
170. **Close and reopen.** File ▸ Save As… `Channels.tessera-doc`, close the tab, File ▸ Open Document… the file:
     the `Sky` row is back (same name and thumbnail). Repeat with `Channels.psd`: Photoshop-compatible alpha channel.
171. **Load it back.** Select ▸ Deselect (⌘D), then Select ▸ Load Selection…: Channel `Sky`, Invert off, New
     Selection, OK: the marching ants trace the saved rectangle and History adds `Load Selection`. ⌘-click the `Sky`
     row does the same (⇧⌘ adds, ⌥⌘ subtracts, ⇧⌥⌘ intersects); the row's context menu lists the four loads,
     Duplicate Channel, Rename…, Channel Options…, Delete Channel. Double-click the name renames it (one
     `Rename Channel` row); names may repeat, and the sheets tell duplicates apart by position.
172. **Spot channel.** Marquee a second area, Channels footer `+` ▸ New Spot Channel…: name `Spot Color 1`, an ink
     colour, Solidity, From the selection; the sheet shows the note that spot colour is preview-only. OK adds a row
     with the ink swatch and History `New Spot Channel`. Click its eye: the ink appears over the area at its
     solidity; the alpha eye shows red over the masked areas (Channel Options ▸ Selected Areas flips it).
173. **Undo.** ⌘Z removes the spot channel row (and its overlay), ⇧⌘Z restores it; every channel edit (save, rename,
     duplicate, options, delete) is one History row that undoes the same way.
174. **RGB export unchanged.** With the spot and alpha channels present and visible, File ▸ Export… PNG; compare with an
     export made before adding them: the pixels are identical (the Rust test
     `spot_channels_do_not_change_the_rgb_composite` asserts it byte for byte, also after a PSD round trip).
175. **Quick Mask.** With a selection press Q: a `Quick Mask` row with a `Temporary` chip appears, the Quick Mask
     footer button is on and the canvas shows red over the unselected area. Save Selection into `Quick Mask` with
     Add / Subtract edits it. Press Q again: the mask becomes the selection and the temporary row disappears.
     Select ▸ Edit in Quick Mask Mode does the same.
176. **Scripted run.**
     ```sh
     TESSERA_CHANNELS_SELFTEST="$SCR/out" apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" \
       --app-dir "$SCR/appdir" --open-document "$SCR/photo.png" --front 2>&1 | grep channels-selftest
     ```
     (`photo.png`: `sips -s format png sample.dng --out photo.png`.) Expect every `check … ok` and
     `done, 0 failure(s)`; each `step` line gives the window rectangle for `screencapture -R`.

## Verdict (channels)

PASS when steps 168–176 meet their expectations. Known limitations: painting directly into a channel (and so into
Quick Mask) needs brush support for channel targets; Quick Mask is edited with Save Selection into it. Alpha overlay
colour / opacity and channel visibility are session preferences, not saved.

## Appendix: accessibility identifiers (B5-08)

| Identifier | Element |
| --- | --- |
| `document.channels` · `document.channels.list` | Channels sub-tab (B5-16), its row list |
| `document.channels.rgb` · `document.channels.component.{0,1,2}` · `document.channels.channel.<id>` | Rows (`….eye`, `….swatch`, `….rename`) |
| `document.channels.load` · `document.channels.save` · `document.channels.quickMask` · `document.channels.add` · `document.channels.delete` | Footer |
| `document.channels.save.{channel,name,operation}` · `document.channels.load.{channel,invert,operation}` | Save / Load Selection sheets |
| `document.channels.options.{name,kind,indicates,color,opacity}` · `document.channels.spot.{name,color,solidity,fromSelection}` · `document.channels.spotNote` · `document.channels.sheet.ok` | Channel Options, New Spot Channel |
## Z. Remove tool, Content-Aware Fill and neural filters (B5-09)

Engine backend (not `--stub-library`), a scratch copy of `fixtures/raw/sample.dng` in `$SCR/shoot`, opened with
Library ▸ Edit in Layers (⌘E), the photo layer selected. Model weights are never downloaded on their own: with no
weights installed (the default), expect the missing-model messages below. B5-09b: LaMa, DDColor and DRUNet download
through the app's model downloads (the M2-51 flow of AI Denoise) only when asked for (steps 334–337), into
`<app support>/models/cache/<sha256>.onnx`, where retouching looks, and only if Settings ▸ AI ▸ Allow model downloads is
on. Steps 320–333 assume the model is not installed.

320. **Remove tool.** Press ⇧J (or click the palette slot under the Healing Brush, icon `eraser.line.dashed`): the slot
     turns amber, the options bar reads `Remove` with Size, the Auto · PatchMatch · LaMa picker, Expand, Remove
     Selection and Remove Distractions…; the status bar reads `Remove (⇧J)`. Without LaMa installed the bar shows
     `LaMa not installed` (its help names `remove/lama`, the Hugging Face URL and the cache path) and **Download LaMa**
     (with model downloads off: `LaMa not installed · model downloads are off` and **Settings ▸ AI…**). [ ] change the size.
321. **Remove an object by a stroke.** Size about 1/40 of the long edge, Auto. Drag over a small object: a translucent
     red band at brush width follows the pointer (📸 `retouch-01-remove-stroke.png`). On release the options bar shows a
     spinner, the elapsed seconds and Cancel; then the object is filled from its surroundings, History gains exactly one
     `Remove` row and the status bar reads `Remove: PatchMatch, N s (LaMa (remove/lama) is not installed, so Auto used
     PatchMatch)` (📸 `retouch-02-remove-applied.png`). With LaMa installed it reads `Remove: LaMa, …`.
322. **Undo, redo, reopen.** ⌘Z restores the object (one step; 📸 `retouch-03-remove-undone.png`), ⇧⌘Z removes it
     again. File ▸ Save As… a `.tessera-doc`, close it, open it again: the removal is there (📸 `retouch-04-reopened.png`).
323. **Remove with LaMa, weights missing.** Settings ▸ AI ▸ Allow model downloads **off**. Choose LaMa and stroke again:
     nothing changes, no History row, the stroke band disappears and the options bar shows the warning `Remove needs
     remove/lama, which is not installed, and model downloads are off (Settings ▸ AI). Nothing was downloaded.` (its help
     has the whole line). Nothing is downloaded. (With downloads on, see 334.)
324. **Remove Selection.** Make a marquee around an object, click Remove Selection: one `Remove` row; outside the
     selection nothing changes.
325. **Edit ▸ Content-Aware Fill.** Make a small marquee, Edit ▸ Content-Aware Fill (disabled without a selection or on
     an adjustment layer): the selection is filled from its surroundings; one `Content-Aware Fill` row
     (📸 `retouch-05-content-aware-fill.png`).
326. **Start and cancel a slow job.** PatchMatch, marquee most of the image, Remove Selection: the options bar shows the
     spinner, a counting `Removing… N s` and Cancel (📸 `retouch-06-slow-job-running.png`). Click Cancel (or press Esc):
     the bar returns to the options **at once** (B5-09b; no `Cancelling…` state), the status bar reads `Remove cancelled`,
     History and the pixels are unchanged (📸 `retouch-07-slow-job-cancelled.png`). See 338 for what happens while the
     engine is still stopping the job.
327. **Remove Distractions: review first.** Deselect (⌘D), click Remove Distractions…: boxes appear over the canvas, one per
     suggestion, with chips `Wire-like line` / `Face box`; the bar reads `N of N suggestions selected`, All, None, the note
     `geometric suggestions, not person segmentation`, Cancel and Remove Selected.
     No History row yet. Click a box: it turns dashed and its chip reads `(kept)`. Remove Selected (or Return) removes only
     the boxes still selected: one `Remove Distractions` row. Esc cancels the review. Without the face detector weights
     (`opencv/yunet`) no face boxes are suggested, and `sample.dng` has no thin straight wire, so there the bar reads
     `Nothing found` and the status bar names the missing face model (📸 `retouch-08-distractions-review.png`).
328. **Neural Filters: Colorize.** Filter ▸ Neural Filters…: a sheet lists Skin Smoothing, Colorize and JPEG Artifact
     Removal (the last two with a `No model` chip when their weights are missing). Choose Colorize: its
     Saturation / Artifact Reduction sliders, the Output picker, the note `CPU-only DDColor; …`, and the warning
     `Colorize needs the filters/ddcolor model, which is not installed.` with the Hugging Face URL and cache path; the
     button reads **Download and Apply** (📸 `retouch-09-neural-colorize.png`; with model downloads off: `… and model
     downloads are off.`, **Settings ▸ AI…**, Apply disabled; see 336). With the model installed: Apply colorizes as one
     `Colorize` row.
329. **Neural Filters: JPEG Artifact Removal.** Same, naming `enhance/drunet-color` (📸 `retouch-10-neural-jpegArtifactRemoval.png`).
330. **Neural Filters: Skin Smoothing.** Needs no weights. Without a selection and without the face detector the sheet says
     `The face detector (opencv/yunet) is not installed: select a face first …` (📸 `retouch-11-neural-skin-no-faces.png`);
     Apply then fails with `Skin Smoothing needs face boxes …` and records nothing. Cancel, make an elliptical marquee
     around a face, reopen, Output **New layer**, Apply: a new layer `<layer> (Skin Smoothing)` above the photo, one
     `Skin Smoothing` row (📸 `retouch-12-neural-skin-new-layer.png`).
331. **Outputs.** On a pixel layer Output offers Current layer, New layer and Smart filter (Smart filter only without a
     selection; the sheet lists why an output is unavailable). **Smart filter** converts the layer into a smart object
     with the filter in the same single History row; the filter appears under the layer in the Layers panel.
     Double-click that row: the Neural Filters sheet reopens on the same filter with its values (Output fixed to Smart
     filter); OK records `Edit Smart Filter`. On a smart object, New layer is unavailable.
332. **Outline after deselect.** Make a large wand selection and press ⌘D immediately: the marching ants disappear and do
     not come back when the outline computation finishes.
333. **Scripted run.** Background only: the app is launched with `open -g` and never activated or raised
     (`--new-document` starts the self-test from the document view, since a background app never builds its menu bar).
     ```sh
     open -g -n --stderr "$SCR/retouch.log" apps/mac/build/Tessera.app --args --folder "$SCR/shoot" \
       --app-dir "$SCR/appdir" --new-document --retouch-selftest="$SCR/out"; grep retouch-selftest "$SCR/retouch.log"
     ```
     Expect the step lines above (each names its window number for `screencapture -x -o -l <n>`), every `check … ok`,
     the measured `remove stroke: backend … engine … ms`, `cancel returned in … ms`, `the cancelled job returned after
     … s` and `done, 0 failure(s)`. The download steps (334–337) use a local stand-in for the downloader that reports
     progress and writes nothing, and a scratch preference suite for the Settings toggle.
334. **Download LaMa, then Remove runs by itself (B5-09b).** Fresh `--app-dir`, Allow model downloads **on**. Remove tool,
     LaMa, stroke over an object: the band stays; the options bar shows `LaMa model: Queued…`, then a determinate bar
     `LaMa model: Downloading N MB of 208 MB` and `Remove runs when it is ready` (📸 `retouch-15-download-progress.jpg`).
     When it completes the Remove runs without another click: one `Remove` row, status `Remove: LaMa, N s`. **Download
     LaMa** in the bar downloads without running anything. The file lands at the path the bar's help names
     (`<app dir>/models/cache/1faef530….onnx`); relaunch: `LaMa not installed` is gone and Auto reports `Remove: LaMa`.
335. **Download failure.** Disconnect the network, fresh `--app-dir`, stroke with LaMa: the bar shows `LaMa model:
     Failed: …` with **Retry**; no History row, the stroke is dropped. Reconnect, Retry: it downloads (the Remove has to
     be painted again).
336. **Downloads off.** Settings ▸ AI ▸ Allow model downloads off. Remove bar: `LaMa not installed · model downloads are
     off` and **Settings ▸ AI…** (opens Settings; the toggle is on its AI tab) (📸 `retouch-14-download-off.jpg`). Neural Filters ▸
     Colorize: `Colorize needs the filters/ddcolor model, which is not installed, and model downloads are off.`,
     **Settings ▸ AI…**, Apply disabled. Nothing downloads anywhere; switching the toggle on turns the button back into
     Download and Apply.
337. **Neural Filters: Download and Apply.** Allow model downloads on. Colorize ▸ **Download and Apply**: the sheet shows
     `Colorize model: Downloading …` and `Colorize applies when the download completes`
     (📸 `retouch-17-download-neural-progress-sheet.jpg`); then the filter applies and the sheet closes with one `Colorize`
     row. Same for JPEG Artifact Removal (`enhance/drunet-color`). Cancel while downloading forgets the apply (the download
     continues into the cache).
338. **Cancel does not wait (B5-09b).** Repeat 326 on 80 % of `sample.dng` (PatchMatch takes minutes to notice the
     cancel; M5-33 is Machine A's). Right after Cancel: the bar is idle and shows `Stopping the cancelled Remove… N s`
     (📸 `retouch-07-slow-job-cancelled.jpg`). Remove Selection (or a stroke, or Edit ▸ Content-Aware Fill) now is refused
     with `The cancelled Remove is still stopping in the engine (N s); try again when it has stopped`
     (📸 `retouch-08-slow-job-refused.jpg`). When the engine returns, the note disappears, the status bar reads `Remove
     cancelled; the engine job has stopped`, History is unchanged (a result the engine committed before it saw the cancel
     is undone), and a new Remove runs.
339. **Edit ▸ Content-Aware Fill enablement (B5-09b).** Marquee tool (M), drag a small rectangle on the photo layer: Edit ▸
     Content-Aware Fill is enabled and fills (one row). ⌘D: disabled. While a Remove runs: disabled; right after its
     Cancel: enabled again (it used to stay disabled until the engine returned, minutes after a large Cancel).

## Verdict (Remove tool and neural filters)

PASS when steps 320–339 meet their expectations: every apply is one History row, cancel and every error leave History
unchanged, Cancel returns to idle at once, missing models are named with their source, models download only when asked
and only with Allow model downloads on, and nothing is simulated.

## Appendix: accessibility identifiers (B5-09)

| Identifier | Element |
| --- | --- |
| `document.tool.remove` | Remove tool palette slot |
| `document.remove.size` · `document.remove.backend` · `document.remove.dilation` | Options bar: Size, backend picker, Expand |
| `document.remove.selection` · `document.remove.distractions` · `document.remove.cancel` · `document.remove.error` | Remove Selection, Remove Distractions…, Cancel (while busy), error line |
| `document.remove.download` · `document.remove.downloads-off` · `document.remove.settings` · `document.remove.model` · `document.remove.waiting` | B5-09b: Download LaMa, downloads-off warning, Settings ▸ AI…, inline progress row (`-retry`), "Remove runs when it is ready" |
| `document.remove.stopping` · `document.remove.notice` | B5-09b: cancelled job still stopping; refusal / download notice |
| `document.neural.model` · `document.neural.settings` · `document.neural.stopping` | B5-09b: sheet download progress row, Settings ▸ AI…, cancelled job still stopping |
| `document.remove.review.summary` · `document.remove.review.apply` · `document.remove.review.cancel` | Distraction review bar |
| `document.neural.filter.<kind>` · `document.neural.<kind>.<key>` · `document.neural.output` | Neural Filters list rows, sliders, Output picker |
| `document.neural.missing` · `document.neural.error` · `document.neural.apply` · `document.neural.cancel` · `document.neural.reset` | Missing-model block, error block, footer buttons |

## AA. Export: AVIF, JPEG XL, DNG, file size limit and watermarks (M2-46)

Continues section O (engine backend, `$SCR/raw3` open, three photos selected, File ▸ Export… open). Only what the
engine writes today is offered; the rest is shown disabled with the reason. Known gaps: tools/orchestrate/wp/M2-46/REPORT.md.

500. **Tests.**
     ```sh
     (cd apps/mac && swift test --filter 'ExportFormatsWatermarkTests|ExportPrintTests|ThemeLintTests' 2>&1 | grep Executed)
     ```
     Expect `Executed 7 tests, with 0 failures`, `Executed 9 tests, with 0 failures` and `Executed 1 test, with 0 failures`
     (real AVIF / JPEG XL / DNG exports, size-limit convergence, text and graphic watermarks, preset round trips).
501. **Formats.** 📸 File Settings ▸ Format shows six segments: **JPEG · PNG · TIFF · AVIF · JPEG XL · DNG**. Choose
     **AVIF**: Quality, Bit depth **8-bit · 10-bit · 12-bit** and Speed (1–10) appear; a dimmed **HDR output** checkbox
     with the hint `HDR output (PQ / HLG, gain maps) is not available yet…`. The summary reads `… AVIF <q> 8-bit …`.
502. **JPEG XL.** Choose **JPEG XL**: Compression reads **Lossless** (no Quality slider), Bit depth **8-bit · 16-bit**, the
     hint `Lossy JPEG XL is not available yet…`; Colour space is dimmed on **sRGB** with `Lossless JPEG XL is written in
     sRGB only.`
503. **DNG.** Choose **DNG**: Data reads **Linear 32-bit float** with the *baked edits* explanation; Colour space is
     dimmed (`DNG is always linear Rec. 2020…`); the Watermark control is dimmed with the warning `Watermarks are not
     available for DNG…`. Summary: `… DNG linear float · Linear Rec. 2020 …`.
504. **JPEG size limit.** Choose **JPEG**: under Quality, tick **Limit file size to** and type `300` **KB**. The summary
     shows `JPEG <q> ≤ 300 KB`. Switch to PNG and back: the limit is off (only JPEG has one), and ticking it again restores 300. Choose a folder `$SCR/m246`
     and export: every `.jpg` is at most 300,000 bytes (`stat -f %z "$SCR"/m246/*.jpg`).
505. **Text watermark.** Watermark ▸ **Text**. 📸 Expect Text (`© `), Font (a pop-up of installed .ttf / .otf fonts,
     Arial by default, and **Other…**), Size %, Colour, Rotation °, then Opacity %, Position (3 × 3 grid, bottom right
     chosen), Inset %, and the **Preview** well: a 3:2 placement preview with the chip `Placement preview`. Type
     `© Tessera`, set Size 10 %, move Position to top left, Rotation −20: the preview text follows each change.
506. **Engine preview.** Click **Render with Engine**: a spinner, then the well shows the first photo rendered by the
     engine at 480 px with the watermark (chip `Rendered by the engine`). Change the opacity: the well returns to the
     placement preview until rendered again.
507. **Graphic watermark.** Watermark ▸ **Graphic**, **Choose…** a PNG (e.g. a logo with transparency), Scale 25 %,
     Position centre. Switch to **Text** and back: both kinds keep their fields. Export PNG to `$SCR/m246-mark`: the
     graphic is burned into the centre of every file, the same size relative to the short edge.
508. **Presets.** Preset menu ▸ **Save as Preset…** `Marked AVIF` (AVIF 10-bit, text watermark). Choose **Web 2048
     sRGB**, then **Marked AVIF**: every field returns. Quit and relaunch: still there. The four shipped presets and any
     preset saved before M2-46 load unchanged (no watermark, no size limit).
509. **Other formats on disk.** Export once each as AVIF, JPEG XL and DNG to `$SCR/m246-fmt`. Expect `.avif`, `.jxl`
     and `.dng` files; Preview.app opens the AVIF and the JPEG XL; the DNG opens in a raw editor such as Lightroom or
     darktable (Apple's Preview cannot decode this linear float DNG).

## Verdict (export formats and watermarks)

PASS when steps 500–509 meet their expectations. Disabled controls with their reasons are expected, not failures.

## Appendix: accessibility identifiers (M2-46)

| Identifier | Element |
| --- | --- |
| `export-format` · `export-quality` · `export-bit-depth` · `export-avif-speed` · `export-jxl-lossless` · `export-dng-note` | File Settings: format bar, Quality, Bit depth, AVIF Speed, JPEG XL "Lossless", DNG explanation |
| `export-color-space` · `export-color-space-note` · `export-hdr` | Colour space pop-up, its "does not apply" hint, the disabled HDR checkbox |
| `export-size-limit` · `export-size-limit-kb` | JPEG "Limit file size to" checkbox and KB field |
| `export-watermark-kind` · `export-watermark-unavailable` | None / Text / Graphic control; the DNG warning |
| `export-watermark-text` · `export-watermark-font` · `export-watermark-size` · `export-watermark-color` · `export-watermark-rotation` | Text watermark fields |
| `export-watermark-graphic` · `export-watermark-choose` · `export-watermark-scale` | Graphic watermark file, Choose…, Scale |
| `export-watermark-opacity` · `export-watermark-anchor` · `export-watermark-anchor-<top_left…bottom_right>` · `export-watermark-inset` | Shared: Opacity, Position grid and its nine cells, Inset |
| `export-watermark-preview` · `export-watermark-render` · `export-watermark-problem` | Preview well, Render with Engine, problem line |

## AB. Photo Merge (HDR, Panorama, HDR Panorama) and Enhance (M2-50)

Continues section O (engine backend, a fresh `--app-dir`, `$SCR/raw3` open in the grid). For the merges you also need
one real bracket and one real panorama from a single camera: `$BRACKETS` (3 exposures of one scene, ±2 EV) and
`$PANO` (3 overlapping frames). Engine limits (preview is a camera-channel approximation, stage-level model download
progress, no Raw Details, no enhance preview call): tools/orchestrate/wp/M2-50/REPORT.md.

540. **Tests.**
     ```sh
     (cd apps/mac && swift test --filter 'PhotoMergeEnhanceTests|ThemeLintTests' 2>&1 | grep Executed)
     ```
     Expect `Executed 12 tests, with 0 failures` and `Executed 1 test, with 0 failures`. The last PhotoMergeEnhanceTests
     case writes three bracketed LinearRaw DNGs, merges them through the app into `bracket-1-HDR.dng`, checks it is
     selected in the grid, stacked with its three sources, and opens in Develop.
541. **Menu and shortcuts.** 📸 The menu bar has **Photo** between Library and Develop: **Photo Merge ▸ HDR… ⌃H ·
     Panorama… ⌃M · HDR Panorama…**, **Enhance… ⌃⌥I**, and a dimmed **Cancel Photo Merge**. With one photo selected:
     HDR and Panorama are dimmed, Enhance is enabled. Select all three (⌘A): HDR and Panorama enable; HDR Panorama stays
     dimmed until four or more are selected. In Layered Documents, the People view or a stub library every item is dimmed.
542. **Warnings.** With the three `raw3` photos (different cameras and scenes) selected, press **⌃M**. 📸 The sheet
     `Panorama Merge Preview` (subtitle `3 photos · …`) shows the preview well on the left (a spinner in the header while
     the engine works) and, under it, the engine's warning that the frames could not be registered (insufficient
     overlap / no geometry) with no image. Options: Projection **Auto · Spherical · Cylindrical · Perspective**,
     Boundary Warp (0–100), Fill Edges, Auto Settings, Create Stack. Choose **Spherical**: a Focal length field appears
     and the footer reads `Spherical needs the focal length in pixels`, Merge dimmed. **Cancel**.
543. **HDR.** Open `$BRACKETS`, select its three photos, **⌃H**. 📸 `HDR Merge Preview`: the merged preview (≤ 512 px,
     chip `Engine preview · W × H`), options Auto Align, Deghost Amount **None · Low · Medium · High** (Medium), Auto
     Settings, Create Stack (both on), and the hint that the preview is an approximation. Change Deghost to High: the
     preview re-renders (dimmed while it works). Select three frames with the *same* exposure instead: a warning
     `Exposures differ by 0.0 EV: HDR adds little range…`.
544. **Merge in the background.** With the bracket, click **Merge**. The sheet closes at once; above the status bar a
     strip `HDR · <name> and 2 more` shows the stage (Reading photos → Merging → Writing DNG) with counts and **Cancel**.
     The grid stays usable. When it ends: the toast `HDR merge: created <first>-HDR.dng` with `Stacked with 3 source
     photos`; the new DNG appears in the grid, selected (also when a filter would have hidden it). Press **E**: it
     opens in Develop and responds to Exposure. The file sits next to the first source; a second merge makes `-HDR-2.dng`.
545. **Cancel.** Start another HDR merge and click **Cancel** in the strip (or Photo ▸ Cancel HDR merge): the strip
     disappears, the toast reads `HDR merge cancelled`, and no new file appears in the folder.
546. **Panorama.** Open `$PANO`, select the frames, **⌃M**: the preview shows the stitched panorama and the hint
     `Projection chosen: …`. Set Boundary Warp 60 and tick Fill Edges: the preview edges change. Merge: `<first>-Pano.dng`
     appears selected. Untick **Create Stack** before a second merge: the toast has no `Stacked with…` line.
547. **HDR Panorama.** Select six frames (two brackets of three, in order), Photo ▸ Photo Merge ▸ **HDR Panorama…**:
     `Frames per bracket` offers **2 · 3** with `2 brackets of 3, in selection order…`; choose 2 with a count that does
     not divide and the footer explains why Merge is dimmed. Merge with 3: `<first>-HDR-Pano.dng`.
548. **Enhance, offline.** Select two `raw3` photos, **⌃⌥I**. 📸 `Enhance`: Denoise (on) with Amount 50, Super
     Resolution (off), a dimmed **Raw Details** with `Not available…`, **Download missing models** (off) with the model
     note, and the Preview note that the engine has no before / after preview call. Click **Enhance** with downloads off
     on a fresh app dir: the strip appears, then the toast `Enhance failed: The enhancement model … is not on this Mac
     and downloads are off. Turn on “Download missing models”…`. Reopen Enhance: the same message shows as `Last run: …`.
     Untick both Denoise and Super Resolution: Enhance dims with `Choose Denoise, Super Resolution or both`.
549. **Enhance with download.** Tick **Download missing models**, Enhance: the strip shows `Downloading denoise model…`
     (an indeterminate bar: the engine reports only start and verified-ready), then Denoising per photo with counts.
     Each photo gets `<name>-Enhanced-NR.dng`, stacked with it, and the results are selected; select one with its source
     and press **C** to compare. Amount 0 writes an unchanged copy without loading a model.

## Verdict (Photo Merge and Enhance)

PASS when steps 540–549 meet their expectations. Engine warnings, dimmed options with their reason and the missing-model
error with downloads off are expected, not failures.

## Appendix: accessibility identifiers (M2-50)

| Identifier | Element |
| --- | --- |
| `photo-merge-sheet` · `photo-merge-preview` · `photo-merge-preview-busy` · `photo-merge-preview-error` · `photo-merge-warnings` | Merge sheet, preview well, header spinner, preview problem, warning list |
| `photo-merge-auto-align` · `photo-merge-deghost` · `photo-merge-auto-tone` · `photo-merge-create-stack` | HDR options, Auto Settings, Create Stack |
| `photo-merge-projection` · `photo-merge-focal` · `photo-merge-boundary-warp` · `photo-merge-fill-edges` · `photo-merge-bracket-size` | Panorama options, HDR Panorama bracket size |
| `photo-merge-problem` · `photo-merge-error` · `photo-merge-output` · `photo-merge-cancel` · `photo-merge-start` | Footer: why Merge is dimmed, start error, output name, Cancel, Merge |
| `enhance-sheet` · `enhance-denoise` · `enhance-denoise-amount` · `enhance-super-resolution` · `enhance-raw-details` · `enhance-allow-download` | Enhance sheet and options |
| `enhance-preview-note` · `enhance-last-error` · `enhance-problem` · `enhance-error` · `enhance-output` · `enhance-cancel` · `enhance-start` | Notes, errors, footer and actions |
| `photo-job-progress` · `photo-job-cancel` | Activity strip for a running merge / enhance and its Cancel |

## AC. Develop: model downloads, Lens Blur depth tools, Guided uncorrected view, export warnings (M2-51)

Continues section X (engine backend, `$SCR/raw`, **sony-arw.ARW** in the loupe). Models land in
`<support>/models/cache` (the cache the loupe and export read). The CFA denoise entry is a local artifact that does not
resolve from the engine's copied catalog; with `TESSERA_MODEL_MANIFEST=<checkout>/crates/ml-runtime/models.toml` and the
artifact present it installs, otherwise its failure reason is the expected result. Known limits:
tools/orchestrate/wp/M2-51/REPORT.md.

520. **Tests.**
     ```sh
     (cd apps/mac && swift test --filter 'ModelAcquisitionTests|LensBlurDepthModelTests|UncorrectedPlacementTests|ExportWarningsTests|LensBlurExportWarningTests|TransformLensBlurTests|ThemeLintTests' 2>&1 | grep Executed)
     ```
     Expect every line `with 0 failures` (download progress states, depth histogram binding, Visualize Depth, Subject,
     Guided enter/exit, export warnings from stubs and one real export).
521. **Settings ▸ AI.** ⌘, ▸ AI. 📸 A **Develop models** section with **Allow model downloads** (on by default) and the
     hint `AI Denoise and Lens Blur fetch their pinned, checksum-verified models the first time you use them…`. Quit and
     relaunch: the setting is kept.
522. **Downloads off.** Untick Allow model downloads, move `<support>/models/cache` aside, then in LENS BLUR tick
     **Apply**: a warning line `Depth model: … (model downloads are off in Settings ▸ AI)` with **Retry**; nothing is
     recorded in History. DETAIL ▸ AI Denoise shows `Model downloads are off (Settings ▸ AI)…` before first use.
523. **Download progress.** Tick Allow model downloads again (failures are forgotten) and click **Retry** (or tick Apply):
     📸 the inline bar goes `Queued…` → `Downloading 12 MB of 99 MB` (determinate) → Lens Blur applies (`Lens Blur On`).
     A second photo reuses the cache: no progress, Apply is immediate.
524. **Depth histogram.** With Lens Blur applied, 📸 the Focal Range strip shows the 256-bin near → far depth histogram
     behind the band; the band's handles still drag (one history step per drag, `Focal Range 20–45`). Switching to
     another photo with Lens Blur recomputes it; a photo without Lens Blur shows the plain ramp.
525. **Visualize Depth.** Tick **Visualize Depth**: the loupe shows the grayscale depth map (near is light) instead of
     the photo; nothing is added to History and the recipe is unchanged. Untick: the photo returns. Switching photos
     turns it off.
526. **Subject.** Click **Subject**: `Finding the subject…` (plus the segmentation models' progress on first use), then
     the focal range band moves around the main subject in one History step `Focal Range: Subject 30–45` (numbers vary);
     ⌘Z restores the previous band. With the segmentation weights missing: a warning line with the reason; the band and
     History are unchanged.
527. **Apertures.** Choose each **Bokeh** entry: History `Bokeh: Cat Eye`, etc.; the loupe's out-of-focus highlights
     change shape (5-Blade pentagons, Ring outlines, Oval stretched, Cat Eye clipped near the corners).
528. **Guided on a corrected photo.** Covered by step 173: with a keystone or crop set, Guided shows the uncorrected full
     frame while armed and restores the corrected view on Done / Esc / switching photo.
529. **Export warnings.** Move the depth weights out of `<support>/models/cache`, keep Lens Blur applied and export PNG
     to `$SCR/m251`. 📸 The completion toast headline ends `; 1 with warnings` and its details list
     `sony-arw.ARW: Lens Blur skipped: Lens Blur depth model is not cached; download depth/anything-v2-small …`; the file is written (unblurred) and
     `<exported file>.tessera-warnings.txt` (written by the engine) sits beside it.

## Verdict (model downloads, depth tools, export warnings)

PASS when steps 520–529 meet their expectations. A model that cannot be acquired on the test Mac is PASS when the panel
shows the inline failure with its reason and nothing is recorded.

## Appendix: accessibility identifiers (M2-51)

| Identifier | Element |
| --- | --- |
| `ai-allow-model-downloads` | Settings ▸ AI ▸ Develop models ▸ Allow model downloads |
| `detail-ai-denoise-model` · `lensblur-model` (and `-retry`) | Inline model progress / failure rows |
| `lensblur-visualize-depth` · `lensblur-subject` · `lensblur-error` · `lensblur-busy` | Lens Blur depth tools |
| `transform-guides-uncorrected` | Guided Upright's uncorrected-view hint |

## AD. Layer styles and Global Light (B5-07)

Setup: an engine build (`Support/make-app.sh`), a scratch folder `$SCR` with an empty `shoot` folder, and a small
document. Styled layers are composited on the CPU (the Metal resident renderer does not draw effects yet), so a
1000 × 700 document redraws in about 0.5–1 s per change; larger documents are slow, and above 16.7 million
pixels (canvas plus effect padding) styled frames fail with "style alpha canvas exceeds CPU pixel limit"
(IMPLEMENTATION-STATUS.md of WP B5-07). Steps 300–312 use File ▸ New Document at 1000 × 700.

300. **Two shapes.** On the first layer, Select ▸ All, Edit ▸ Fill… with a light grey, deselect. Add two pixel
     layers, *Shape A* and *Shape B*, each with a filled rectangular selection (orange), well apart.
301. **Drop Shadow.** Select Shape A, Layer ▸ Layer Style ▸ Drop Shadow…. The Layer Style panel opens (title
     *Layer Style*, subtitle *Shape A*) with Drop Shadow checked and selected; a shadow appears down-right of the
     shape (Global Light 120°). History gains one *Drop Shadow* row. The Layers panel shows an fx glyph on
     Shape A and, under it, a *Drop Shadow* row with an eye.
302. **Live drag, one node.** Drag Distance to about 28 px and Size to about 12 px: the canvas follows the drag;
     each release adds exactly one *Drop Shadow* history row.
303. **Stroke.** Check Stroke in the panel's list: a 3 px black stroke appears outside the edge. Set Size 6 px and
     the colour well to blue. Stroke is listed above Drop Shadow (the engine's stacking order), in the panel and
     under Shape A in the Layers panel.
304. **Repeat an effect.** Click "+" on the Stroke row: a second Stroke appears above the first (both listed);
     the "−"/trash button removes the selected one. Bevel & Emboss, Satin, the glows and Pattern Overlay have no
     "+" beyond one instance of the non-repeatable kinds.
305. **Fill 0 %, effects stay.** Select Blending Options in the panel and drag Fill Opacity to 0 %: the orange
     interior disappears while the stroke and shadow stay (the shadow shows through the empty interior: the
     engine has no "Layer Knocks Out Drop Shadow" yet). The Properties panel lists *Stroke* and *Drop Shadow*
     under Layer Style.
306. **Second shape.** Double-click Shape B's row away from its name: the panel switches to Shape B (Blending
     Options). Check Drop Shadow; set Distance 28 px.
307. **Global Light across two layers.** Layer ▸ Layer Style ▸ Global Light…: the panel says "2 layers use it".
     Drag the dial (or Angle) from 120° to 30°: both shadows swing to the lower left together; one *Global Light*
     history row on release. In the Layer Style panel both shadows show *Angle (global)* 30°.
308. **Local angle.** On Shape B's Drop Shadow, uncheck Use Global Light (the angle stays 30°), then set its Angle
     to 90°: only Shape B's shadow moves. Undo twice to return it to the global light.
309. **Undo / redo.** ⌘Z undoes *Global Light*: both shadows return to 120°. ⇧⌘Z redoes it.
310. **Copy / paste / clear.** With Shape A selected, Layer ▸ Layer Style ▸ Copy Layer Style; select Shape B,
     Paste Layer Style (one *Paste Layer Style* row; Shape B gets Stroke and Drop Shadow); Clear Layer Style
     (one row); undo.
311. **Locked and unstyleable layers.** Lock All on Shape A: the panel shows a *Locked* chip and its controls are
     disabled; Layer ▸ Layer Style's effect items and Clear Layer Style are disabled, and a style edit
     from anywhere else reports that the layer is locked. On an adjustment layer, the effects are disabled with a
     warning line. Unlock.
312. **Save PSD, reopen.** File ▸ Save As… `Styles.psd`, close the document, open `Styles.psd`: Shape A has
     Stroke and Drop Shadow with Fill 0 %, Shape B its Drop Shadow, Global Light 30° (Layer ▸ Layer Style ▸
     Global Light…). Effects PSD cannot store (bevel, satin, gradient / pattern overlays, repeated effects,
     gradient strokes) make Save As PSD fail with a message instead of being dropped; `.tessera-doc` keeps
     everything.
313. **Metadata only.** Open a PSD whose effects have contours: the effect's editor lists *Contour* (and for
     Bevel & Emboss *Texture*) under "Kept, not rendered"; no working controls for them. Saving keeps them.
314. **Scripted run.**
     ```sh
     apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/shoot" --app-dir "$SCR/appdir" \
       --styles-selftest "$SCR/out" 2>&1 | grep styles-selftest
     ```
     Runs 300–312 through the controller (shapes, drop shadow + stroke with a one-node drag, Fill 0 %, a second
     shadow, Global Light 120° → 30°, undo, redo, save `.tessera-doc` and PSD, close, reopen the PSD) and prints
     `step <n> <name> window … panel …` for `screencapture -R`. Expect 15 `check … ok` and `done, 0 failure(s)`.

## Verdict (layer styles)

PASS when steps 300–314 meet their expectations. Frame times on styled documents are recorded, not gated
(the CPU fallback is a stopgap until effects render on the GPU).

## Appendix: accessibility identifiers (B5-07)

| Identifier | Element |
| --- | --- |
| `document.layerStyle` (panel) · `document.layerStyle.list` · `.detail` · `.done` · `.addMenu` · `.delete` · `.globalLight` | Layer Style panel |
| `document.layerStyle.row.blending` · `document.layerStyle.row.<kind>[.<index>]` · `document.layerStyle.enable.<kind>[.<index>]` · `document.layerStyle.add.<kind>.<index>` | Effect list (kind = serde name, e.g. `drop_shadow`) |
| `document.layerStyle.<kind>.<key>` (e.g. `document.layerStyle.drop_shadow.distance`, `.dial` for angles, `.fill.color`) · `document.layerStyle.editor.<kind>` · `document.layerStyle.<kind>.metadata.<key>` | Effect editors |
| `document.layerStyle.opacity` · `.fill` · `.scale` | Blending Options |
| `document.globalLight` (panel) · `document.globalLight.dial` · `.angle` · `.altitude` · `.done` | Global Light panel |
| `document.layers.addStyle` · `document.layers.fx.<layer>` · `document.layers.effect.<layer>.<index>` · `.visibility` · `.name` | Layers panel fx button, glyph and effect rows |
| `document.properties.style.<index>` · `document.properties.style.edit` | Properties summary |

## B5-10. Type tool and text layers

Engine backend (not `--stub-library`), a 20 MP scratch image (`sips -s format png fixtures/raw/sample.dng --out
src.png && sips -z 3648 5472 src.png --out photo20mp.png`, a copy, never the fixture) opened with
`apps/mac/build/Tessera.app --app-dir "$SCR/appdir" --open-document "$SCR/photo20mp.png"`. Inspector shown, the
History expanded; the Properties sub-tab chosen unless a step names another (B5-16). Fonts are the installed system
fonts (Helvetica).

340. **Inspector at 1440 pt.** Window 1440 × 850 pt (also 1280, 1366, 1512): the sidebar's left edge and the
     inspector's right edge are inside the window; Properties (Name field, Character; scrolling the Properties
     sub-tab), Stack (⌃1: Opacity and Fill values, filter field), Channels (⌃3: lock glyphs) and History rows are fully
     visible or reachable by scrolling their own tab or pane.
     No right edge is clipped. The inspector column keeps its 288 pt minimum; the canvas shrinks instead.
341. **Point text.** Press T (or click the palette's Aa): the options bar shows the font, size, alignment and the hint.
     Click the canvas: a caret appears with its baseline at the click; type `Hello World`; Enter (keypad) or ⌘Return
     applies: one History row `Add Text`, a `Hello World` text row (Aa glyph) selected.
342. **Area text.** Drag a rectangle: a dashed box with eight handles; typed text wraps inside its width. The status
     bar hint reads `Area text: …` here and `Point text: …` while editing point text (B5-10c).
343. **Selection and mixed runs.** Click into text (or just right of its last glyph, B5-10c) to resume editing; drag across words selects them (accent
     highlight following the glyphs); typing replaces the selection; ⌫ / ⌦ delete across differently styled runs.
344. **Character.** With a selection, change Font, Style (weight / italic), Size and Color in Properties ▸ Character:
     only the selected characters change; one History row per change (`Font: …`, `Font Style`, `Font Size`,
     `Text Color`); runs outside the selection are unchanged. With a caret only, the change applies to the next
     typed text (Hint says so).
345. **Tracking, leading, baseline.** Drag the Tracking, Leading and Baseline shift sliders: the text updates live;
     each release adds exactly one History row (`Tracking`, `Leading`, `Baseline Shift`); ⌘Z undoes one each.
346. **Paragraph.** Alignment (left / center / right / justify), indents and space before / after update the layout;
     run styles and the point / area geometry are unchanged.
347. **Resize the box.** Drag an area box handle: the text rewraps (overflow when too short), glyphs keep their size
     (no bitmap stretching); release adds `Resize Text Box`. Then type and press ⌘Return: it applies (B5-10c; the
     handle drag no longer leaves the keyboard on the canvas), as do keypad Enter and Esc whichever view has focus.
348. **Caret alignment.** At Fit, 100 % and 200 % with panning, and after ⌘-dragging outside the frame to rotate the
     layer (⌘-drag inside moves it), the caret and selection sit on the rendered glyph edges.
349. **Clusters.** Type `office` (the ffi ligature is one caret stop), `e` + combining acute, `𝐀` (U+1D400) and a
     skin-toned emoji: arrows and ⌫ never stop inside a ligature, a mark or a surrogate pair; ⌫ removes the emoji whole.
350. **Bidi.** Type `abc `, Hebrew `אבג`, ` def`: the Hebrew renders right to left; clicking the right half of a Hebrew
     letter places the caret before it (its visual right edge); replacing it keeps the surrounding source intact.
351. **IME.** With a Japanese (Kana / Romaji) input source: typing `ka` shows underlined marked text `か` on canvas and
     the candidate window below it; Esc cancels (no text, no History row); committing `漢` then Enter adds one History
     row; ⌘Z removes the whole composition.
352. **Keys stay in the text.** While editing, T V X D Q, digits, Space and ⌫ type or delete text: the tool, the
     colours, layer opacity and the layer list do not change. ⌘A selects all text, ⌘C / ⌘V copy and paste text,
     ⌘Z reverts the typing since the last change.
353. **Typing group.** Type a word and apply: one History row; ⌘Z restores the exact previous text and styles,
     ⇧⌘Z the typed version.
354. **Cancel and switch.** Start new text, type, press Esc: the draft layer disappears, no History row. Switch to
     another document tab and back: no caret, no draft, History unchanged.
355. **Locks.** Lock pixels (or all): typing into the layer is rejected with a status message and no change. Lock
     position only: typing works, ⌘-drag moves are rejected.
356. **Convert to Pixels.** Add a mask to a styled text layer, Layer ▸ Convert Text to Pixels (or Properties ▸
     Convert to Pixels): the row becomes a pixel layer with the same name, mask and look; one History row
     `Convert to Pixels`; ⌘Z restores the editable text.
357. **Native reopen.** Save As `Text.tessera-doc`, close, reopen: text rows are editable (click with T, type).
358. **PSD reopen.** Save As `Text.psd`, close, reopen: mixed-style text stays editable with its runs; a layer
     converted to pixels before saving comes back as pixels (no stale type).
359. **Limitations.** A warped layer shows `Warped text: … canvas caret placement is disabled` and a Source text
     field instead of a canvas caret; a layer using a missing family shows `Missing font “…”` (no substitution);
     Display P3 / 32-bit documents show the Colour note (text colours are sRGB bytes, not colour-managed). Vertical
     text and dictionary hyphenation are reported unsupported. Screenshot the inspector at 1440 pt again.

Scripted run (steps 340–359 except a real IME input source, which uses `NSTextInputClient` calls):

```sh
open -n apps/mac/build/Tessera.app --env TESSERA_TEXT_SELFTEST="$SCR/out" --stderr "$SCR/text.log" \
  --args --app-dir "$SCR/appdir" --open-document "$SCR/photo20mp.png"
```

Each shot writes `$SCR/out/<step>.req` (a `screencapture -R` rectangle of the Tessera window) and waits for
`<step>.png` from an external watcher; expect every `check … ok`, `latency …` and `done, 0 failure(s)`.

## Verdict (B5-10 Type tool and text layers)

PASS when steps 340–359 meet their expectations. Known limitations: no canvas caret on warped, path or vertical text
(Source text editor instead); vertical composition and dictionary hyphenation unsupported; no font fallback (missing
fonts are errors); live text colours are sRGB bytes written into the document's samples (no colour management);
arrow keys move logically through bidi text (clicks and carets are visual); paragraph settings apply to the whole
layer; scale / skew of a text layer's affine is not offered on canvas (move and rotate are).

## Appendix: accessibility identifiers (B5-10)

| Identifier | Element |
| --- | --- |
| `document.text.family` / `document.text.style` | Character font family and style pop-ups |
| `document.text.size`, `.leading`, `.tracking`, `.baselineShift` | Character sliders |
| `document.text.color`, `document.text.kerning` | Colour well, kerning checkbox |
| `document.text.alignment` | Paragraph alignment segments |
| `document.text.leftIndent`, `.rightIndent`, `.firstLineIndent`, `.spaceBefore`, `.spaceAfter` | Paragraph sliders |
| `document.text.toggleBox` | Convert to Point / Paragraph Text |
| `document.text.source` | Source text field (warped / path / vertical text) |
| `document.text.limitation` | A limitation note |
| `document.text.apply`, `document.text.optionsApply` | Apply (inspector, options bar) |
| `document.text.convert` | Convert to Pixels |
| `document.option.textSize`, `document.text.latency` | Options bar size field, typing latency readout |
## B5-11. Shapes, Pen and vector masks (B5-11)

Engine backend, this worktree's app (`apps/mac/build/Tessera.app`, `Support/make-app.sh debug`). New document
**5472 × 3648 px, 8-bit, sRGB** (File ▸ New; 20 MP). Only scratch folders. The scripted run of every step is
`tools/orchestrate/wp/B5-11/run-vector-selftest.sh` (launches with `open -g -n … --new-document
--vector-selftest=<dir>`, never activates the app, captures its own window with `screencapture -l`); its log and 27
captures are in `tools/orchestrate/wp/B5-11/evidence/`. Known gaps: tools/orchestrate/wp/B5-11/NEEDS.md.

360. **Tools.** The palette has three new slots: Pen (P), Path Selection / Direct Selection (A, ⇧A cycles) and
     Rectangle / Ellipse / Polygon / Line (U, ⇧U cycles; right-click lists the group). Press **U** and drag on the canvas.
     📸 A row with the `square.on.circle` kind glyph named `Rectangle 1` appears; Properties ▸ Kind reads **Shape**
     (never Fill); History reads **Rectangle Tool**.
361. **Rounded rectangle.** Options bar ▸ Radius `40`, drag another rectangle: all four corners rounded. In Properties
     untick **Same radius for all corners** and drag Top right / Bottom right / Bottom left independently: the corners
     follow live during the drag; each release is one **Edit Shape** node.
362. **Ellipse.** ⇧U to Ellipse; drag with ⇧⌥ from a point: a circle centred on the press point. Without modifiers the
     box corner follows the pointer.
363. **Polygon / star.** ⇧U to Polygon; options Sides `6`, tick Star, Inset `50 %`; drag from the centre (⇧ snaps the
     angle to 15°): a 12-point star. In Properties drag Sides to 8 and Star inset: the geometry regenerates live.
364. **Line.** ⇧U to Line, Weight `14`, drag (⇧ snaps to 45°): a stroke-only line (Properties ▸ Fill **None**). With
     Path Selection (A) click just beside the line (within the stroke): the line is selected.
365. **Paint.** Rectangle ▸ Properties ▸ Fill ▸ **Gradient**; change Style, Start / End colours, Angle: the canvas
     follows. Move the shape with Path Selection: the gradient stays anchored to the document (the shape moves across
     it; Hint in Properties says so). Fill ▸ Solid and a colour: the colour applies.
366. **Stroke.** Stroke ▸ Solid, Width `18`, Align **Inside**, Caps **Round**, Corners **Bevel**, Dashes `60, 30`, Dash
     offset `12`: each visible on the canvas; the values survive save / reopen (step 378).
367. **Pen.** P, click three points, drag on the second to pull symmetric handles (⌥ breaks them), click the first
     point: the path closes and one **Pen** node adds a custom shape. Return finishes an open path; Esc discards the
     draft; ⌫ removes the last point.
368. **Direct Selection.** ⇧A to Direct Selection (or A twice). Click an anchor (filled square = selected), drag it: one
     **Move Anchor Point** node. Drag a direction point: the opposite handle mirrors; with ⌥ it stays (one **Move
     Direction Point** node each).
369. **Insert / delete.** ⌥-click a segment: **Add Anchor Point**; ⌫ deletes the selected anchor (**Delete Anchor
     Point**); with the Pen, clicking a selected shape's anchor deletes it and a segment adds one. Drag a corner of a
     live rectangle: Properties changes to **Custom Path** and later paint edits no longer regenerate the rectangle.
370. **Path operations.** Draw two overlapping rectangles, select both rows, Layer ▸ Combine Shapes ▸ **Combine /
     Subtract Front Shape / Intersect Shape Areas / Exclude Overlapping Shapes** (also Path Selection's Combine menu):
     the front shape merges into the back one with correct holes; each is **one** history row; ⌘Z restores both layers.
371. **Hits at any view.** At Fit, 100 % (⌘1) and after panning, Path Selection clicks select the shape under the
     pointer, including thin stroke-only lines; the overlay path, anchors and box stay on the geometry. Give a shape a
     skewed transform (Path Selection box: drag a side handle, then rotate outside the box): clicks inside the skewed
     shape still hit it.
372. **Affine handles.** Path Selection: drag a corner handle (⇧ keeps proportions, ⌥ from the centre), inside to move,
     outside to rotate: one **Transform Shape** node per drag. Start another drag and press Esc before releasing: the
     shape returns and History is unchanged.
373. **Vector mask next to a layer mask.** Layer ▸ Layer Mask ▸ Reveal All, then make a marquee and Layer ▸ Vector Mask
     ▸ Current Selection (or Properties ▸ Add Vector Mask / From Selection): the raster mask thumbnail stays; the vector
     mask clips outside the marquee; Properties shows the Vector Mask section.
374. **Mask controls.** Untick Enabled (whole shape shows), tick it again; drag Density 100 → 50 % (outside shows at half
     opacity; one **Vector Mask Density** node on release), Feather 40 px (soft edge; one **Vector Mask Feather**
     node). ⌘Z steps back through each.
375. **Fixed versus linked mask.** Path Selection, drag the shape: the mask stays in place (the shape slides under it).
     Tick **Move vector mask with shape** (options bar or Properties) and drag again: shape and mask move together as
     one **Transform Shape and Vector Mask** row; one ⌘Z restores both.
376. **Errors.** Lock pixels on the shape and change its colour: the status bar says the content is locked and nothing
     changes. Lock position only: moving fails (`position is locked`), recolouring works. Select the line and choose
     Align ▸ Outside: `Inside and Outside alignment need a closed path; …`.
377. **Convert.** Layer ▸ Rasterize Shape (or Properties ▸ Convert to Pixels) on the masked shape: the row becomes a
     pixel layer with both masks still applied once (appearance unchanged), history row **Convert to Pixels** (B5-10's
     shared conversion); ⌘Z restores the live shape exactly.
378. **Reopen.** Save As `.tessera-doc` and `.psd` into a scratch folder, close, reopen each: every shape is a Shape row
     with its live controls (rectangle radii, star, line, custom paths, dashes) and its vector mask. Properties ▸
     Interchange states that in a PSD the extra vector mask is a raster user mask plus Tessera's private tvMk record
     (other apps see the combined raster mask). A shape with an imported pattern fill shows the warning that PSD save
     does not support pattern shape fills, and Save As `.psd` fails with that reason (native save works).
379. **Inspector and neighbours.** At 1440 pt window width 📸 the shape Properties (Shape, Fill, Stroke, Vector Mask,
     Interchange) scroll inside the Properties sub-tab; the Remove tool (⇧J) still activates and deactivates. Add B5-07
     layer styles (Drop Shadow, Stroke) to a shape: two history rows, the row shows FX and its effects, the layer stays
     a live Shape; Convert to Pixels keeps the styles and ⌘Z restores the styled live shape.

**B5-11b re-check** (fixes from the on-screen verification; scripted in the `11b-*` checks of
`tools/orchestrate/wp/B5-11b/run-vector-selftest.sh`, evidence in `tools/orchestrate/wp/B5-11b/evidence/`):
- **A** twice (or ⇧A) cycles Path ↔ Direct Selection (368).
- Stroke ▸ Solid on a shape without a stroke takes the foreground colour, or black / white against the fill when the
  foreground equals the fill; dashes are visible at once (366).
- Tab to the Dash offset slider and press → several times, Return, → again, click elsewhere: **one** Edit Shape row
  (0.6 s after the last key). Same for every shape and vector-mask slider.
- Pen: after clicking the next point the previous anchor's direction handles stay visible (367).
- A layer with both masks shows the raster mask thumbnail and a separate vector-mask thumbnail (373).
- Pixel-locked shape: a rejected recolour puts the colour well back to the shape's colour (376).
- With a Properties slider or the Layers list focused, U, ⇧U, Z, A … still choose tools; text fields keep letters.
- Path Selection: a click on empty canvas deselects the path (no box, no outline); a drag outside still rotates.
- The status bar shows the current tool's hint on every tool change and returns to the Pen's idle hint after Return
  (no stale "Pen path discarded" or Remove hint).
- Properties ▸ Bounds / Position / Size report the shape's own bounds, following Path Selection drags live.
- Type tool: new area text resized before the first apply records one **Add Text**; the idle Type hint returns after
  applying; an auto-named text layer's name follows its first line until renamed (the rename undoes with the edit).

## Verdict (B5-11 shapes, Pen and vector masks)

PASS when steps 360–379 meet their expectations. Known engine limitations listed in NEEDS.md (PSD reopen of a shape
with both a full-canvas raster mask and a vector mask on large documents; slow previews of stroked shapes) are
recorded, not failures of the host.

## Appendix: accessibility identifiers (B5-11)

| Identifier | Element |
| --- | --- |
| `document.tool.rectangleShape` · `ellipseShape` · `polygonShape` · `lineShape` · `pen` · `pathSelect` · `directSelect` | Palette slots (the slot shows the group's current tool) |
| `document.shape.inspector` · `document.shape.convert` | Shape Properties section, Convert to Pixels |
| `document.shape.rect.width` · `rect.height` · `rect.linkRadii` · `rect.radius` · `rect.radius0…3` | Rectangle controls |
| `document.shape.ellipse.width` · `ellipse.height` · `polygon.sides` · `polygon.radius` · `polygon.rotation` · `polygon.star` · `polygon.inset` · `line.length` · `line.angle` · `fillRule` | Other live parameters |
| `document.shape.fill.kind` · `fill.color` · `fill.gradientKind` · `fill.start` · `fill.end` · `fill.angle` | Fill |
| `document.shape.stroke.kind` · `stroke.color` · `stroke.width` · `stroke.alignment` · `stroke.cap` · `stroke.join` · `stroke.miter` · `stroke.dashes` · `stroke.dashOffset` | Stroke |
| `document.shape.mask.add` · `mask.enabled` · `mask.density` · `mask.feather` · `mask.linked` · `mask.delete` | Vector mask |
| `document.option.fillColor` · `document.option.strokeColor` · `document.option.width` · `document.option.radius` · `document.option.sides` · `document.option.weight` | Options bar |

## B5-12. Warp, perspective, puppet and content-aware scale

Engine backend, a scratch image (never a fixture), e.g. a 1600 × 1000 PNG and a 5472 × 3648 copy for step 399. The
self-test `Tessera.app --args --transform-selftest=<dir>` (launched with `open -g -n`) writes its own test cards, runs
every step below through `DocumentTransforms` and synthesized viewport events and prints `check <step> ok|FAIL`.

380. **Menus.** Edit ▸ Transform lists Content-Aware Scale (⌥⇧⌘C), Puppet Warp, Perspective Warp and Warp under the
     rotate / flip items. They are enabled for pixel, text, shape, fill, group and smart object layers, disabled for
     adjustment layers.
381. **Consent.** On a pixel layer choose Warp, pick Arc in the preset pop-up: the canvas shows the warp at once.
     Esc: the layer is a Pixel layer again and History is unchanged. Warp again, Arc, Return: an alert "Convert to
     Smart Object?" appears; Cancel keeps the session; Convert and Apply records one "Warp" node, the row becomes a
     smart object (same name, opacity, mask, style) with a "Warp" smart filter row. Undo restores the pixel layer.
382. **Bézier net.** Double-click the Warp row: the net re-opens with its handles. Drag an anchor: its tangent
     handles follow; drag a tangent: only that curve bends. The image follows the drag (reduced resolution while
     dragging a large layer); the options bar shows the preview latency.
383. **Splits.** Choose the crosswise split segment and click inside the net: a row and a column of patches appear
     through the click and the image does not move.
384. **Presets.** Each preset with Bend 0 % is the flat net; Arc at 40 % arcs the layer; negative bend reverses it.
385. **Linked planes.** Perspective Warp: Layout mode shows one plane over the layer; Split Vertically makes two
     planes sharing an edge. Warp mode: dragging the shared top vertex moves both planes with no crack.
386. **Rejected geometry.** Drag a corner across its plane: the drag stops at the last convex shape, the status bar
     says the planes must stay convex, and the previous preview stays.
387. **Pins.** Puppet Warp: the mesh covers the layer's opaque pixels. Click three places: three pins; drag one: the
     mesh bends around the others; ⌥-drag beside a pin: a ring with an angle tick rotates the mesh around it;
     ⌥-click a pin removes it; select a pin and type an angle in Rotate.
388. **Options.** Mode Rigid, Density Sparse / Normal / Dense and Expansion 0…64 px re-mesh with the pins kept. A
     large opaque layer shows a warning glyph: the mesh was built from a coarser level to stay within 16,384 vertices.
     Expansion above 64 px is refused (engine error, not a clamp).
389. **Content-Aware Scale.** Drag the right handle to 70 %: W / H fields and the chip show pixels and percent.
     Amount 0 % is a plain resize, 100 % seam carving (visibly different). The output is anchored at the top left
     inside the fixed canvas.
390. **Protection.** Select ▸ Save Selection over an area, then choose it in Protect: that area keeps its shape.
     The pop-up lists saved alpha channels only; there is no skin-tone option.
391. **Zoom / pan.** At 200 % with the view panned, handles sit on the net and a 40-pixel drag moves the point 40
     child pixels.
392. **History.** Apply is one node labelled after the operation; undo / redo step exactly once.
393. **Cancel / switch.** With a preview showing, switch to another document (or open one): the session ends with
     no node in either document; nothing lands later.
394. **Re-edit.** A smart object with Gaussian Blur, Warp and Add Noise: re-editing Warp keeps the order, the other
     filters and Warp's enabled state and blending.
395. **Locks.** Lock Position (or All): Warp refuses with "locked"; colour filters still follow their existing rules.
396. **Live text.** On a text layer, Warp previews; Esc leaves the text layer editable; Apply asks to convert and the
     text stays live inside the smart object.
397. **Native reopen.** Save as `.tessera-doc`, reopen: the stages, masks and source are intact and re-editable.
398. **PSD.** Save As `.psd` fails with "native-only; rasterize explicitly for PSD". File ▸ Save Rasterized PSD Copy…
     writes a PSD with those smart objects rasterized; the open document is unchanged.
399. **20 MP and evidence.** On the 5472 × 3648 copy converted to a smart object, drag a warp anchor for a few
     seconds: frames keep up (draft preview, latency in the options bar); Apply renders the exact result. Screenshots
     at 1440 pt; B5-07 styles and B5-09 Remove still work (their self-tests).
## B5-16. Document inspector layout

Engine or stub backend, one document open (`--new-document`; the stub adds sample layers), inspector shown. Run each
step at 960 × 600, 1280 × 800, 1440 × 900 and 1728 × 1117 (window content size) unless it names one; check light and
dark once. Scripted in `ShellLayoutTests` (`testDocumentInspectorEveryTabAndHistoryStateAtEverySize`,
`testManyDocumentTabsStayCapped`, `testInspectorTabShortcuts`) and the document self-test's `B5-16 ⌃n` checks; evidence
in `tools/orchestrate/wp/B5-16/evidence/`.

420. **Tabs.** 📸 The inspector's top row is a segmented control **Stack · Properties · Channels** (`document.inspector.tabs`),
     neutral (the chosen segment raised, no accent), below the toolbar at every size. Nothing else sits above it.
421. **Stack.** Stack shows the blend mode pop-up, Opacity and Fill (each on its own row at the default 288–296 pt
     inspector; side by side when the inspector is wide enough for a 300 pt interior), Lock with four icons and the
     dimmed Filter field, the outline and the footer (add, mask, adjustment, FX; group, delete). The footer is whole
     at 960 × 600 with History open.
422. **Outline scrolls.** Add 12 adjustment layers: the outline scrolls; the controls above and the footer stay put.
423. **Properties.** Click **Properties** (or ⌃2): the selected layer's Name, Kind, Bounds and editor, then Color (and
     Brushes with a painting tool) scroll inside the tab. A Curves layer's editor and Reset are reachable at 960 × 600.
424. **Channels.** Click **Channels** (or ⌃3): RGB, Red, Green, Blue (and saved channels) fill the tab; the footer
     (load, save, Quick Mask, new, delete) is whole at the tab's bottom.
425. **Shortcuts.** ⌃1 / ⌃2 / ⌃3 switch the tabs from the canvas, the Layers list or a Properties slider; the chosen
     tab is remembered after a relaunch. Letters still choose tools; ⌘ shortcuts are unchanged.
426. **History open.** History sits under every tab: its 32 pt header (`document.history.toggle`), the states and
     snapshots in one scroller, New Snapshot… and the memory line (`12 states · 48 KB`) at its bottom, whole.
427. **History resize.** Drag the hairline above the History header (row-resize pointer) up and down: History grows
     until the tab content reaches its minimum (Stack still shows its controls, four outline rows and its footer) and
     shrinks to about two rows plus New Snapshot. Double-click the hairline: the default height returns. The height
     is remembered.
428. **History collapsed.** Click the History header: the pane closes to its header at the column's bottom and the tab
     content takes the height; click again to open it.
429. **Many snapshots.** Make six snapshots: they scroll with the states; New Snapshot… stays visible; Restore works.
430. **No clipping.** 📸 At 960 × 600 with each tab and History open and closed, nothing in the inspector is cut off at
     its bottom edge, no header draws over another (the old "Color / Layers" and "FX / Channels" overlaps are gone),
     and no control is under the toolbar.
431. **Status bar.** Narrow the window to 960 pt with a selection and Debug ▸ Show Render Timing on: the status bar
     switches to its compact row (canvas size, zoom, tool name, selection size, message, `n open`); the full strings
     are in the help tags. It never widens the window.
432. **Tabs cap.** Open eight documents. At 1280 pt and wider the toolbar shows three tabs (always the current one)
     and a `+5` pull-down listing the others (choosing one makes it current and a tab); below 1280 pt one tab and `+7`.
     The strip never drops into the toolbar's overflow chevron.
433. **Close slot.** Hover an unselected tab: its close glyph appears in place; the title does not move.
434. **Zoom chip.** Zoom (⌘=, ⌘−): the percentage chip appears at the bottom of the canvas for about a second; the
     canvas and the tools palette do not move when it appears or fades.
435. **Tools palette.** At 960 × 600 with a progress strip showing (e.g. an export running) the palette's top tool
     (Move) is below the toolbar and the swatches are reachable by scrolling the palette (no indicator shown).
436. **Filter sheet.** Filter ▸ Blur ▸ Gaussian Blur…: header and Preview / Reset / Cancel / OK stay fixed; the sheet
     is at least its former 560 × 300 and the body scrolls if its controls or an error outgrow it. The same holds
     for Neural Filters (the list and the detail scroll separately), Image ▸ Adjustments, Blending Options, Select
     and Mask, Color Range, Modify Selection, Fill, the Channels sheets and New / Export Flat / Save As.
437. **Accessibility.** VoiceOver reads the tab control as "Inspector" with three buttons, the History header as
     "History, expanded/collapsed", and the existing `document.*` identifiers (appendix B5-02) are unchanged.
438. **Light appearance.** Repeat 420 and 430 in light: same structure, no new colours.
439. **Self-tests.** `--document-selftest`, `--tools-selftest`, `--filter-selftest`, `--retouch-selftest=`,
     `--styles-selftest`, `--vector-selftest=`, `TESSERA_CHANNELS_SELFTEST` and `TESSERA_TEXT_SELFTEST` each end with
     `done, 0 failure(s)` when launched in the background (`open -g -n … --nonactivating`).

## Verdict (B5-16 document inspector layout)

PASS when steps 420–439 meet their expectations at the four sizes.



## Workspace redesign: Library / Photo Edit

These are acceptance instructions, not evidence that the scenario has run.

1. Open a RAW folder, choose an album/filter, select three photos, focus the middle
   one, and scroll the grid away from its first row. The header distinguishes the
   focused filename from “3 selected · decisions apply to 3 photos.”
2. Choose Edit photo (D). The filename stays correct and scope reads “Editing 1
   photo.” Develop/Masks occupy fixed inspector tabs; Library selection/metadata
   panels are absent. Change exposure, visit Masks, then Develop. Only this photo
   changes; switching tabs creates no document or history entry.
3. Choose a filmstrip neighbor. Its filename and controls agree throughout loading.
   Back to Library restores the original three-photo selection, focus, filter,
   arrangement and scroll anchor. G from Edit explicitly returns to Grid.
4. Repeat from Compare; Back restores the pair and active candidate. Escape in
   Compare retains its existing parent arrangement. Insert/remove an image while
   editing; return follows stable surviving photo identity, with a visible nearest
   neighbor fallback if the original photo disappeared.
5. Type X/Y/D in a search/text field and use arrows in a numeric field. Typing never
   decides a photo. In Photo Edit, X/P/Y without an active tool do not cull hidden
   selection; Undo without a photo edit to undo does not consume cull history.
6. With Crop or a mask tool active, Escape cancels/leaves that tool before a later
   Escape returns to Library. Existing Document tool keys still work independently.
7. Invoke Open in Layers (Cmd-E outside Document). Read
   the named source and baked-adjustment explanation. Cancel creates no document.
   Confirm reaches the existing layered-copy flow; Document Save/dirty state,
   tabs, tools and Cmd-E Merge Down retain their existing behavior.
8. Check 960×600, 1280×800, 1440×900 and 1728×1117 in dark/light. Header/back/target
   remain accessible, long names truncate with full help, no toolbar or inspector
   control overlaps, and the native canvas retains the shell's containment rules.

New identifiers: `workspace-back-to-library`, `workspace-photo-target`,
`workspace-command-scope`, `workspace-edit-photo`, `photo-edit-inspector-tabs`,
`photo-edit-target`, `workspace-create-layered-copy`.

The first Layers open renders current adjustments; an already-open copy reopens unchanged. Change the photo recipe after creating a copy, invoke Open in Layers again, and confirm the disclosure promises no refresh and the existing document/pixels/layer edits are retained.

## Workspace redesign: navigable Review (UX-02a)

These replace the earlier modal Agent Review expectations for this slice. They
are acceptance instructions; executed evidence is recorded separately under
`tools/orchestrate/wp/UX-02a/`. Relaunch persistence remains UX-02b.

1. Run Auto Edit on photos outside a current Library filter. Completion leaves the
   current workspace intact; choose Review explicitly. Empty and all-reviewed
   queues are also reachable from the workspace control, Review count, and menu.
2. Browse the least-confident queue. The named target, Current preview and inspector
   agree. Critic outcome and Your review status are separate. Long filenames remain
   identifiable; failed and foreign-library entries retain their actual reason.
3. From a filtered, scrolled, multi-selected Library, enter Review and Edit photo on
   a queued image outside that filter. The actual Develop session must match the
   queued stable image ID. Return to Review, retaining selected row/list position
   and draft, then Library, retaining its original filter, selection and anchor.
   Resize and insert/remove catalog items while away; restoration uses stable IDs.
4. Accept & next moves only after a successful current-target acceptance, skipping
   failed/reviewed rows according to the existing queue rule. Revert stays on the
   row. Redo's merge may reorder rows but keeps the selected image. Errors, foreign
   owners, changed generation and late completions never advance another selection.
5. Use cull keys and Undo in Review: Library decisions/history stay unchanged. Type
   in the Redo field and use numeric controls: those controls retain input. Escape
   cancels a draft first, then returns. Document shortcuts retain precedence.
6. Review at 960×600, 1280×800, 1440×900 and 1728×1117 in both appearances. Back,
   target, list and actions remain reachable; test empty, populated, failed, busy
   and long-name content without foreground activation.

New identifiers include `agent-review-list`, `agent-review-inspector`,
`review-current-preview`, `review-user-status`, `review-critic-status`,
`review-edit-photo`, `review-accept-next`, `review-revert` and
`agent-review-instruction`. The legacy sheet is not presented by the shell.
