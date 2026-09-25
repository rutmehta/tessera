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
   Expect: `swift test` reports `Executed 25 tests, with 0 failures` (XCTest, all suites) and the Swift Testing line
   `Test run with 5 tests in 2 suites passed`; the last line reads `Built …/apps/mac/build/Tessera.app`.
   Also run `cargo test -p tessera-ffi -p cull -p image-core -p library --release 2>&1 | grep "test result"`. Expect only `ok.` lines.
2. Create scratch data (a fresh folder each run; do not reuse an old path):
   ```sh
   SCR="$(mktemp -d)"
   swift apps/mac/Support/make-sample-folder.swift "$SCR/shoot" 40
   cp -RL fixtures/raw "$SCR/raw"
   ```
   Expect: `Wrote 40 JPEGs in 16 bursts to …/shoot`, and `ls "$SCR/raw"` lists 5 RAW files.
   If `fixtures/raw` is missing, skip step 32 and note it in the verdict.
3. Reset app preferences: `defaults delete dev.tessera.app 2>/dev/null; true`.
4. Launch with synthetic AI scores (hidden test flag):
   `open -n apps/mac/build/Tessera.app --args --folder "$SCR/shoot" --seed-scores` 📸
   Expect:
   - a grid of 40 cells with coloured block-mosaic images; the window subtitle reads `40 images`
   - the status bar message starts `Opened shoot: 40 images (0 RAW), 16 groups (12 with 2+)` and ends with
     `synthetic scores seeded`
   - captions end with group labels: cells 1–2 read `G1 · 1/2`, `G1 · 2/2`; cell 3 `G2`; cell 4 `G3`;
     cells 5–8 `G4 · 1/4` … `G4 · 4/4`
   - cell 2 (SAMPLE_0002) shows the outlined **SUGGESTED** pill; cells 3 and 4 (single-frame groups) do not
   - the sidebar ALBUMS section lists **Selects** with a small blue `B` tag and count 0
   - the status bar right side reads `Keep 0  Reject 0   Basket → Selects 0   Auto-advance on`.

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

12. Press **⇧⌘D** (or Cull ▸ Defect Sweep…). 📸 Expect a sheet titled **Defect Sweep**:
    - three threshold rows, all checked: `Missed focus below 0.40`, `Closed eyes above 0.80`, `Blown highlights above 0.05`
    - the counter reads `16 candidates · 16 selected`
    - the first rows are SAMPLE_0002 `Missed focus 0.25 < 0.40`, SAMPLE_0003 `Closed eyes 0.92 > 0.80`,
      SAMPLE_0006 `Missed focus 0.25 < 0.40`, each with a checkbox and a thumbnail
    - no grid cell has changed yet (the sweep is review-only).
13. Uncheck **Closed eyes**. Expect: `10 candidates`. Check it again: `16 candidates`.
    Drag the Missed-focus slider to 0.20. Expect: the focus rows disappear (`8 candidates`, all closed eyes); drag it back to about 0.40 (`16 candidates`).
14. Uncheck the checkbox of the first row (SAMPLE_0002). Expect: `16 candidates · 15 selected` and the default button
    reads **Reject 15 Frames**. Click it.
    Expect: the sheet closes, toast `Rejected 15 frames from the defect sweep`, status `Reject 15`; SAMPLE_0002 is not rejected.
15. Press **⌘Z**. Expect: `Undo: 15 images`, `Reject 0`.

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

29. Quit with **⌘Q** and relaunch without the flag: `open -n apps/mac/build/Tessera.app --args --folder "$SCR/shoot"`.
    Expect: the decisions from steps 18–20 are still shown (cells 5 and 7 REJECT, cell 6 KEEP) and
    `Basket → Selects 1`. Press **⌘Z**: message `Nothing to undo` (undo history belongs to one session).
30. In the terminal: `ls "$SCR/shoot/.edits" | head -3; cat "$SCR/shoot/library.json"`.
    Expect: per-image JSON recipes, and `library.json` with albums `Portfolio` (2 image ids) and `Selects` (1).

## I. RAW fixtures copy and stub performance

31. Quit. `open -n apps/mac/build/Tessera.app --args --folder "$SCR/raw"`. 📸 Expect `Opened raw: 5 images (5 RAW), 5 groups`,
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
    `open -n apps/mac/build/Tessera.app --args --folder "$SCR/raw"`. Click the **nikon-nef.NEF** cell and press **Return**. 📸
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
40. Quit with **⌘Q** and relaunch: `open -n apps/mac/build/Tessera.app --args --folder "$SCR/raw"`. Click the NEF and press
    **Return**. 📸 Expect: the edit is shown (bright, warm), Exposure reads +1.00, and **⌘Z** steps back through the saved
    history (message `Undo: …`).
41. Optional automated drag: quit, then run
    `apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/raw" --keys "return" --develop-selftest 2>&1 | grep -m1 develop-selftest`
    and quit the app once the line appears. It drags Exposure 0 → +1.5 on the first cell through the slider path. Expect
    `develop-selftest: <n> tone frames at L2, render median <m> ms, p90 <p> ms` with p90 below 16 ms (reference: median
    6.2 ms, p90 7.9 ms for the 16 MP RAF). This leaves an `Exposure +1.50` edit on that image.

## K. Library: albums, groups, smart albums, filter bar, keywords, metadata

Albums, album groups and smart albums live in `<folder>/library.json`; keywords and IPTC fields are written to each
photo's XMP sidecar. Nothing in this section deletes or moves a photo. Use a **fresh** sample folder (it has two
simulated cameras, `Sim A` on 30 frames and `Sim B` on 10):

```sh
swift apps/mac/Support/make-sample-folder.swift "$SCR/lib" 40
open -n apps/mac/build/Tessera.app --args --folder "$SCR/lib"
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
43. Relaunch on the RAW copies (`open -n apps/mac/build/Tessera.app --args --folder "$SCR/raw"`), select
    **sony-arw.ARW** and press **Return**. Open **TONE CURVE**. 📸 Expect the parametric curve over the luminance
    histogram, three split triangles under it and Highlights/Lights/Darks/Shadows sliders. Drag inside the curve's
    upper-middle area upwards: the Lights region highlights, the curve bows up, the **Lights** slider follows and the
    loupe brightens the upper mid-tones. Release: HISTORY (below) lists `Curve Lights +…`.
44. Click **Point**, choose **R**. Click the middle of the curve to add a point, drag it up. Expect the red curve bends
    (never below the previous point, never above the next: the editor keeps the curve monotone) and the image turns
    redder. With the point selected press **↑** three times: it nudges up; the burst becomes one history step
    `Point Curve (Red)` after a pause. Double-click the point: it is removed. **Curve Presets ▸ Strong Contrast** on
    **RGB**: an S-curve and a punchier image.
45. Open **HSL / COLOR** ▸ **Saturation**. Drag **Blue** to −100: the sky greys. Click the target button (◎) and
    drag **up** on the cube's orange face in the loupe (cursor ↕): the Orange (and a little Red/Yellow) saturation
    sliders rise together, and the status/History read `Orange Saturation +…`. Press **Esc** to disarm. There is no
    B&W mix (the recipe schema has no field for it yet).
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
    `Preset: Look`). The file exists: `ls ~/Library/Application\ Support/Tessera/Presets/Look.json`.
51. **SNAPSHOTS ▸ New Snapshot…** `Graded`, Save; the list shows `Graded`. Change anything, click `Graded`: back.
52. **HISTORY**: newest first, the current step highlighted, `Original` at the bottom. Click an older step: the image
    and all sliders go back to it; later steps stay listed (dimmed) until a new edit. Click the newest step again.
    Untick the checkbox of the `Blue Saturation −100` step: the sky's colour returns and a step
    `Turn Off Blue Saturation −100` appears (itself not toggleable); tick it again to turn it back on.
53. Quit and relaunch; press Return on the ARW. Expect the crop, curve, HSL, grading, detail and effects restored;
    the grid thumbnail shows the cropped edit.
54. Optional automated pass: `apps/mac/build/Tessera.app/Contents/MacOS/Tessera --folder "$SCR/raw" --develop-panels-selftest 2>&1 | grep -m1 develop-panels-selftest`.
    It opens the first photo in the loupe and drags one control of each panel through the slider path; expect a line
    `develop-panels-selftest: tone curve … L… median … ms; hsl …; grading …; detail …; vignette …; grain …`.

## Verdict

PASS when steps 1–40 and 42–53 meet their expectations (step 32's first part may be skipped only if fixtures are missing, step 31;
steps 33–41 need the RAW fixtures).
Report the command outputs from steps 1–2 and 33, the 📸 screenshots, the benchmark line and any readout values. Afterwards you may delete
`$SCR` and reset preferences with `defaults delete dev.tessera.app`.
