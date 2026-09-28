# Stage C lease impact on existing FFI tests

Source basis: `origin/main` at `049bfe95f219c0bb88b90be531b8f83cbfaf02b1`; test-only admission checkpoint `d61512ed4a9e9b2ca50deb7a7bf1ceec39007113`. Source-only audit; no tests/builds were run.

## Intentional premise changes in d61512ed

- `crates/tessera-ffi/src/develop.rs` test `open_develop_editor_does_not_overwrite_newer_engine_settings` becomes `...foreign_disk_settings`. Its old setup called `Engine::set_recipe_json` while its own Develop session was active. That is precisely the now-forbidden competing writer, so d615 replaces it with direct recipe/XMP sidecar publication and retains exact-byte assertions that a stale editor flush cannot overwrite the foreign fields. This preserves OwnerBaseline conflict detection, but the direct filesystem mutation is deliberately an out-of-gate foreign-writer fixture; it must not be mistaken for all-writer admission coverage.
- `second_develop_editor_cannot_replace_first_edit` is changed from two simultaneous successful sessions followed by a stale-flush conflict to fail-fast same-destination admission through a second Engine, checking the exact conflict and that no writer is constructed. It then edits/flushes/closes the first owner and successfully reopens through the second Engine, checking persisted settings and unchanged recipe bytes. This is the right replacement for lease semantics; it intentionally drops support for simultaneous editors on one destination, not the requirement that a later owner can edit after release.
- The new `direct_recipe_replacement_conflicts_without_publishing_during_develop` verifies active-session `set_recipe_json` rejection and no recipe/XMP/index publication. This is new coverage, not a deletion.

## Existing valid behavior that should remain covered

- `develop.rs::selection_during_develop_does_not_conflict_with_edited_settings` must continue to pass: selection is orthogonal and intentionally remains admissible while a Develop lease owns recipe fields.
- `develop.rs` legacy RGB first flush and both unknown nested-field conflict tests remain valid. They protect on-disk baseline capture before normalization and fail-closed preservation of unrepresented fields; lease acquisition must not weaken the owner-field comparison.
- `develop.rs::post_recipe_failure_advances_owner_baseline_for_newer_local_edit` and the post-recipe repair/failure tests must remain green; exclusive admission does not remove Stage A retry/repair semantics.
- `recipe_write_tests.rs::set_recipe_json_publishes_develop_settings_to_recipe_xmp_and_index`, `set_selection_retains_existing_xmp_develop_settings_and_description`, and `set_recipe_json_waits_for_the_same_destination_gate` have no active Develop owner. The last test is two ordinary Engine setters serialized by the existing write gate, not a second Develop editor. They should retain their current success/serialization behavior.
- Public API recipe round-trip/validation tests in `tests/api.rs` and the assist fixture setter in `tests/assist.rs` run without a Develop session; they should remain valid. The Swift binding exposes `setRecipeJson`, but source search found no production Swift call site invoking it.
- Sequential session uses in `tests/develop.rs` (close then reopen in `process_version_is_undoable_and_persisted`), HDR, masks, merge, and document tests are not simultaneous same-destination editor attempts. Ensure they close/drop sessions before a later writer operation where required. `tests/document.rs::open_document_from_image_is_the_developed_raw` obtains `info()` from a temporary Develop session; the temporary owner should be released on drop before subsequent document assertions. `Engine::depth_histogram` is the special internal read-only path called out in the Stage C plan: it must not create a competing writer lease while another editor is open, and should preserve its saved-recipe behavior.

## Suggested test follow-ups for the native owner

1. Keep d615's two behavioral REDs separate: second same-destination session must fail before writer construction; active-session direct `set_recipe_json` must fail before recipe/XMP/index changes. Preserve the exact conflict contract and selection control.
2. Add a post-release direct-setter control: after the active Develop session closes successfully, `set_recipe_json` should succeed and publish recipe/XMP/index. This distinguishes a scoped lease from a permanently poisoned destination gate.
3. Retain the ordinary two-Engine setter contention test (`set_recipe_json_waits_for_the_same_destination_gate`) unchanged; its held generic write gate is not a Develop lease and should still permit the setter after the held writer completes.
4. Keep the foreign-sidecar OwnerBaseline test labeled as external/foreign mutation coverage. Do not use it as evidence that participating Engine writers are serialized.
5. Audit histogram and temporary/session helper paths for accidental active-owner overlap; adapt only a genuinely concurrent same-destination writer premise, not sequential tests or read-only operations.

## Out of scope / caller note

The lease makes `set_recipe_json` return a conflict while a Develop owner is live. That is intentional in Stage C, but it is a public FFI behavior change for any non-repository caller that invokes this setter concurrently. In-repository Swift production code currently has no `setRecipeJson` invocation. Other writers such as Agent/Cull/import or direct sidecar processes are explicitly outside the proposed guarantee; do not imply the lease covers them.
