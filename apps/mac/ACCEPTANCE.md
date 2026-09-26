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
