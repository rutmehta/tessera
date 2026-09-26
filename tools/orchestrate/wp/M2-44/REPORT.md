# M2-44 — People view adopts the new FFI

## Built

- `PersonSummary` carries `named`, `confirmedCount` and `medoid` (from `PersonInfo.named/confirmed_count/medoid_face`, the medoid mapped to an item id, nil when outside the library). `PeopleRefresh.sampleSize` carries `PeopleJobResult.sample_size`.
- `PeopleEngine` gains `personMembers(_:)`, `undoPeopleEdit()` and `redoPeopleEdit()`; `CullController` implements them over `person_members` / `undo_people_edit` / `redo_people_edit`.
- `PeopleModel.reload` builds tiles from `people()` alone. The per-image `person_assignments` scan is gone. Names come from `named`, counts from `confirmed_count`/`faces`, and `PersonTile.cover` is the medoid, falling back to the sharpest cover. `isConfirmed` is `confirmedCount >= faces`.
- Only the person open in the detail view carries `members`, loaded with one `person_members` call and sorted by item order. Each face's confirmation comes from the counts (none or all confirmed). Only a partly confirmed person reads its member photos' assignments, because `person_members` returns no confirmation flag. `members(_:)` exposes the same lookup. Reassign's "already there" check reads one photo's assignments.
- The footnote reads `Clustered from a sample of N faces`, where N is the job's reported sample size (1,024 if the job reported none).
- Undo/redo: `PeopleModel.undo()/redo()` call the engine, then reload from `people(refresh: false)` and notify `onPeopleChange`. A mirror of the engine history (one entry per successful engine edit, bounded to 32, redo cleared on a new edit, kept on an engine error, reset on install) supplies the menu titles. The engine has no way to read the next description before undoing, hence the mirror. `undoTitle`/`redoTitle` read like "Undo Merge People". `AppModel.undo/redo` route to people while `source == .people` (grid or detail). `undoMenuTitle`/`redoMenuTitle` title the Edit menu items in `AppCommands`.
- The view shows tile and detail counts from the engine counts. The ACCEPTANCE.md §U intro, step 140 (footnote text, removed "not on ⌘Z"), new step 140a (Undo/Redo), step 141 (filter, 17 tests) and the verdict are updated. No new accessibility identifiers.

## Verified

Gate `(cd apps/mac && ./build-ffi.sh && swift build && swift test -c release -Xswiftc -enable-testing)` with `CARGO_TARGET_DIR=/Volumes/betterSSD/tessera-cache/target/M2-44` exited 0: **117 tests, 0 failures**. PeopleModelTests 14 (10 existing plus 4 new: medoid tiles without an assignment scan, one-call detail members, sample-size footnote, undo/redo through the view model with the stub engine), PeopleBridgeTests 1 (now also undo/redo of a merge through a real session), PeopleLayoutTests 1, new PeopleUndoMenuTests 1 (AppModel Edit-menu routing and titles), ThemeLintTests 1. `build-ffi.sh` left the generated bindings unchanged.

Existing tests that read `members` from grid tiles now use `model.members(id)`, because grid tiles no longer carry members. Their intent is unchanged.

## Not verified

No manual UI walkthrough: step 140a and the menu titles in the running app were not checked. The mixed-confirmation detail path does one assignment read per member photo, so it is not a single call.
