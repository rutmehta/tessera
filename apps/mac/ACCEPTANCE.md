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
   Expect: `swift test` reports `Executed 14 tests, with 0 failures` (XCTest, all suites) and the Swift Testing line
   `Test run with 5 tests in 2 suites passed`; the last line reads `Built …/apps/mac/build/Tessera.app`.
   Also run `cargo test -p tessera-ffi -p cull -p image-core --release 2>&1 | grep "test result"`. Expect only `ok.` lines.
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
    Texture…Saturation are dimmed (hover: `… is not in the M1 pipeline yet`); the status bar shows `render: L…, … ms`.
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

## Verdict

PASS when steps 1–40 meet their expectations (step 32's first part may be skipped only if fixtures are missing, step 31;
steps 33–41 need the RAW fixtures).
Report the command outputs from steps 1–2 and 33, the 📸 screenshots, the benchmark line and any readout values. Afterwards you may delete
`$SCR` and reset preferences with `defaults delete dev.tessera.app`.
