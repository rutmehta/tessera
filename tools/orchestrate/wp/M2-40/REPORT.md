# M2-40 report: People view

## Built
- `TesseraCore/Assist/People.swift`: `PeopleEngine` protocol (the FFI people calls in item ids), `CullController`
  conformance, and the `@Observable` `PeopleModel`: tiles (named first, then photos/faces/id), names and per-face
  confirmation from `person_assignments`, name suggestions (`people_name_suggestions`, filtered to unnamed → named),
  `name` (Return; trimmed, empty clears; Settings opt-ins, persisted), suggestion accept (= merge into the named person),
  multi-select merge (named/larger target wins), split (selected faces → new id `person-split-<uuid>`), reassign
  (drag / Move To), confirm / unconfirm / Confirm All, the Person facet (`frames_with_person`, union within, intersect
  with other facets), off-main `refresh_people(force:)`, approximation footnote, in-place library remap. Every edit
  reloads from `people(refresh: true)`.
- App: `LibrarySource.people` + sidebar row `People` (count), `PeopleView` (grid, tile name field + suggestions menu,
  context menu, detail view with face chips, confirm seals, Split, Move-to drop column, Esc/back), toolbar **Merge**
  (People only), Library ▸ Show People (⌥⌘P), filter bar **Person** facet (with Clear / match count), face strip
  tooltip uses the person's name and gains a context menu **Name… / Rename… / Show in People**, Settings ▸ **Library**
  tab ("Write face regions to XMP", "Add person keywords"). People view opens → `refresh_people(false)` off the main
  actor; also after library load and after Analyze Faces. Culling keys are ignored while People is shown.
- ACCEPTANCE.md §U steps 130–141 (+ fixture note: `--seed-faces` synthetic descriptors) and an identifiers appendix;
  DESIGN.md People view entry. Theme tokens only.

## Verified
- Gate `./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing`: exit 0; **110 XCTests, 0
  failures** (+5 Swift Testing tests passed), ThemeLint green. New: `PeopleModelTests` (10, stubbed engine: sorting,
  naming + opt-ins, suggestions, merge, split, reassign/confirm, facet intersection, off-main refresh + approximation,
  errors, remap) and `PeopleBridgeTests` (1, real engine session with seeded faces: name, default no XMP, split,
  confirm, reassign, merge keeps name, Person facet, and the opt-in writing `mwg-rs` regions + name to every XMP).
- Generated bindings unchanged by build-ffi.sh.

## Not verified
- The UI was not launched or screenshotted; ACCEPTANCE §U has not been walked by a person (layout, drag-and-drop feel,
  double-click vs click timing, keyboard focus of name fields are unverified).
- Real face models (Analyze Faces) and a >1024-face library (approximation footnote) were not exercised.

## FFI gaps (worked around, `assist.rs` unchanged)
- `PersonInfo` has no named flag / confirmation / member list: derived from `person_assignments` per image (one call per
  image with faces; fine at folder scale, O(images) per reload at library scale).
- `PeopleJobResult` reports `approximate` but not the sample size: the footnote uses the ml-faces reservoir constant
  1024 (`PeopleModel.clusteringSample`).
- Cover is the sharpest member (`cover_image`), not a true medoid face; the medoid is not exposed.
- People edits are not on the undo history (no FFI undo for merge/split/name).
