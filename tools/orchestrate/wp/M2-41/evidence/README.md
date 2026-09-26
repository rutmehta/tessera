M2-41 visual check (40-image sample folder, fresh --app-dir for each launch)

- `before-people-grid.png`: pre-fix seeded People grid. Two 136 pt tiles at the left.
- `before-detail-overflow.png`: pre-fix double-click on the first tile. The sidebar's AX frame starts at y=144 while the 900 pt window starts at y=645; the detail header is above the window and underlying photo chips are clipped. The hosted content measured 1954 pt tall in the regression test for a ~600 pt window.
- `after-seeded-launch.png`: fixed binary, fresh `--seed-faces` launch. Sidebar and photo grid/filter bar sit below the toolbar.
- `after-people-grid.png`: same run, Library > Show People. Full-width header and two 176 pt tiles flowing from the left.
- `after-detail-top.png`: same run, double-click on first tile. Header and first faces visible at the top, with the face list confined above the hint/status/filmstrip; sidebar stays in the window. The regression test measures 632 pt rather than 1954 pt for a small host.
- `after-unseeded-launch.png` and `after-unseeded-people.png`: fresh launch without `--seed-faces`, normal photo grid/filter bar and zero-people empty state.

The exact supplied launch-broken image did not appear on the first fresh launch here: `open -n` initially showed All Photos and the filter bar with and without seeding. Its `People · 40 images` subtitle and face-chip filename/badge style identify the detail's overflowing face grid, not a separate photo-grid cell style or a missing filter bar (the filter bar is intentionally hidden for People). Reproducing the detail transition gave the same window-height displacement; the fix bounds the canvas to the viewport with GeometryReader and top-aligns/bounds the People detail and its scroll view.
