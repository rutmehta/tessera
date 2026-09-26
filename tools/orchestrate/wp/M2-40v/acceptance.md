# M2-40v — People view acceptance

Setup (do this first, from the repo root; never edit source files; capture only the app window with the computer_use screenshot tool):
1. Build: `export CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-40v; (cd apps/mac && ./build-ffi.sh && swift build && Support/make-app.sh release)`; the last line reads `Built …/apps/mac/build/Tessera.app`.
2. Scratch data: `SCR="$(mktemp -d)"; export TESSERA_APP_DIR="$SCR/appdir"; cp -RL fixtures/raw "$SCR/raw"`.
3. `defaults delete dev.tessera.app 2>/dev/null; true`.
4. Read the "Identifiers" appendix at the end of apps/mac/ACCEPTANCE.md for control identifiers; prefer keyboard-first interaction.
Then perform steps 130–141 below exactly as written; for each report pass/fail with what you observed. List visual defects (overlap, spacing, off-theme colours, truncation) in the notes. Finally, as step 142: quit, launch `open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-real" --folder "$SCR/raw"`, run Cull ▸ Analyze Faces (real models download once), wait for it to finish, open People and report whether real clusters appear and how the tiles look.

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
     and keeps its name) and one tile, `Ada L. · 40 photos`. People edits are not on the ⌘Z history: split to undo a
     merge. The approximation footnote (`people-approximate-note`, `Clustered from a sample of 1,024 faces`) appears
     under the grid only after a clustering job over more than 1,024 eligible faces; record whether a large library
     was tried.
141. **Tests.**
     ```sh
     (cd apps/mac && swift test --filter "PeopleModelTests|PeopleBridgeTests|ThemeLintTests" 2>&1 | grep "Executed")
     ```
     Expect `Executed 12 tests, with 0 failures`: the view model against a stubbed engine (naming and opt-ins,
     suggestions, merge, split, reassign / confirm, the Person facet's intersection, the off-main refresh and the
     approximation note, errors, in-place library updates) and the same calls through a real engine session.

