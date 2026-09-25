# M0-04 acceptance script: macOS app shell

For a computer-use verifier. Run every command from the **repository root**. The path contains a colon
and a space, so always quote it. Take a screenshot at each step marked 📸.
Pass criteria: every "Expect" holds. Record any deviation together with its screenshot.

Notes:
- The app captures single-key culling shortcuts globally. Before pressing keys, click once on a grid
  thumbnail so the PhotoEditor window is the key window.
- "Cell N" means the N-th thumbnail in reading order (left to right, top to bottom).
- The status bar is the thin row directly above the filmstrip. Its left side reads
  `<pos> of <total>   G<group> · <frame>/<size>   <state>`. Its right side reads
  `Keep k  Reject r  Basket b   Auto-advance on|off`.

## A. Build and launch

1. Build the app bundle:
   `cd apps/mac && Support/make-app.sh release && cd ../..`
   Expect: the last line reads `Built …/apps/mac/build/PhotoEditor.app`, and no line contains `error:`.
   Also run `cd apps/mac && xcodebuild -scheme PhotoEditor -configuration Debug -destination 'platform=macOS' build | tail -1 && cd ../..`.
   Expect: `** BUILD SUCCEEDED **`.
2. Check the fixtures: `ls fixtures/raw | head`. Expect: RAW and/or JPEG files.
   If the folder is missing or empty (M0-01's fetch script has not run), create stand-in JPEGs with
   `swift apps/mac/Support/make-sample-folder.swift fixtures/raw 60`. Note in the verdict that you did this.
3. Clear the remembered folder so the app starts empty: `defaults delete dev.local.photoeditor 2>/dev/null; true`.
4. Launch it: `open apps/mac/build/PhotoEditor.app`. 📸
   Expect a dark window titled **PhotoEditor** with three areas:
   - a left sidebar with the sections Library, Folders, Albums and Smart Albums
   - a centre area reading **"No images"**, with the buttons **Open Folder…** and **Load 20,000 Stub Items**
   - a right inspector with the panels IMAGE, SELECTION and BASIC. BASIC holds sliders labelled
     Temperature … Saturation, each showing a value such as `+0`.

   The toolbar holds **Open Folder…**, a **Grid | Loupe** segmented control, a size slider, and the
   **Auto-advance** and **Inspector** buttons.

## B. Open the fixtures folder

5. Press **⌘O**. Expect an Open panel sheet ("Choose a folder of JPEG or RAW images").
6. In the sheet, press **⌘⇧G**, type the absolute path of `fixtures/raw` in this repository, press
   Return, then click **Open**. 📸
   Expect:
   - the centre area shows a **grid of thumbnails** with file names under them and a `G<n>` group
     label at the right of each caption
   - the window title is `raw` and the subtitle is `<N> images`
   - the status bar message starts `Opened raw: <N> images (<k> RAW), <g> groups`
   - cell 1 has an **amber border**, which marks the focus
   - the sidebar row **All Photos** shows N
   - a horizontal **filmstrip** of the same images runs along the bottom.

## C. Culling keys in the grid (auto-advance is on)

7. Click cell 1. Press **X**.
   Expect: cell 1 shows a red **REJECT** pill at top-left and its image is dimmed. The amber focus
   moves to cell 2. The status bar shows `Reject 1`.
8. Press **P**. Expect: cell 2 shows a green **KEEP** pill. Focus moves to cell 3.
9. Press **2**. Expect: cell 3 shows a green **GOOD 2** pill. Focus moves to cell 4. The status bar shows `Keep 2`.
10. Press **6**. Expect: cell 4 shows a magenta **6** chip at top-right. Focus stays on cell 4, because
    marks do not advance. The SELECTION panel shows `Mark: Needs Retouch`.
11. Press **B**. 📸 Expect: cell 4 shows a blue **BASKET** pill at bottom-left. The sidebar row
    **Basket** shows 1. The filmstrip cells show the same badges in compact form (X, K, 2, 6, B).
12. Press **A**. Expect: the status bar shows `Auto-advance off`.
    Press **3**. Expect: cell 4 shows **BEST 3** and focus stays on cell 4.
    Press **A** again. Expect: `Auto-advance on`.
13. Press **⌘Z**. Expect: the BEST 3 pill disappears from cell 4. Its 6 chip and BASKET pill remain.
    The status bar message reads `Undo: 1 image`.
14. Press **7**, then **7** again. Expect: the first press shows a yellow 7 chip and the second removes it (marks toggle).

## D. Group navigation

15. Read the group in the status bar (for example `G3 · 1/2`). Press **⌥→**.
    Expect: focus jumps to the first frame of the next group. The status bar shows the group number
    plus one and `· 1/<size>`. Press **⌥←** and expect focus to return to the start of the previous group.
16. Press **Return**. 📸 Expect **Loupe** mode:
    - the segmented control reads Loupe, and one large image fills the centre
    - the file name is at the top-left
    - a line at the top-right reads `<display name> · linear extended · RGBA16F · EDR headroom <x>× (max <y>×)`
    - a key hint line is at the bottom.
17. Press **→**. Expect: the status bar group number increases by 1 and the frame reads `1/<size>`.
    Press **→** again until the status bar shows a group whose size is 2 or more.
    Press **↓**. Expect: the same group, with the frame number increased by 1.
    Press **↑**. Expect: the frame number decreases by 1.
    Press **←**. Expect: the first frame of the previous group.
18. Press **X** in the loupe. Expect: the overlay briefly showed REJECT, then the view advanced to the next image.
    The status bar Reject count went up by 1.

## E. Inspector slider (AppKit NSControl) and loupe

19. In the loupe, drag the **Exposure** slider thumb in the BASIC panel to the right. 📸
    Expect: the value text follows the drag (for example `+1.50`) and the loupe image brightens
    **continuously while dragging**. Double-click the slider. Expect: the value resets to `+0.00` and
    the image returns to normal.
20. Press **Esc**. Expect: Grid mode again, with every badge from part C still visible.

## F. Sidebar filters and remembered folder

21. Click **Rejects** in the sidebar. Expect: only the rejected images show, and the subtitle reads
    `Rejects · <r> images`. Click **All Photos** and expect every image back.
22. Quit with **⌘Q**. Run `open apps/mac/build/PhotoEditor.app` again.
    Expect: the app reopens `fixtures/raw` without prompting, because it remembers the last folder.
    Decisions are gone, which is expected: this WP keeps them in memory only.
    Press ⌘O. Expect: the Open panel starts next to the `fixtures/raw` folder. Cancel it.

## G. 20,000-item performance

23. Choose **Debug ▸ Load 20,000 Stub Items** (⇧⌘N).
    Expect: the grid fills with generated gradient thumbnails numbered 1, 2, 3…, All Photos shows
    20,000, and the status bar reads `Generated 20,000 stub items in <g> groups`.
24. Scroll the grid quickly with the trackpad, or drag the scroller from top to bottom.
    Expect: scrolling stays smooth with no visible stalls, and thumbnails fill in within a moment of
    stopping.
25. Choose **Debug ▸ Run Grid Scroll Benchmark** (⇧⌘B). Do not touch the input for 10 s. 📸
    Expect: the grid auto-scrolls for 8 s. Then the status bar message starts
    `Scroll benchmark PASS: 20,000 items, <n> frames, <f> fps avg, p99 frame <≤17.5> ms, <≤1% of n> frames slower than 60 fps`.
    Alternatively, run from the command line:
    `apps/mac/build/PhotoEditor.app/Contents/MacOS/PhotoEditor --stub 20000 --benchmark` and read the
    same line on stderr.
26. Drag the grid scroller to the bottom, then click the last cell and press **X**.
    Expect: cell `20000` shows REJECT.

## H. Optional: screen change (needs a second display)

27. In the loupe, drag the window to a second display. Expect: the image redraws and the top-right
    colour line updates to the new display's name and EDR headroom.

## Verdict

PASS when steps 1–26 meet their expectations. Report the build output lines, the 📸 screenshots, and
the benchmark line from step 25.
