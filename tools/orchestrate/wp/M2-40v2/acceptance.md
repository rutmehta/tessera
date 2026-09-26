# M2-40v2 — People view (focused re-run)

The app is already built at apps/mac/build/Tessera.app (do NOT rebuild; do not run cargo or swift build). Never edit source files. Capture only the Tessera window with the computer_use screenshot tool after every step. Read the "Identifiers" appendix at the end of apps/mac/ACCEPTANCE.md for control identifiers.

Setup: `SCR="$(mktemp -d)"; swift apps/mac/Support/make-sample-folder.swift "$SCR/people" 40; defaults delete dev.tessera.app 2>/dev/null; open -n apps/mac/build/Tessera.app --args --app-dir "$SCR/appdir-people" --folder "$SCR/people" --seed-faces`. Wait 5 s. 📸

Then perform these steps; for each, report pass/fail and describe exactly what the window shows (layout, overlaps, blank areas, truncation, off-theme colours):

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

Final step 200: press ⌘1 (or click All Photos in the sidebar) to return to the grid; 📸 confirm the sidebar and grid are intact (no blank sidebar, no grid rows drawn under the toolbar). Quit with ⌘Q.
